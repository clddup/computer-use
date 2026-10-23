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
    fn end_session(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        cu_platform_macos::close_presentation()?;
        Ok(())
    }
    fn capabilities(&self) -> Capabilities {
        backend().capabilities()
    }

    fn list_windows(&self) -> Result<Vec<Window>> {
        let windows: Vec<Window> = self.query(Probe::List)?;
        #[cfg(target_os = "macos")]
        let windows = windows
            .into_iter()
            .filter(|window| !cu_platform_macos::is_presentation_window(&window.id))
            .collect();
        Ok(windows)
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
        let observation: Observation = self.query(Probe::Observe {
            window: window.clone(),
            screenshot: screenshot && !cfg!(target_os = "macos"),
            accessibility,
        })?;
        #[cfg(target_os = "macos")]
        let observation = {
            let mut observation = observation;
            cu_platform_macos::present(window, screenshot)?;
            if screenshot {
                observation.images = vec![cu_platform_macos::capture(window, &self.cancel)?];
            }
            observation
        };
        Ok(observation)
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
            #[cfg(not(target_os = "macos"))]
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
