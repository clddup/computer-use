use crate::{platform::backend, worker::Probe, worker_query::query};
use cu_core::{ApplicationPolicy, Backend, NativeAction};
use cu_protocol::*;
use serde::de::DeserializeOwned;
use std::sync::{atomic::AtomicBool, Arc};

pub(crate) struct WorkerBackend {
    cancel: Arc<AtomicBool>,
}

impl WorkerBackend {
    pub(crate) fn new(cancel: Arc<AtomicBool>) -> Self {
        Self { cancel }
    }

    fn query<T: DeserializeOwned>(&self, probe: Probe) -> Result<T> {
        let may_execute = probe.may_execute();
        serde_json::from_value(query(probe, &self.cancel)?).map_err(|error| {
            let error = Error::new("worker_failed", error.to_string());
            if may_execute {
                error.after_dispatch()
            } else {
                error
            }
        })
    }
}

impl Backend for WorkerBackend {
    fn capabilities(&self) -> Capabilities {
        backend().capabilities()
    }

    fn list_windows(&self) -> Result<Vec<Window>> {
        self.query(Probe::List)
    }

    fn list_apps(&self, apps: &[ApplicationPolicy]) -> Result<Vec<Application>> {
        backend().list_apps(apps)
    }

    fn launch_app(&self, app: &ApplicationPolicy, cancel: &AtomicBool) -> Result<Vec<Window>> {
        backend().launch_app(app, cancel)
    }

    fn activate_window(&self, window: &Window, _cancel: &AtomicBool) -> Result<()> {
        let _: serde_json::Value = self.query(Probe::Activate {
            window: window.clone(),
        })?;
        Ok(())
    }

    fn observe(
        &self,
        window: &Window,
        screenshot: bool,
        accessibility: bool,
    ) -> Result<Observation> {
        self.query(Probe::Observe {
            window: window.clone(),
            screenshot,
            accessibility,
        })
    }

    fn validate(&self, window: &Window) -> Result<()> {
        let _: serde_json::Value = self.query(Probe::Validate {
            window: window.clone(),
        })?;
        Ok(())
    }

    fn preflight(&self, window: &Window, action: &NativeAction) -> Result<()> {
        let _: serde_json::Value = self.query(Probe::Semantic {
            window: window.clone(),
            action: Box::new(action.clone()),
            execute: false,
        })?;
        Ok(())
    }

    fn perform(
        &self,
        window: &Window,
        action: &NativeAction,
        cancel: &AtomicBool,
    ) -> Result<String> {
        if matches!(action, NativeAction::Semantic { .. }) {
            self.query(Probe::Semantic {
                window: window.clone(),
                action: Box::new(action.clone()),
                execute: true,
            })
        } else {
            query(
                Probe::Prepare {
                    window: window.clone(),
                },
                &self.cancel,
            )?;
            backend().perform(window, action, cancel)
        }
    }
}
