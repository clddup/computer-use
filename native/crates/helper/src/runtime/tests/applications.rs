use super::*;

fn discover(runtime: &TestRuntime) {
    let mut state = runtime.backend.state.borrow_mut();
    state.apps = vec![Application {
        id: "fixture".into(),
        display_name: "Fixture".into(),
        is_running: true,
        windows: vec![window()],
    }];
}

#[test]
fn pending_launch_binds_late_windows_without_launching_again() {
    let (mut runtime, id) = setup(app_policy());
    let result = runtime
        .handle(
            Command::LaunchApp {
                session_id: id.clone(),
                app_id: "fixture".into(),
            },
            MAX_VERSION,
        )
        .unwrap();
    assert_eq!(result["state"], "pending");
    discover(&runtime);
    runtime
        .handle(
            Command::ListApps {
                session_id: id.clone(),
            },
            MAX_VERSION,
        )
        .unwrap();
    let result = runtime
        .handle(
            Command::ListWindows {
                session_id: id.clone(),
            },
            MAX_VERSION,
        )
        .unwrap();
    assert_eq!(result.as_array().unwrap().len(), 1);
    runtime
        .handle(
            Command::GetWindow {
                session_id: id.clone(),
                window: window(),
            },
            MAX_VERSION,
        )
        .unwrap();
    runtime
        .handle(
            Command::Observe {
                session_id: id,
                window: window(),
                screenshot: false,
                accessibility: false,
            },
            MAX_VERSION,
        )
        .unwrap();
    assert_eq!(runtime.backend.state.borrow().launches, 1);
}

#[test]
fn pending_launch_can_be_reconciled_by_listing_windows_directly() {
    let (mut runtime, id) = setup(app_policy());
    runtime
        .handle(
            Command::LaunchApp {
                session_id: id.clone(),
                app_id: "fixture".into(),
            },
            MAX_VERSION,
        )
        .unwrap();
    list(&mut runtime, &id);
    discover(&runtime);
    let result = runtime
        .handle(Command::ListWindows { session_id: id }, MAX_VERSION)
        .unwrap();
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(runtime.backend.state.borrow().launches, 1);
}

#[test]
fn discovery_does_not_bind_apps_without_a_launch_request() {
    let (mut runtime, id) = setup(app_policy());
    discover(&runtime);
    runtime
        .handle(
            Command::ListApps {
                session_id: id.clone(),
            },
            MAX_VERSION,
        )
        .unwrap();
    let result = runtime
        .handle(Command::ListWindows { session_id: id }, MAX_VERSION)
        .unwrap();
    assert!(result.as_array().unwrap().is_empty());
}

#[test]
fn launch_cancellation_after_backend_success_is_uncertain() {
    let (mut runtime, id) = setup(app_policy());
    runtime.backend.state.borrow_mut().launch_cancel = true;
    let error = runtime
        .handle(
            Command::LaunchApp {
                session_id: id,
                app_id: "fixture".into(),
            },
            MAX_VERSION,
        )
        .unwrap_err();
    assert!(error.may_have_executed);
}

#[test]
fn failed_launch_preserves_backend_execution_marker() {
    for uncertain in [false, true] {
        let (mut runtime, id) = setup(app_policy());
        let mut error = Error::new("fixture_launch_failure", "Controlled failure");
        error.may_have_executed = uncertain;
        runtime.backend.state.borrow_mut().launch_error = Some(error);
        let error = runtime
            .handle(
                Command::LaunchApp {
                    session_id: id,
                    app_id: "fixture".into(),
                },
                MAX_VERSION,
            )
            .unwrap_err();
        assert_eq!(error.may_have_executed, uncertain);
    }
}

#[test]
fn uncertain_launch_failure_can_be_reconciled_without_relaunch() {
    let (mut runtime, id) = setup(app_policy());
    runtime.backend.state.borrow_mut().launch_error =
        Some(Error::new("worker_failed", "Response lost").after_dispatch());
    assert!(
        runtime
            .handle(
                Command::LaunchApp {
                    session_id: id.clone(),
                    app_id: "fixture".into()
                },
                MAX_VERSION
            )
            .unwrap_err()
            .may_have_executed
    );
    discover(&runtime);
    let result = runtime
        .handle(Command::ListWindows { session_id: id }, MAX_VERSION)
        .unwrap();
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(runtime.backend.state.borrow().launches, 1);
}

#[test]
fn definite_launch_rejection_does_not_authorize_discovered_processes() {
    let (mut runtime, id) = setup(app_policy());
    runtime.backend.state.borrow_mut().launch_error =
        Some(Error::new("permission_denied", "Before launch"));
    assert!(runtime
        .handle(
            Command::LaunchApp {
                session_id: id.clone(),
                app_id: "fixture".into()
            },
            MAX_VERSION
        )
        .is_err());
    discover(&runtime);
    let result = runtime
        .handle(Command::ListWindows { session_id: id }, MAX_VERSION)
        .unwrap();
    assert!(result.as_array().unwrap().is_empty());
}

#[test]
fn failed_retry_does_not_forget_an_earlier_pending_launch() {
    let (mut runtime, id) = setup(app_policy());
    runtime
        .handle(
            Command::LaunchApp {
                session_id: id.clone(),
                app_id: "fixture".into(),
            },
            MAX_VERSION,
        )
        .unwrap();
    runtime.backend.state.borrow_mut().launch_error = Some(Error::new("fixture", "Retry rejected"));
    assert!(runtime
        .handle(
            Command::LaunchApp {
                session_id: id.clone(),
                app_id: "fixture".into()
            },
            MAX_VERSION
        )
        .is_err());
    discover(&runtime);
    let result = runtime
        .handle(Command::ListWindows { session_id: id }, MAX_VERSION)
        .unwrap();
    assert_eq!(result.as_array().unwrap().len(), 1);
}

#[test]
fn bound_application_pid_reuse_does_not_authorize_a_new_process() {
    let (mut runtime, id) = setup(app_policy());
    discover(&runtime);
    runtime.backend.state.borrow_mut().launch_windows = vec![window()];
    runtime
        .handle(
            Command::LaunchApp {
                session_id: id.clone(),
                app_id: "fixture".into(),
            },
            MAX_VERSION,
        )
        .unwrap();
    runtime.backend.state.borrow_mut().windows[0].process_identity = "different-start".into();
    let result = runtime
        .handle(Command::ListWindows { session_id: id }, MAX_VERSION)
        .unwrap();
    assert!(result.as_array().unwrap().is_empty());
}
