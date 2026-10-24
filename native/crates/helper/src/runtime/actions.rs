use super::*;
use cu_core::{resolve_action, NativeAction};

impl<B: Backend, L> Runtime<B, L> {
    pub(super) fn act(
        &mut self,
        id: &str,
        action_id: &str,
        observation_id: &str,
        action: Action,
        version: u32,
    ) -> Result<Value> {
        check_action(&action, action_id, version)?;
        let fingerprint = serde_json::to_string(&(observation_id, &action))
            .map_err(|error| Error::new("invalid_params", error.to_string()))?;
        let session = self.session(id)?;
        if let Some((old, outcome)) = session.actions.get(action_id) {
            if old != &fingerprint {
                return Err(Error::new(
                    "action_id_conflict",
                    "Same action_id used for different arguments",
                ));
            }
            return Ok(json!(outcome));
        }
        if session.actions.len() >= 1024 {
            return Err(Error::new(
                "session_limit",
                "Action journal full; start a new helper",
            ));
        }
        let outcome = self
            .dispatch_action(id, action_id, observation_id, &action, &fingerprint)
            .unwrap_or_else(|error| {
                self.outcome(action_id, ActionState::Rejected, "none", error.to_string())
            });
        if let Some(session) = self.session.as_mut() {
            session
                .actions
                .insert(action_id.into(), (fingerprint, outcome.clone()));
        }
        Ok(json!(outcome))
    }

    fn dispatch_action(
        &mut self,
        id: &str,
        action_id: &str,
        observation_id: &str,
        action: &Action,
        fingerprint: &str,
    ) -> Result<ActionOutcome> {
        let observation = self.current_observation(id, observation_id)?;
        let native = resolve_action(&observation, action)?;
        self.backend.validate(&observation.window)?;
        self.validate_elements(&observation, action)?;
        if cfg!(target_os = "macos") || matches!(native, NativeAction::Semantic { .. }) {
            self.backend.preflight(&observation.window, &native)?;
        }
        cancelled(&self.cancel)?;
        let provisional = self.outcome(
            action_id,
            ActionState::Unknown,
            "pending",
            "Input dispatch may have begun",
        );
        let session = self.session(id)?;
        session.observation = None;
        session
            .actions
            .insert(action_id.into(), (fingerprint.into(), provisional));
        let result = self
            .backend
            .perform(&observation.window, &native, &self.cancel)
            .and_then(|mechanism| {
                cancelled(&self.cancel)?;
                Ok(mechanism)
            });
        let mut outcome = match result {
            Ok(mechanism) => self.outcome(
                action_id,
                ActionState::Dispatched,
                &mechanism,
                "OS accepted the operation; refreshed windows are available in window_refresh; observe a selected returned window before the next action",
            ),
            Err(error) => self.outcome(
                action_id,
                ActionState::Unknown,
                "native",
                format!("{error}; refreshed windows are available in window_refresh; do not replay this action automatically"),
            ),
        };
        match self.refresh_windows_after_action(id) {
            Ok((windows, settled, warnings)) => {
                outcome.window_refresh = Some(WindowRefresh {
                    attempted: true,
                    settled,
                    windows,
                    warnings,
                });
            }
            Err(error) => {
                let windows = self
                    .session
                    .as_ref()
                    .filter(|session| session.id == id)
                    .map(|session| session.windows.values().cloned().collect())
                    .unwrap_or_default();
                outcome.window_refresh = Some(WindowRefresh {
                    attempted: true,
                    settled: false,
                    windows,
                    warnings: vec![format!("Post-action window refresh failed: {error}")],
                });
            }
        }
        Ok(outcome)
    }

    fn current_observation(&mut self, id: &str, observation_id: &str) -> Result<Observation> {
        if !self.policy.allow_input {
            return Err(Error::new(
                "permission_denied",
                "Input is not approved by host policy",
            ));
        }
        let observation = self
            .session(id)?
            .observation
            .as_ref()
            .filter(|o| o.id == observation_id)
            .ok_or_else(|| {
                Error::new(
                    "stale_observation",
                    "Observe again; each observation may be used for one action",
                )
            })?;
        if now_ms() > observation.expires_at_ms {
            return Err(Error::new("stale_observation", "Observation expired"));
        }
        Ok(observation.clone())
    }

    fn validate_elements(&self, observation: &Observation, action: &Action) -> Result<()> {
        let targets = action_elements(action);
        if observation.elements.is_empty()
            || (targets.is_empty() && !matches!(action, Action::Drag { .. }))
        {
            return Ok(());
        }
        let fresh = self.backend.observe(&observation.window, false, true)?;
        for target in targets {
            let old = observation
                .elements
                .iter()
                .find(|e| e.id == target)
                .ok_or_else(|| Error::new("stale_reference", "Element not in observation"))?;
            if !fresh.elements.iter().any(|e| same_element(e, old)) {
                return Err(Error::new(
                    "stale_element",
                    "Referenced element changed; observe again",
                ));
            }
        }
        Ok(())
    }

    fn outcome(
        &self,
        action_id: &str,
        state: ActionState,
        mechanism: &str,
        message: impl Into<String>,
    ) -> ActionOutcome {
        ActionOutcome {
            action_id: action_id.into(),
            helper_epoch: self.epoch.clone(),
            state,
            mechanism: mechanism.into(),
            message: message.into(),
            window_refresh: None,
        }
    }

    pub(super) fn action_status(&self, id: &str, action_id: &str) -> Result<Value> {
        let session = self
            .session
            .as_ref()
            .filter(|s| s.id == id)
            .ok_or_else(|| Error::new("invalid_session", "No matching session"))?;
        session
            .actions
            .get(action_id)
            .map(|(_, outcome)| json!(outcome))
            .ok_or_else(|| {
                Error::new(
                    "unknown_action",
                    "Action ID was not recorded in this helper epoch",
                )
            })
    }
}

fn check_action(action: &Action, action_id: &str, version: u32) -> Result<()> {
    if version == VERSION
        && (matches!(
            action,
            Action::Invoke { .. } | Action::Toggle { .. } | Action::Select { .. }
        ) || cfg!(windows) && matches!(action, Action::SetValue { .. }))
    {
        return Err(Error::new(
            "unsupported_action",
            "Windows semantic actions require protocol profile 2",
        ));
    }
    if action_id.is_empty() || action_id.len() > 128 {
        return Err(Error::new(
            "invalid_params",
            "action_id must be 1..128 characters",
        ));
    }
    Ok(())
}

fn same_element(current: &Element, old: &Element) -> bool {
    current.locator == old.locator
        && current.role == old.role
        && current.name == old.name
        && current.bounds == old.bounds
        && current.password == old.password
        && current.value == old.value
}

fn action_elements(action: &Action) -> Vec<&str> {
    fn add<'a>(out: &mut Vec<&'a str>, target: &'a Target) {
        if let Target::Element { element_id } = target {
            out.push(element_id.as_str());
        }
    }
    let mut out = vec![];
    match action {
        Action::Click { target, .. } | Action::Scroll { target, .. } => add(&mut out, target),
        Action::Drag { from, to, .. } => {
            add(&mut out, from);
            add(&mut out, to);
        }
        Action::SetValue { element_id, .. }
        | Action::Invoke { element_id }
        | Action::Toggle { element_id }
        | Action::Select { element_id } => out.push(element_id.as_str()),
        _ => {}
    }
    out
}
