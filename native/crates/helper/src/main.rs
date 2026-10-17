use cu_core::{cancelled, new_id, now_ms, resolve_action, Backend, DesktopLease, Policy};
use cu_protocol::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{self, BufRead, Read, Write},
    process::{Command as ProcessCommand, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
fn backend() -> impl Backend {
    cu_platform_windows::WindowsBackend
}
#[cfg(target_os = "macos")]
fn backend() -> impl Backend {
    cu_platform_macos::MacosBackend
}
#[cfg(target_os = "linux")]
fn backend() -> impl Backend {
    cu_platform_linux::LinuxBackend
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum Probe {
    List,
    Observe {
        window: Window,
        screenshot: bool,
        accessibility: bool,
    },
    Validate {
        window: Window,
    },
    Prepare {
        window: Window,
    },
    Activate {
        window: Window,
    },
}

fn output(response: &Response) {
    let bytes = serde_json::to_vec(response).unwrap_or_default();
    let mut stdout = io::stdout().lock();
    if bytes.len() > MAX_FRAME {
        let fallback = Response {
            version: VERSION,
            id: response.id.clone(),
            result: None,
            error: Some(Error::new(
                "frame_limit",
                "Response exceeds protocol frame limit",
            )),
        };
        let _ = serde_json::to_writer(&mut stdout, &fallback);
    } else {
        let _ = stdout.write_all(&bytes);
    }
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}
fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let data = reader.fill_buf()?;
        if data.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Partial NDJSON frame",
                ))
            };
        }
        let count = data
            .iter()
            .position(|b| *b == b'\n')
            .map(|v| v + 1)
            .unwrap_or(data.len());
        if frame.len() + count > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "NDJSON frame exceeds limit",
            ));
        }
        frame.extend_from_slice(&data[..count]);
        reader.consume(count);
        if frame.last() == Some(&b'\n') {
            return Ok(Some(frame));
        }
    }
}
fn query(probe: Probe, cancel: &AtomicBool) -> Result<Value> {
    cancelled(cancel)?;
    let mut command = ProcessCommand::new(
        std::env::current_exe().map_err(|e| Error::new("worker_failed", e.to_string()))?,
    );
    command
        .arg("--probe")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| Error::new("worker_failed", e.to_string()))?;
    let payload =
        serde_json::to_vec(&probe).map_err(|e| Error::new("worker_failed", e.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(&payload).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::new("worker_failed", "Worker input failed"));
        }
    }
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take((MAX_FRAME + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(12);
    let result = loop {
        if cancel.load(Ordering::SeqCst) || Instant::now() > deadline {
            let _ = child.kill();
            break Err(Error::new(
                if cancel.load(Ordering::SeqCst) {
                    "cancelled"
                } else {
                    "query_timeout"
                },
                "Accessibility/capture worker was terminated",
            ));
        }
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(Ok(bytes)) => {
                if bytes.len() > MAX_FRAME {
                    break Err(Error::new("frame_limit", "Worker response exceeds limit"));
                }
                break serde_json::from_slice::<std::result::Result<Value, Error>>(&bytes)
                    .map_err(|e| Error::new("worker_failed", e.to_string()))
                    .and_then(|r| r);
            }
            Ok(Err(e)) => break Err(Error::new("worker_failed", e.to_string())),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err(Error::new("worker_failed", "Worker output closed"))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    };
    let _ = child.wait();
    let _ = reader.join();
    result
}
fn probe_main() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let mut bytes = vec![];
    io::stdin()
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::new("invalid_params", e.to_string()))?;
    let result = serde_json::from_slice::<Probe>(&bytes)
        .map_err(|e| Error::new("invalid_params", e.to_string()))
        .and_then(|p| match p {
            Probe::List => backend().list_windows().map(|v| json!(v)),
            Probe::Observe {
                window,
                screenshot,
                accessibility,
            } => backend()
                .observe(&window, screenshot, accessibility)
                .map(|v| json!(v)),
            Probe::Validate { window } => {
                backend().validate(&window).map(|_| json!({"valid":true}))
            }
            Probe::Prepare { window } => {
                backend().prepare(&window).map(|_| json!({"prepared":true}))
            }
            Probe::Activate { window } => backend()
                .activate_window(&window, &cancel)
                .map(|_| json!({"activated":true})),
        });
    serde_json::to_writer(io::stdout(), &result)
        .map_err(|e| Error::new("worker_failed", e.to_string()))?;
    Ok(())
}

