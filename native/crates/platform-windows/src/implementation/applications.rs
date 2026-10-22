use super::*;

pub(super) fn list_apps(apps: &[ApplicationPolicy]) -> cu_protocol::Result<Vec<Application>> {
    let windows = WindowsBackend.list_windows()?;
    Ok(apps
        .iter()
        .map(|app| {
            let matching = matching_windows(&windows, &app.executable);
            Application {
                id: app.id.clone(),
                display_name: app.display_name.clone(),
                is_running: !matching.is_empty(),
                windows: matching,
            }
        })
        .collect())
}

pub(super) fn launch_app(
    app: &ApplicationPolicy,
    cancel: &AtomicBool,
) -> cu_protocol::Result<Vec<Window>> {
    cancelled(cancel)?;
    if !std::path::Path::new(&app.executable).is_absolute() {
        return Err(cu_protocol::Error::new(
            "permission_denied",
            "Application executable must be an absolute host-approved path",
        ));
    }
    let existing = matching_windows(&WindowsBackend.list_windows()?, &app.executable);
    if !existing.is_empty() {
        return Ok(existing);
    }
    let mut command = ProcessCommand::new(&app.executable);
    command
        .args(&app.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cancelled(cancel)?;
    command.spawn().map_err(err)?;
    wait_for_windows(app, cancel).map_err(cu_protocol::Error::after_dispatch)
}

fn wait_for_windows(
    app: &ApplicationPolicy,
    cancel: &AtomicBool,
) -> cu_protocol::Result<Vec<Window>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cancelled(cancel)?;
        let windows = matching_windows(&WindowsBackend.list_windows()?, &app.executable);
        if !windows.is_empty() {
            return Ok(windows);
        }
        if Instant::now() >= deadline {
            return Ok(vec![]);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn matching_windows(windows: &[Window], executable: &str) -> Vec<Window> {
    let expected = executable.to_ascii_lowercase();
    windows
        .iter()
        .filter(|window| unsafe {
            executable_path(window.pid).ok().as_deref() == Some(expected.as_str())
        })
        .cloned()
        .collect()
}
