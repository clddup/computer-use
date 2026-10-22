use crate::worker::Probe;
use cu_core::cancelled;
use cu_protocol::*;
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) fn query(probe: Probe, cancel: &AtomicBool) -> Result<Value> {
    cancelled(cancel)?;
    let payload =
        serde_json::to_vec(&probe).map_err(|e| Error::new("worker_failed", e.to_string()))?;
    let may_execute = probe.may_execute();
    let mut child = spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(&payload).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(uncertain(
                Error::new("worker_failed", "Worker input failed"),
                may_execute,
            ));
        }
    }
    let stdout = child.stdout.take().expect("piped worker stdout");
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take((MAX_FRAME + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let result = wait(&mut child, rx, cancel, may_execute);
    let _ = child.wait();
    let _ = reader.join();
    result
}

fn spawn() -> Result<Child> {
    let mut command = Command::new(
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
    command
        .spawn()
        .map_err(|e| Error::new("worker_failed", e.to_string()))
}

fn wait(
    child: &mut Child,
    rx: mpsc::Receiver<io::Result<Vec<u8>>>,
    cancel: &AtomicBool,
    may_execute: bool,
) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if cancel.load(Ordering::SeqCst) || Instant::now() > deadline {
            let _ = child.kill();
            let code = if cancel.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "query_timeout"
            };
            return Err(uncertain(
                Error::new(code, "Accessibility/capture worker was terminated"),
                may_execute,
            ));
        }
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(Ok(bytes)) => return decode(&bytes, may_execute),
            Ok(Err(error)) => {
                return Err(uncertain(
                    Error::new("worker_failed", error.to_string()),
                    may_execute,
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(uncertain(
                    Error::new("worker_failed", "Worker output closed"),
                    may_execute,
                ))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn decode(bytes: &[u8], may_execute: bool) -> Result<Value> {
    if bytes.len() > MAX_FRAME {
        return Err(uncertain(
            Error::new("frame_limit", "Worker response exceeds limit"),
            may_execute,
        ));
    }
    serde_json::from_slice::<Result<Value>>(bytes)
        .map_err(|error| uncertain(Error::new("worker_failed", error.to_string()), may_execute))?
}

fn uncertain(error: Error, may_execute: bool) -> Error {
    if may_execute {
        error.after_dispatch()
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lost_worker_response_is_uncertain_only_for_side_effecting_probes() {
        for may_execute in [false, true] {
            let error = decode(b"truncated response", may_execute).unwrap_err();
            assert_eq!(error.may_have_executed, may_execute);
        }
    }

    #[test]
    fn complete_worker_error_preserves_the_dispatch_boundary() {
        for dispatched in [false, true] {
            let mut error = Error::new("fixture", "Known failure stage");
            error.may_have_executed = dispatched;
            let bytes = serde_json::to_vec(&Result::<Value>::Err(error)).unwrap();
            assert_eq!(
                decode(&bytes, true).unwrap_err().may_have_executed,
                dispatched
            );
        }
    }

    #[test]
    fn probe_classification_does_not_treat_semantic_preflight_as_dispatch() {
        let window = Window {
            id: "fixture".into(),
            pid: 1,
            process_identity: "process".into(),
            title: "Fixture".into(),
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        };
        let action = Box::new(cu_core::NativeAction::Key {
            keys: vec!["Tab".into()],
        });
        assert!(Probe::Activate {
            window: window.clone()
        }
        .may_execute());
        assert!(!Probe::Semantic {
            window: window.clone(),
            action: action.clone(),
            execute: false
        }
        .may_execute());
        assert!(Probe::Semantic {
            window,
            action,
            execute: true
        }
        .may_execute());
        assert!(!Probe::List.may_execute());
    }
}
