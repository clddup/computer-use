use super::*;

impl<B: Backend, L> Runtime<B, L> {
    pub(super) fn list_windows(&mut self, id: &str) -> Result<Value> {
        self.refresh_pending_apps(id)?;
        let mut windows = self.backend.list_windows()?;
        let session = self.session(id)?;
        windows.retain(|w| session.processes.get(&w.pid) == Some(&w.process_identity));
        session.windows = windows.iter().map(|w| (w.id.clone(), w.clone())).collect();
        Ok(json!(windows))
    }

    fn known_window(&mut self, id: &str, window: &Window) -> Result<()> {
        if self.session(id)?.windows.get(&window.id) != Some(window) {
            return Err(Error::new(
                "stale_window",
                "List windows again and use the returned window reference",
            ));
        }
        Ok(())
    }

    pub(super) fn get_window(&mut self, id: &str, window: &Window) -> Result<Value> {
        self.known_window(id, window)?;
        self.backend.validate(window)?;
        self.session(id)?;
        Ok(json!(window))
    }

    pub(super) fn activate_window(&mut self, id: &str, window: &Window) -> Result<Value> {
        self.known_window(id, window)?;
        self.backend.validate(window)?;
        self.session(id)?.observation = None;
        self.backend.activate_window(window, &self.cancel)?;
        self.backend
            .validate(window)
            .map_err(Error::after_dispatch)?;
        self.session(id).map_err(Error::after_dispatch)?;
        Ok(json!({"activated":true}))
    }

    pub(super) fn observe(
        &mut self,
        id: &str,
        window: &Window,
        screenshot: bool,
        accessibility: bool,
        version: u32,
    ) -> Result<Value> {
        self.check_observe(id, window, screenshot, accessibility)?;
        self.session(id)?.observation = None;
        let mut observation = self.backend.observe(window, screenshot, accessibility)?;
        observation.helper_epoch = self.epoch.clone();
        if version == VERSION {
            for element in &mut observation.elements {
                element.capabilities = None;
            }
        }
        if &observation.window != window {
            return Err(Error::new(
                "stale_window",
                "Worker returned a different window",
            ));
        }
        self.session(id)?.observation = Some(observation.clone());
        Ok(json!(observation))
    }

    fn check_observe(
        &mut self,
        id: &str,
        window: &Window,
        screenshot: bool,
        accessibility: bool,
    ) -> Result<()> {
        let pid_allowed = self.policy.allowed_pids.contains(&window.pid);
        let session = self.session(id)?;
        if !pid_allowed && session.processes.get(&window.pid) != Some(&window.process_identity) {
            return Err(Error::new(
                "permission_denied",
                "Window is outside the approved application session",
            ));
        }
        if screenshot && !self.policy.allow_screenshot
            || accessibility && !self.policy.allow_accessibility
        {
            return Err(Error::new(
                "permission_denied",
                "Capture/read capability is not approved by host policy",
            ));
        }
        self.known_window(id, window)
    }
}
