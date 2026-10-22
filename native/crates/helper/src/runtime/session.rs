use super::*;
use crate::watchdog::expiry_watchdog;
use std::{collections::HashSet, time::Duration};

pub(super) struct Session<L> {
    pub(super) id: String,
    pub(super) expires: u64,
    pub(super) _lease: L,
    pub(super) processes: HashMap<u32, String>,
    pub(super) pending_apps: HashSet<String>,
    pub(super) windows: HashMap<String, Window>,
    pub(super) observation: Option<Observation>,
    pub(super) actions: HashMap<String, (String, ActionOutcome)>,
}

impl<B: Backend, L> Runtime<B, L> {
    pub(super) fn session(&mut self, id: &str) -> Result<&mut Session<L>> {
        cancelled(&self.cancel)?;
        let session = self
            .session
            .as_mut()
            .filter(|s| s.id == id)
            .ok_or_else(|| Error::new("invalid_session", "No matching active session"))?;
        if now_ms() > session.expires {
            self.cancel.store(true, Ordering::SeqCst);
            return Err(Error::new(
                "session_expired",
                "Session expired; stop and create a new helper",
            ));
        }
        Ok(session)
    }

    pub(super) fn start(&mut self) -> Result<Value> {
        self.check_start()?;
        let lease = (self.acquire_lease)()?;
        let processes: HashMap<_, _> = self
            .backend
            .list_windows()?
            .into_iter()
            .filter(|w| self.policy.allowed_pids.contains(&w.pid))
            .map(|w| (w.pid, w.process_identity))
            .collect();
        if processes.is_empty() && self.policy.allowed_apps.is_empty() {
            return Err(Error::new(
                "permission_denied",
                "No visible approved process windows and no approved launch profile",
            ));
        }
        let id = new_id();
        let lifetime = self.policy.session_seconds.clamp(1, 3600);
        let expires = now_ms() + lifetime * 1000;
        self.session = Some(Session {
            id: id.clone(),
            expires,
            _lease: lease,
            processes,
            pending_apps: HashSet::new(),
            windows: HashMap::new(),
            observation: None,
            actions: HashMap::new(),
        });
        self.started = true;
        expiry_watchdog(self.cancel.clone(), Duration::from_secs(lifetime));
        Ok(json!({"session_id":id,"helper_epoch":self.epoch,"expires_at_ms":expires}))
    }

    fn check_start(&self) -> Result<()> {
        if self.started {
            return Err(Error::new(
                "session_exists",
                "One session per helper; restart the helper to start again",
            ));
        }
        cancelled(&self.cancel)?;
        if self.policy.allowed_pids.is_empty() && self.policy.allowed_apps.is_empty() {
            return Err(Error::new(
                "permission_denied",
                "Host policy has no approved PIDs or applications",
            ));
        }
        let caps = self.backend.capabilities();
        if !caps.available {
            return Err(Error::new(
                "unsupported_environment",
                caps.reason.unwrap_or_default(),
            ));
        }
        Ok(())
    }

    pub(super) fn stop(&mut self, id: &str) -> Result<Value> {
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.id != id)
        {
            return Err(Error::new("invalid_session", "Stop session mismatch"));
        }
        self.cancel.store(true, Ordering::SeqCst);
        // Dispatch is serial: all native input guards have dropped before the lease.
        self.session = None;
        Ok(json!({"stopped":true,"helper_epoch":self.epoch}))
    }
}
