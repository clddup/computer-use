use super::*;

#[test]
fn identical_action_is_dispatched_once_and_conflicting_id_is_rejected() {
    let (mut runtime, id) = setup(policy());
    let observed = observation(&mut runtime, &id);
    let first = act(&mut runtime, &id, &observed, "action");
    assert_eq!(first["state"], "dispatched");
    assert_eq!(act(&mut runtime, &id, &observed, "action"), first);
    let error = runtime
        .handle(
            Command::Act {
                session_id: id,
                action_id: "action".into(),
                observation_id: observed,
                action: Action::TypeText {
                    text: "different".into(),
                },
            },
            MAX_VERSION,
        )
        .unwrap_err();
    assert_eq!(error.code, "action_id_conflict");
    assert_eq!(runtime.backend.state.borrow().inputs, 1);
}

#[test]
fn unknown_dispatch_is_journaled_and_never_replayed() {
    let (mut runtime, id) = setup(policy());
    let observed = observation(&mut runtime, &id);
    runtime.backend.state.borrow_mut().perform_error =
        Some(Error::new("cancelled", "Partial input"));
    let first = act(&mut runtime, &id, &observed, "action");
    assert_eq!(first["state"], "unknown");
    assert_eq!(act(&mut runtime, &id, &observed, "action"), first);
    assert_eq!(runtime.backend.state.borrow().inputs, 1);
}

#[test]
#[cfg(target_os = "macos")]
fn unsupported_key_preflight_is_rejected_without_dispatch() {
    let (mut runtime, id) = setup(policy());
    let observed = observation(&mut runtime, &id);
    runtime.backend.state.borrow_mut().preflight_error =
        Some(Error::new("unsupported_key", "Unsupported key not-a-key"));
    let rejected = runtime
        .handle(
            Command::Act {
                session_id: id.clone(),
                action_id: "bad-key".into(),
                observation_id: observed,
                action: Action::Key {
                    keys: vec!["not-a-key".into()],
                },
            },
            MAX_VERSION,
        )
        .unwrap();
    assert_eq!(rejected["state"], "rejected");
    assert_eq!(runtime.backend.state.borrow().inputs, 0);
    assert!(runtime.session.as_ref().unwrap().observation.is_some());
    assert_eq!(runtime.action_status(&id, "bad-key").unwrap(), rejected);
}

#[test]
fn consumed_and_superseded_observations_do_not_dispatch_input() {
    let (mut runtime, id) = setup(policy());
    let old = observation(&mut runtime, &id);
    let current = observation(&mut runtime, &id);
    assert_eq!(act(&mut runtime, &id, &old, "stale")["state"], "rejected");
    assert_eq!(
        act(&mut runtime, &id, &current, "current")["state"],
        "dispatched"
    );
    assert_eq!(
        act(&mut runtime, &id, &current, "consumed")["state"],
        "rejected"
    );
    assert_eq!(runtime.backend.state.borrow().inputs, 1);
}

#[test]
fn stop_releases_session_and_prevents_restart() {
    let (mut runtime, id) = setup(policy());
    runtime
        .handle(Command::Stop { session_id: id }, MAX_VERSION)
        .unwrap();
    assert!(runtime.session.is_none());
    assert_eq!(
        runtime
            .handle(Command::Start {}, MAX_VERSION)
            .unwrap_err()
            .code,
        "cancelled"
    );
}

#[test]
fn expired_observation_and_denied_input_never_dispatch() {
    for expired in [false, true] {
        let (mut runtime, id) = setup(policy());
        let observed = observation(&mut runtime, &id);
        if expired {
            runtime
                .session
                .as_mut()
                .unwrap()
                .observation
                .as_mut()
                .unwrap()
                .expires_at_ms = 0;
        } else {
            runtime.policy.allow_input = false;
        }
        assert_eq!(
            act(&mut runtime, &id, &observed, "denied")["state"],
            "rejected"
        );
        assert_eq!(runtime.backend.state.borrow().inputs, 0);
    }
}

#[test]
fn activation_worker_marks_cancellation_after_native_acceptance() {
    let (runtime, _) = setup(policy());
    runtime.backend.state.borrow_mut().activation_cancel = true;
    let error = crate::worker::execute(
        crate::worker::Probe::Activate { window: window() },
        &runtime.backend,
        &runtime.cancel,
    )
    .unwrap_err();
    assert!(error.may_have_executed);
}

#[test]
fn stop_and_eof_cancellation_can_drop_session_before_queued_stop_reply() {
    let (mut runtime, id) = setup(policy());
    runtime.cancel.store(true, Ordering::SeqCst);
    runtime.release_if_cancelled();
    assert!(runtime.session.is_none());
    let result = runtime
        .handle(Command::Stop { session_id: id }, MAX_VERSION)
        .unwrap();
    assert_eq!(result["stopped"], true);
}
