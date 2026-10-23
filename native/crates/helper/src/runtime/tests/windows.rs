use super::*;

#[test]
fn sharing_interruption_preserves_cause_and_blocks_input_before_expiry() {
    let (mut runtime, id) = setup(policy());
    let observed = observation(&mut runtime, &id);
    runtime.interrupt(Error::new(
        "capture_stream_stopped",
        "SCStreamErrorDomain code -3805",
    ));
    runtime.release_if_cancelled();
    for command in [
        Command::Start {},
        Command::ListWindows {
            session_id: id.clone(),
        },
    ] {
        let error = runtime.handle(command, MAX_VERSION).unwrap_err();
        assert_eq!(error.code, "capture_stream_stopped");
        assert!(error.message.contains("-3805"));
        assert!(!error.may_have_executed);
    }
    let error = runtime
        .handle(
            Command::Act {
                session_id: id.clone(),
                action_id: "old-action".into(),
                observation_id: observed,
                action: Action::Key {
                    keys: vec!["Tab".into()],
                },
            },
            MAX_VERSION,
        )
        .unwrap_err();
    assert_eq!(error.code, "capture_stream_stopped");
    assert_eq!(runtime.backend.state.borrow().inputs, 0);
    assert!(runtime
        .handle(Command::Stop { session_id: id }, MAX_VERSION)
        .is_ok());
}

#[test]
fn stop_acknowledges_presentation_cleanup_or_reports_failure() {
    for fail in [false, true] {
        let (mut runtime, id) = setup(policy());
        if fail {
            runtime.backend.state.borrow_mut().cleanup_error =
                Some(Error::new("cleanup_unconfirmed", "Stream did not stop"));
        }
        let result = runtime.handle(Command::Stop { session_id: id }, MAX_VERSION);
        assert_eq!(runtime.backend.state.borrow().cleanups, 1);
        assert!(runtime.session.is_none());
        if fail {
            assert_eq!(result.unwrap_err().code, "cleanup_unconfirmed");
        } else {
            assert_eq!(result.unwrap()["stopped"], true);
        }
    }
}

fn changed_windows() -> Vec<Vec<Window>> {
    let mut moved = window();
    moved.bounds.x += 10.0;
    let mut recycled = window();
    recycled.pid = 2;
    recycled.process_identity = "process:2".into();
    let mut restarted = window();
    restarted.process_identity = "reused-pid:1".into();
    vec![vec![], vec![moved], vec![recycled], vec![restarted]]
}

#[test]
fn get_window_rejects_closed_moved_and_reused_live_windows() {
    for windows in changed_windows() {
        let (mut runtime, id) = setup(policy());
        list(&mut runtime, &id);
        runtime.backend.state.borrow_mut().windows = windows;
        let error = runtime
            .handle(
                Command::GetWindow {
                    session_id: id,
                    window: window(),
                },
                MAX_VERSION,
            )
            .unwrap_err();
        assert_eq!(error.code, "stale_window");
        assert!(!error.may_have_executed);
    }
}

#[test]
fn activation_rejects_changed_windows_before_dispatch() {
    for windows in changed_windows() {
        let (mut runtime, id) = setup(policy());
        list(&mut runtime, &id);
        runtime.backend.state.borrow_mut().windows = windows;
        let error = runtime
            .handle(
                Command::ActivateWindow {
                    session_id: id,
                    window: window(),
                },
                MAX_VERSION,
            )
            .unwrap_err();
        assert_eq!(error.code, "stale_window");
        assert!(!error.may_have_executed);
        assert_eq!(runtime.backend.state.borrow().activations, 0);
    }
}

#[test]
fn activation_cancellation_after_dispatch_is_uncertain() {
    let (mut runtime, id) = setup(policy());
    list(&mut runtime, &id);
    runtime.backend.state.borrow_mut().activation_cancel = true;
    let error = runtime
        .handle(
            Command::ActivateWindow {
                session_id: id,
                window: window(),
            },
            MAX_VERSION,
        )
        .unwrap_err();
    assert!(error.may_have_executed);
    assert_eq!(runtime.backend.state.borrow().activations, 1);
}

#[test]
fn activation_geometry_change_after_dispatch_is_uncertain() {
    let (mut runtime, id) = setup(policy());
    list(&mut runtime, &id);
    let mut moved = window();
    moved.bounds.x += 10.0;
    runtime.backend.state.borrow_mut().activation_window = Some(moved);
    let error = runtime
        .handle(
            Command::ActivateWindow {
                session_id: id,
                window: window(),
            },
            MAX_VERSION,
        )
        .unwrap_err();
    assert_eq!(error.code, "stale_window");
    assert!(error.may_have_executed);
}

#[test]
fn activation_preserves_definite_pre_dispatch_backend_rejection() {
    let (mut runtime, id) = setup(policy());
    list(&mut runtime, &id);
    runtime.backend.state.borrow_mut().activation_error =
        Some(Error::new("unsupported_action", "Not supported"));
    let error = runtime
        .handle(
            Command::ActivateWindow {
                session_id: id,
                window: window(),
            },
            MAX_VERSION,
        )
        .unwrap_err();
    assert!(!error.may_have_executed);
}

#[test]
fn activation_worker_rechecks_live_identity_before_native_activation() {
    let (runtime, _) = setup(policy());
    runtime.backend.state.borrow_mut().windows.clear();
    let result = crate::worker::execute(
        crate::worker::Probe::Activate { window: window() },
        &runtime.backend,
        &runtime.cancel,
    );
    let error = result.unwrap_err();
    assert_eq!(error.code, "stale_window");
    assert!(!error.may_have_executed);
    assert_eq!(runtime.backend.state.borrow().activations, 0);
}

#[test]
fn uncertain_activation_consumes_the_previous_observation() {
    let (mut runtime, id) = setup(policy());
    let observed = observation(&mut runtime, &id);
    runtime.backend.state.borrow_mut().activation_error =
        Some(Error::new("focus_failed", "Partial activation").after_dispatch());
    let error = runtime
        .handle(
            Command::ActivateWindow {
                session_id: id.clone(),
                window: window(),
            },
            MAX_VERSION,
        )
        .unwrap_err();
    assert!(error.may_have_executed);
    assert_eq!(
        act(&mut runtime, &id, &observed, "old")["state"],
        "rejected"
    );
    assert_eq!(runtime.backend.state.borrow().inputs, 0);
}
