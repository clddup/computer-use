mod actions;
mod applications;
mod session;
#[cfg(test)]
mod tests;
mod windows;

use cu_core::{cancelled, new_id, now_ms, Backend, DesktopLease, Policy};
use cu_protocol::*;
use serde_json::{json, Value};
use session::Session;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub(crate) struct Runtime<B, L = DesktopLease> {
    backend: B,
    acquire_lease: fn() -> Result<L>,
    policy: Policy,
    epoch: String,
    session: Option<Session<L>>,
    started: bool,
    cancel: Arc<AtomicBool>,
    interruption: Option<Error>,
}

impl<B: Backend, L> Runtime<B, L> {
    pub(crate) fn new(
        policy: Policy,
        backend: B,
        cancel: Arc<AtomicBool>,
        acquire_lease: fn() -> Result<L>,
    ) -> Self {
        Self {
            backend,
            acquire_lease,
            policy,
            epoch: new_id(),
            session: None,
            started: false,
            cancel,
            interruption: None,
        }
    }

    pub(crate) fn respond(&mut self, request: Request) -> Response {
        let (result, error) = match self.handle(request.command, request.version) {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(error)),
        };
        Response {
            version: request.version,
            id: request.id,
            result,
            error,
        }
    }

    pub(crate) fn handle(&mut self, command: Command, version: u32) -> Result<Value> {
        self.dispatch(command, version).map_err(|error| {
            if error.code == "cancelled" {
                if let Some(mut cause) = self.interruption.clone() {
                    cause.may_have_executed |= error.may_have_executed;
                    return cause;
                }
            }
            error
        })
    }

    fn dispatch(&mut self, command: Command, version: u32) -> Result<Value> {
        match command {
            Command::Capabilities {} => Ok(json!(self.capabilities(version))),
            Command::Start {} => self.start(),
            Command::ListApps { session_id } => self.list_apps(&session_id),
            Command::LaunchApp { session_id, app_id } => self.launch_app(&session_id, &app_id),
            Command::ListWindows { session_id } => self.list_windows(&session_id),
            Command::GetWindow { session_id, window } => self.get_window(&session_id, &window),
            Command::ActivateWindow { session_id, window } => {
                self.activate_window(&session_id, &window)
            }
            Command::Observe {
                session_id,
                window,
                screenshot,
                accessibility,
            } => self.observe(&session_id, &window, screenshot, accessibility, version),
            Command::Act {
                session_id,
                action_id,
                observation_id,
                action,
            } => self.act(&session_id, &action_id, &observation_id, action, version),
            Command::ActionStatus {
                session_id,
                action_id,
            } => self.action_status(&session_id, &action_id),
            Command::Stop { session_id } => self.stop(&session_id),
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn interrupt(&mut self, error: Error) {
        if self.interruption.is_none() {
            eprintln!(
                "Control session interrupted: {}: {}",
                error.code, error.message
            );
            self.interruption = Some(error);
        }
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn capabilities(&self, version: u32) -> Capabilities {
        let mut caps = self.backend.capabilities();
        caps.protocol_version = MAX_VERSION;
        if version == VERSION {
            caps.actions.retain(|a| {
                !(["invoke", "toggle", "select"].contains(&a.as_str())
                    || cfg!(windows) && a == "set_value")
            });
        }
        caps
    }

    pub(crate) fn release_if_cancelled(&mut self) {
        if self.session.as_ref().is_some_and(|s| now_ms() > s.expires) {
            self.cancel.store(true, Ordering::SeqCst);
        }
        if self.cancel.load(Ordering::SeqCst) {
            self.session = None;
        }
    }
}