fn expiry_watchdog(cancel: Arc<AtomicBool>, lifetime: Duration) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let deadline = Instant::now() + lifetime;
        while !cancel.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                cancel.store(true, Ordering::SeqCst);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    })
}

struct Session {
    id: String,
    expires: u64,
    _lease: DesktopLease,
    processes: HashMap<u32, String>,
    windows: HashMap<String, Window>,
    observation: Option<Observation>,
    actions: HashMap<String, (String, ActionOutcome)>,
}
struct Runtime {
    policy: Policy,
    epoch: String,
    session: Option<Session>,
    started: bool,
    cancel: Arc<AtomicBool>,
}
impl Runtime {
    fn session(&mut self, id: &str) -> Result<&mut Session> {
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
    fn handle(&mut self, command: Command) -> Result<Value> {
        match command {
            Command::Capabilities {} => Ok(json!(backend().capabilities())),
            Command::Start {} => {
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
                let capability = backend().capabilities();
                if !capability.available {
                    return Err(Error::new(
                        "unsupported_environment",
                        capability.reason.unwrap_or_default(),
                    ));
                }
                let lease = DesktopLease::acquire()?;
                let windows: Vec<Window> =
                    serde_json::from_value(query(Probe::List, &self.cancel)?)
                        .map_err(|e| Error::new("worker_failed", e.to_string()))?;
                let processes: HashMap<_, _> = windows
                    .iter()
                    .filter(|w| self.policy.allowed_pids.contains(&w.pid))
                    .map(|w| (w.pid, w.process_identity.clone()))
                    .collect();
                if processes.is_empty() && self.policy.allowed_apps.is_empty() {
                    return Err(Error::new(
                        "permission_denied",
                        "No visible approved process windows and no approved launch profile",
                    ));
                }
                let id = new_id();
                let expires = now_ms() + self.policy.session_seconds.clamp(1, 3600) * 1000;
                self.session = Some(Session {
                    id: id.clone(),
                    expires,
                    _lease: lease,
                    processes,
                    windows: HashMap::new(),
                    observation: None,
                    actions: HashMap::new(),
                });
                self.started = true;
                expiry_watchdog(
                    self.cancel.clone(),
                    Duration::from_secs(self.policy.session_seconds.clamp(1, 3600)),
                );
                Ok(json!({"session_id":id,"helper_epoch":self.epoch,"expires_at_ms":expires}))
            }
            Command::ListApps { session_id } => {
                self.session(&session_id)?;
                Ok(json!(backend().list_apps(&self.policy.allowed_apps)?))
            }
            Command::LaunchApp { session_id, app_id } => {
                if app_id.is_empty() || app_id.len() > 128 {
                    return Err(Error::new(
                        "invalid_params",
                        "app_id must be 1..128 characters",
                    ));
                }
                self.session(&session_id)?;
                let app = self
                    .policy
                    .allowed_apps
                    .iter()
                    .find(|app| app.id == app_id)
                    .cloned()
                    .ok_or_else(|| {
                        Error::new("permission_denied", "Application is not in the host policy")
                    })?;
                let windows = backend().launch_app(&app, &self.cancel)?;
                let session = self.session(&session_id)?;
                for window in &windows {
                    session
                        .processes
                        .insert(window.pid, window.process_identity.clone());
                    session.windows.insert(window.id.clone(), window.clone());
                }
                let (state, message) = if windows.is_empty() {
                    ("pending", "Launch was dispatched but no unique visible window is ready; list applications again".to_string())
                } else {
                    (
                        "ready",
                        "Application has an approved visible window".to_string(),
                    )
                };
                Ok(json!(LaunchResult {
                    app_id,
                    state: state.into(),
                    windows,
                    message
                }))
            }
            Command::ListWindows { session_id } => {
                self.session(&session_id)?;
                let mut windows: Vec<Window> =
                    serde_json::from_value(query(Probe::List, &self.cancel)?)
                        .map_err(|e| Error::new("worker_failed", e.to_string()))?;
                let session = self.session(&session_id)?;
                windows.retain(|w| session.processes.get(&w.pid) == Some(&w.process_identity));
                session.windows = windows.iter().map(|w| (w.id.clone(), w.clone())).collect();
                Ok(json!(windows))
            }
            Command::GetWindow { session_id, window } => {
                let session = self.session(&session_id)?;
                session
                    .windows
                    .get(&window.id)
                    .filter(|known| *known == &window)
                    .cloned()
                    .ok_or_else(|| {
                        Error::new(
                            "stale_window",
                            "List windows again and use the returned window reference",
                        )
                    })
                    .map(|window| json!(window))
            }
            Command::ActivateWindow { session_id, window } => {
                let session = self.session(&session_id)?;
                if session.windows.get(&window.id) != Some(&window) {
                    return Err(Error::new(
                        "stale_window",
                        "List windows again and use the returned window reference",
                    ));
                }
                query(Probe::Activate { window }, &self.cancel)?;
                Ok(json!({"activated":true}))
            }
            Command::Observe {
                session_id,
                window,
                screenshot,
                accessibility,
            } => {
                let pid_allowed = self.policy.allowed_pids.contains(&window.pid);
                let session = self.session(&session_id)?;
                if !pid_allowed
                    && session.processes.get(&window.pid) != Some(&window.process_identity)
                {
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
                let session = self.session(&session_id)?;
                if session.windows.get(&window.id) != Some(&window) {
                    return Err(Error::new(
                        "stale_window",
                        "List windows again and use the returned window reference",
                    ));
                }
                session.observation = None;
                let mut observation: Observation = serde_json::from_value(query(
                    Probe::Observe {
                        window: window.clone(),
                        screenshot,
                        accessibility,
                    },
                    &self.cancel,
                )?)
                .map_err(|e| Error::new("worker_failed", e.to_string()))?;
                observation.helper_epoch = self.epoch.clone();
                if observation.window != window {
                    return Err(Error::new(
                        "stale_window",
                        "Worker returned a different window",
                    ));
                }
                self.session(&session_id)?.observation = Some(observation.clone());
                Ok(json!(observation))
            }
            Command::Act {
                session_id,
                action_id,
                observation_id,
                action,
            } => {
                if action_id.is_empty() || action_id.len() > 128 {
                    return Err(Error::new(
                        "invalid_params",
                        "action_id must be 1..128 characters",
                    ));
                }
                let fingerprint =
                    serde_json::to_string(&(observation_id.clone(), action.clone())).unwrap();
                let epoch = self.epoch.clone();
                let session = self.session(&session_id)?;
                if let Some((old, outcome)) = session.actions.get(&action_id) {
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
                let outcome = (|| -> Result<ActionOutcome> {
                    if !self.policy.allow_input {
                        return Err(Error::new(
                            "permission_denied",
                            "Input is not approved by host policy",
                        ));
                    }
                    let session = self.session(&session_id)?;
                    let observation = session
                        .observation
                        .as_ref()
                        .filter(|o| o.id == observation_id)
                        .ok_or_else(|| {
                            Error::new(
                                "stale_observation",
                                "Observe again; each observation may be used for one action",
                            )
                        })?
                        .clone();
                    if now_ms() > observation.expires_at_ms {
                        return Err(Error::new("stale_observation", "Observation expired"));
                    }
                    let native = resolve_action(&observation, &action)?;
                    query(
                        Probe::Validate {
                            window: observation.window.clone(),
                        },
                        &self.cancel,
                    )?;
                    // Verify that a referenced element still has the same role, value and bounds.
                    if matches!(
                        &action,
                        Action::Click {
                            target: Target::Element { .. },
                            ..
                        } | Action::SetValue { .. }
                            | Action::Scroll {
                                target: Target::Element { .. },
                                ..
                            }
                            | Action::Drag { .. }
                    ) && !observation.elements.is_empty()
                    {
                        let fresh: Observation = serde_json::from_value(query(
                            Probe::Observe {
                                window: observation.window.clone(),
                                screenshot: false,
                                accessibility: true,
                            },
                            &self.cancel,
                        )?)
                        .map_err(|e| Error::new("worker_failed", e.to_string()))?;
                        for target in action_elements(&action) {
                            let old = observation
                                .elements
                                .iter()
                                .find(|e| e.id == target)
                                .ok_or_else(|| {
                                    Error::new("stale_reference", "Element not in observation")
                                })?;
                            if !fresh.elements.iter().any(|e| {
                                e.locator == old.locator
                                    && e.role == old.role
                                    && e.name == old.name
                                    && e.bounds == old.bounds
                                    && e.password == old.password
                                    && e.value == old.value
                            }) {
                                return Err(Error::new(
                                    "stale_element",
                                    "Referenced element changed; observe again",
                                ));
                            }
                        }
                    }
                    cancelled(&self.cancel)?;
                    let provisional = ActionOutcome {
                        action_id: action_id.clone(),
                        helper_epoch: epoch.clone(),
                        state: ActionState::Unknown,
                        mechanism: "pending".into(),
                        message: "Input dispatch may have begun".into(),
                    };
                    let session = self.session(&session_id)?;
                    session.observation = None;
                    session
                        .actions
                        .insert(action_id.clone(), (fingerprint.clone(), provisional));
                    let result = query(
                        Probe::Prepare {
                            window: observation.window.clone(),
                        },
                        &self.cancel,
                    )
                    .and_then(|_| backend().perform(&observation.window, &native, &self.cancel));
                    Ok(match result {
                        Ok(mechanism)=>ActionOutcome{action_id:action_id.clone(),helper_epoch:epoch.clone(),state:ActionState::Dispatched,mechanism,message:"OS accepted the operation; re-observe to verify the application result".into()},
                        Err(e)=>ActionOutcome{action_id:action_id.clone(),helper_epoch:epoch.clone(),state:ActionState::Unknown,mechanism:"native".into(),message:format!("{}; do not replay this action automatically",e)},
                    })
                })();
                let outcome = outcome.unwrap_or_else(|e| ActionOutcome {
                    action_id: action_id.clone(),
                    helper_epoch: epoch,
                    state: ActionState::Rejected,
                    mechanism: "none".into(),
                    message: e.to_string(),
                });
                if let Some(session) = self.session.as_mut() {
                    session
                        .actions
                        .insert(action_id, (fingerprint, outcome.clone()));
                }
                Ok(json!(outcome))
            }
            Command::ActionStatus {
                session_id,
                action_id,
            } => {
                // The journal is available only while this session is retained, never after stop/EOF/expiry.
                let session = self
                    .session
                    .as_ref()
                    .filter(|s| s.id == session_id)
                    .ok_or_else(|| Error::new("invalid_session", "No matching session"))?;
                session
                    .actions
                    .get(&action_id)
                    .map(|(_, o)| json!(o))
                    .ok_or_else(|| {
                        Error::new(
                            "unknown_action",
                            "Action ID was not recorded in this helper epoch",
                        )
                    })
            }
            Command::Stop { session_id } => {
                if let Some(session) = self.session.as_ref() {
                    if session.id != session_id {
                        return Err(Error::new("invalid_session", "Stop session mismatch"));
                    }
                }
                self.cancel.store(true, Ordering::SeqCst);
                // The input worker is this thread: by reaching here all RAII input guards are dropped.
                if let Some(session) = self.session.as_mut() {
                    session.observation = None;
                }
                let session = self.session.take();
                drop(session);
                Ok(json!({"stopped":true,"helper_epoch":self.epoch}))
            }
        }
    }
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
        Action::SetValue { element_id, .. } => out.push(element_id.as_str()),
        _ => {}
    }
    out
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--schema") {
        println!(
            "{}",
            serde_json::to_string_pretty(&schemars::schema_for!(ProtocolTypes)).unwrap()
        );
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--probe") {
        return probe_main();
    }
    if args.first().is_some_and(|a| a == "--validate-protocol") {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        while let Some(frame) =
            read_frame(&mut input).map_err(|e| Error::new("invalid_frame", e.to_string()))?
        {
            let result = serde_json::from_slice::<Request>(&frame).map_err(|e| e.to_string());
            println!("{}", json!({"valid":result.is_ok()}));
        }
        return Ok(());
    }
    let policy = match args.as_slice() {
        [] => Policy::default(),
        [flag, path] if flag == "--policy" => {
            let metadata =
                std::fs::metadata(path).map_err(|e| Error::new("policy_error", e.to_string()))?;
            if metadata.len() > 65536 {
                return Err(Error::new("policy_error", "Policy exceeds 64 KiB"));
            }
            serde_json::from_slice(
                &std::fs::read(path).map_err(|e| Error::new("policy_error", e.to_string()))?,
            )
            .map_err(|e| Error::new("policy_error", e.to_string()))?
        }
        _ => {
            return Err(Error::new(
                "invalid_params",
                "Usage: cu-helper [--policy absolute-policy.json]",
            ))
        }
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst))
        .map_err(|e| Error::new("signal_error", e.to_string()))?;
    let (tx, rx) = mpsc::sync_channel::<Request>(32);
    let reader_cancel = cancel.clone();
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        loop {
            match read_frame(&mut reader) {
                Ok(Some(frame)) => match serde_json::from_slice::<Request>(&frame) {
                    Ok(request) => {
                        if request.id.is_empty()
                            || request.id.len() > 128
                            || request.version != VERSION
                        {
                            output(&Response {
                                version: VERSION,
                                id: request.id,
                                result: None,
                                error: Some(Error::new(
                                    "protocol_mismatch",
                                    "Invalid request ID or protocol version",
                                )),
                            });
                            continue;
                        }
                        if matches!(request.command, Command::Stop { .. }) {
                            reader_cancel.store(true, Ordering::SeqCst);
                        }
                        if let Err(mpsc::TrySendError::Full(request)) = tx.try_send(request) {
                            reader_cancel.store(true, Ordering::SeqCst);
                            output(&Response {
                                version: VERSION,
                                id: request.id,
                                result: None,
                                error: Some(Error::new(
                                    "queue_limit",
                                    "Request queue full; session stopped",
                                )),
                            });
                            break;
                        }
                    }
                    Err(_) => output(&Response {
                        version: VERSION,
                        id: String::new(),
                        result: None,
                        error: Some(Error::new("invalid_params", "Invalid protocol request")),
                    }),
                },
                Ok(None) => {
                    reader_cancel.store(true, Ordering::SeqCst);
                    break;
                }
                Err(e) => {
                    reader_cancel.store(true, Ordering::SeqCst);
                    output(&Response {
                        version: VERSION,
                        id: String::new(),
                        result: None,
                        error: Some(Error::new("invalid_frame", e.to_string())),
                    });
                    break;
                }
            }
        }
    });
    let mut runtime = Runtime {
        policy,
        epoch: new_id(),
        session: None,
        started: false,
        cancel: cancel.clone(),
    };
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(request) => {
                let result = runtime.handle(request.command);
                let response = match result {
                    Ok(value) => Response {
                        version: VERSION,
                        id: request.id,
                        result: Some(value),
                        error: None,
                    },
                    Err(error) => Response {
                        version: VERSION,
                        id: request.id,
                        result: None,
                        error: Some(error),
                    },
                };
                output(&response);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if runtime
            .session
            .as_ref()
            .is_some_and(|s| now_ms() > s.expires)
        {
            cancel.store(true, Ordering::SeqCst);
        }
        // Idle stop/EOF/expiry releases the lease without waiting for the next request.
        if cancel.load(Ordering::SeqCst) {
            if let Some(s) = runtime.session.as_mut() {
                s.observation = None;
            }
            runtime.session = None;
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expiry_cancels_without_returning_to_dispatch_loop() {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker = expiry_watchdog(cancel.clone(), Duration::from_millis(30));
        worker.join().unwrap();
        assert!(cancel.load(Ordering::SeqCst));
    }
    #[test]
    fn expiry_worker_exits_on_explicit_stop() {
        let cancel = Arc::new(AtomicBool::new(true));
        expiry_watchdog(cancel, Duration::from_secs(3600))
            .join()
            .unwrap();
    }
    #[test]
    fn bounded_frames_reject_partial_eof_and_preserve_multiple_lines() {
        assert!(read_frame(&mut io::Cursor::new(b"partial")).is_err());
        let mut input = io::Cursor::new(b"one\ntwo\n");
        assert_eq!(read_frame(&mut input).unwrap().unwrap(), b"one\n");
        assert_eq!(read_frame(&mut input).unwrap().unwrap(), b"two\n");
        assert!(read_frame(&mut input).unwrap().is_none());
    }
}
