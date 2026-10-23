use super::*;

impl<B: Backend, L> Runtime<B, L> {
    pub(super) fn list_apps(&mut self, id: &str) -> Result<Value> {
        self.session(id)?;
        let mut apps = self.backend.list_apps(&self.policy.allowed_apps)?;
        if !self.policy.desktop {
            let profiles = self.policy.allowed_apps.clone();
            let session = self.session(id)?;
            apps.retain(|app| {
                profiles.iter().any(|p| p.id == app.id)
                    || app
                        .windows
                        .iter()
                        .any(|w| session.processes.get(&w.pid) == Some(&w.process_identity))
            });
        } else {
            let session = self.session(id)?;
            for app in &apps {
                bind_windows(session, &app.windows);
            }
        }
        self.bind_pending_apps(id, &apps)?;
        Ok(json!(apps))
    }

    pub(super) fn launch_app(&mut self, id: &str, app_id: &str) -> Result<Value> {
        if app_id.is_empty() || app_id.len() > 128 {
            return Err(Error::new(
                "invalid_params",
                "app_id must be 1..128 characters",
            ));
        }
        self.session(id)?;
        let desktop = self.policy.desktop;
        let app = self
            .policy
            .allowed_apps
            .iter()
            .find(|app| app.id == app_id)
            .cloned()
            .or_else(|| {
                self.policy.desktop.then(|| cu_core::ApplicationPolicy {
                    id: app_id.into(),
                    display_name: app_id.into(),
                    executable: app_id.into(),
                    args: vec![],
                })
            })
            .ok_or_else(|| {
                Error::new("permission_denied", "Application is not in the host policy")
            })?;
        let windows = match self.backend.launch_app(&app, &self.cancel) {
            Ok(windows) => windows,
            Err(error) => {
                if error.may_have_executed && !desktop {
                    if let Some(session) = self.session.as_mut() {
                        session.pending_apps.insert(app_id.into());
                    }
                }
                return Err(error);
            }
        };
        let session = self.session(id).map_err(Error::after_dispatch)?;
        if windows.is_empty() {
            if !desktop {
                session.pending_apps.insert(app_id.into());
            }
        } else {
            bind_windows(session, &windows);
            session.pending_apps.remove(app_id);
        }
        Ok(launch_result(app_id, windows))
    }

    pub(super) fn refresh_pending_apps(&mut self, id: &str) -> Result<()> {
        if self.session(id)?.pending_apps.is_empty() {
            return Ok(());
        }
        let apps = self.backend.list_apps(&self.policy.allowed_apps)?;
        self.bind_pending_apps(id, &apps)
    }

    fn bind_pending_apps(&mut self, id: &str, apps: &[Application]) -> Result<()> {
        let session = self.session(id)?;
        for app in apps {
            if !app.windows.is_empty() && session.pending_apps.remove(&app.id) {
                bind_windows(session, &app.windows);
            }
        }
        Ok(())
    }
}

fn bind_windows<L>(session: &mut Session<L>, windows: &[Window]) {
    for window in windows {
        session
            .processes
            .insert(window.pid, window.process_identity.clone());
        session.windows.insert(window.id.clone(), window.clone());
    }
}

fn launch_result(app_id: &str, windows: Vec<Window>) -> Value {
    let (state, message) = if windows.is_empty() {
        ("pending", "Launch was dispatched but no visible window is ready; list applications or windows again")
    } else {
        ("ready", "Application has an approved visible window")
    };
    json!(LaunchResult {
        app_id: app_id.into(),
        state: state.into(),
        windows,
        message: message.into()
    })
}
