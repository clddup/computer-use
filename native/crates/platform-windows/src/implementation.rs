use cu_core::{cancelled, new_id, now_ms, png_base64, ApplicationPolicy, Backend, NativeAction};
use cu_protocol::*;
use std::{
    process::{Command as ProcessCommand, Stdio},
    sync::{atomic::AtomicBool, mpsc},
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Dwm::*,
        System::{Com::*, Threading::*},
        UI::{Accessibility::*, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    settings::*,
};

pub struct WindowsBackend;
fn err(e: impl std::fmt::Display) -> cu_protocol::Error {
    cu_protocol::Error::new("os_error", e.to_string())
}
fn hwnd(w: &Window) -> cu_protocol::Result<HWND> {
    w.id.parse::<usize>()
        .map(|v| HWND(v as *mut _))
        .map_err(|_| cu_protocol::Error::new("invalid_window", "Invalid window identifier"))
}
fn setup() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}
unsafe fn identity(pid: u32) -> cu_protocol::Result<String> {
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(err)?;
    let mut created = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let result = GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user);
    let _ = CloseHandle(process);
    result.map_err(err)?;
    Ok(format!(
        "{pid}:{}:{}",
        created.dwHighDateTime, created.dwLowDateTime
    ))
}
unsafe fn executable_path(pid: u32) -> cu_protocol::Result<String> {
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(err)?;
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    let result = QueryFullProcessImageNameW(
        process,
        PROCESS_NAME_WIN32,
        PWSTR(buffer.as_mut_ptr()),
        &mut length,
    );
    let _ = CloseHandle(process);
    result.map_err(err)?;
    Ok(String::from_utf16_lossy(&buffer[..length as usize]).to_ascii_lowercase())
}

unsafe fn window_info(handle: HWND) -> cu_protocol::Result<Window> {
    if !IsWindow(Some(handle)).as_bool()
        || !IsWindowVisible(handle).as_bool()
        || IsIconic(handle).as_bool()
    {
        return Err(cu_protocol::Error::new(
            "invalid_window",
            "Window is hidden, minimized, or closed",
        ));
    }
    let mut pid = 0;
    GetWindowThreadProcessId(handle, Some(&mut pid));
    let mut rect = RECT::default();
    DwmGetWindowAttribute(
        handle,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut rect as *mut _ as *mut _,
        std::mem::size_of::<RECT>() as u32,
    )
    .map_err(err)?;
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return Err(cu_protocol::Error::new(
            "invalid_window",
            "Window has no visible bounds",
        ));
    }
    let mut title = vec![0u16; (GetWindowTextLengthW(handle).max(0) as usize + 1).min(4096)];
    let count = GetWindowTextW(handle, &mut title);
    Ok(Window {
        id: (handle.0 as usize).to_string(),
        pid,
        process_identity: identity(pid)?,
        title: String::from_utf16_lossy(&title[..count as usize]),
        bounds: Rect {
            x: rect.left as f64,
            y: rect.top as f64,
            width: (rect.right - rect.left) as f64,
            height: (rect.bottom - rect.top) as f64,
        },
    })
}
unsafe extern "system" fn enumerate(handle: HWND, param: LPARAM) -> BOOL {
    if let Ok(window) = window_info(handle) {
        if !window.title.is_empty() {
            (*(param.0 as *mut Vec<Window>)).push(window);
        }
    }
    true.into()
}

struct ComGuard;
impl ComGuard {
    unsafe fn new() -> cu_protocol::Result<Self> {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(err)?;
        Ok(Self)
    }
}
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
unsafe fn accessibility(window: &Window) -> cu_protocol::Result<Vec<Element>> {
    let _guard = ComGuard::new()?;
    let automation: IUIAutomation =
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(err)?;
    let root = automation.ElementFromHandle(hwnd(window)?).map_err(err)?;
    let walker = automation.ControlViewWalker().map_err(err)?;
    let mut stack = vec![(root, 0u32, String::from("root"))];
    let mut elements = vec![];
    let start = Instant::now();
    while let Some((node, depth, locator)) = stack.pop() {
        if elements.len() >= 512 || start.elapsed() > Duration::from_secs(3) {
            break;
        }
        if node.CurrentProcessId().unwrap_or(-1) != window.pid as i32 {
            continue;
        }
        let password = node
            .CurrentIsPassword()
            .map(|b| b.as_bool())
            .unwrap_or(true);
        let rect = node
            .CurrentBoundingRectangle()
            .ok()
            .filter(|r| r.right > r.left && r.bottom > r.top)
            .map(|r| Rect {
                x: r.left as f64,
                y: r.top as f64,
                width: (r.right - r.left) as f64,
                height: (r.bottom - r.top) as f64,
            });
        let pattern = if password {
            None
        } else {
            node.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .ok()
        };
        elements.push(Element {
            id: new_id(),
            role: node
                .CurrentLocalizedControlType()
                .map(|v| v.to_string())
                .unwrap_or_default(),
            name: if password {
                String::from("[password]")
            } else {
                node.CurrentName()
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            },
            value: pattern
                .as_ref()
                .and_then(|p| p.CurrentValue().ok())
                .map(|v| v.to_string().chars().take(4096).collect()),
            bounds: rect,
            editable: pattern
                .as_ref()
                .is_some_and(|p| p.CurrentIsReadOnly().is_ok_and(|b| !b.as_bool())),
            password,
            depth,
            locator: locator.clone(),
        });
        if password || depth >= 16 {
            continue;
        }
        let mut child = walker.GetFirstChildElement(&node).ok();
        let mut children = Vec::new();
        while let Some(current) = child {
            if children.len() >= 128 {
                break;
            }
            child = walker.GetNextSiblingElement(&current).ok();
            children.push((current, depth + 1, format!("{locator}/{}", children.len())));
        }
        stack.extend(children.into_iter().rev());
    }
    Ok(elements)
}

struct SingleFrame {
    sender: mpsc::Sender<(u32, u32, Vec<u8>)>,
}
impl GraphicsCaptureApiHandler for SingleFrame {
    type Flags = mpsc::Sender<(u32, u32, Vec<u8>)>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn new(ctx: Context<Self::Flags>) -> std::result::Result<Self, Self::Error> {
        Ok(Self { sender: ctx.flags })
    }
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> std::result::Result<(), Self::Error> {
        let width = frame.width();
        let height = frame.height();
        if width as u64 * height as u64 > 16_000_000 {
            control.stop();
            return Err("Capture exceeds 16 megapixels".into());
        }
        let mut buffer = frame.buffer()?;
        let data = buffer.as_nopadding_buffer()?.to_vec();
        let _ = self.sender.send((width, height, data));
        control.stop();
        Ok(())
    }
}
fn capture(window: &Window) -> cu_protocol::Result<Image> {
    let (tx, rx) = mpsc::channel();
    let item = windows_capture::window::Window::from_raw_hwnd(hwnd(window)?.0);
    let settings = Settings::new(
        item,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::WithBorder,
        SecondaryWindowSettings::Exclude,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        tx,
    );
    SingleFrame::start(settings).map_err(err)?;
    let (width, height, data) = rx.recv_timeout(Duration::from_secs(1)).map_err(err)?;
    if (width as f64 - window.bounds.width).abs() > 2.0
        || (height as f64 - window.bounds.height).abs() > 2.0
    {
        return Err(cu_protocol::Error::new(
            "capture_mapping_changed",
            "WGC frame dimensions do not match the current window bounds",
        ));
    }
    Ok(Image {
        id: new_id(),
        mime_type: "image/png".into(),
        data: png_base64(width, height, &data)?,
        width,
        height,
        screen_bounds: window.bounds.clone(),
        capture_method: "windows-graphics-capture".into(),
    })
}

struct HeldInputs {
    keys: Vec<(u16, bool)>,
    mouse: Option<MOUSE_EVENT_FLAGS>,
}
impl HeldInputs {
    fn new() -> Self {
        Self {
            keys: vec![],
            mouse: None,
        }
    }
    unsafe fn key(&mut self, code: u16, unicode: bool, down: bool) -> cu_protocol::Result<()> {
        // Track before dispatch: partial delivery must still be followed by a release.
        if down {
            self.keys.push((code, unicode));
        }
        send_key(code, unicode, down)?;
        if !down {
            if let Some(i) = self.keys.iter().position(|v| *v == (code, unicode)) {
                self.keys.remove(i);
            }
        }
        Ok(())
    }
    unsafe fn mouse(
        &mut self,
        down: MOUSE_EVENT_FLAGS,
        up: MOUSE_EVENT_FLAGS,
    ) -> cu_protocol::Result<()> {
        self.mouse = Some(up);
        send_mouse(down, 0)?;
        Ok(())
    }
    unsafe fn release_mouse(&mut self) -> cu_protocol::Result<()> {
        if let Some(up) = self.mouse {
            send_mouse(up, 0)?;
            self.mouse = None;
        }
        Ok(())
    }
}
impl Drop for HeldInputs {
    fn drop(&mut self) {
        unsafe {
            for (code, unicode) in self.keys.drain(..).rev() {
                let _ = send_key(code, unicode, false);
            }
            if let Some(up) = self.mouse.take() {
                let _ = send_mouse(up, 0);
            }
        }
    }
}
unsafe fn send(input: INPUT) -> cu_protocol::Result<()> {
    if SendInput(&[input], std::mem::size_of::<INPUT>() as i32) != 1 {
        return Err(cu_protocol::Error::new(
            "input_failed",
            "SendInput rejected an event (possibly UIPI)",
        ));
    }
    Ok(())
}
unsafe fn send_key(code: u16, unicode: bool, down: bool) -> cu_protocol::Result<()> {
    let mut flags = if unicode {
        KEYEVENTF_UNICODE
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    if !down {
        flags |= KEYEVENTF_KEYUP;
    }
    send(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(if unicode { 0 } else { code }),
                wScan: if unicode { code } else { 0 },
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    })
}
unsafe fn send_mouse(flags: MOUSE_EVENT_FLAGS, data: u32) -> cu_protocol::Result<()> {
    send(INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    })
}
fn virtual_key(key: &str) -> cu_protocol::Result<u16> {
    Ok(match key.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => 0x11,
        "alt" => 0x12,
        "shift" => 0x10,
        "meta" | "win" => 0x5b,
        "enter" | "return" => 0x0d,
        "tab" => 0x09,
        "escape" | "esc" => 0x1b,
        "backspace" => 0x08,
        "delete" => 0x2e,
        "space" => 0x20,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphanumeric() => {
            s.as_bytes()[0].to_ascii_uppercase() as u16
        }
        _ => {
            return Err(cu_protocol::Error::new(
                "unsupported_key",
                format!("Unsupported key: {key}"),
            ))
        }
    })
}
unsafe fn foreground(handle: HWND, cancel: &AtomicBool) -> cu_protocol::Result<()> {
    cancelled(cancel)?;
    if GetForegroundWindow() != handle {
        return Err(cu_protocol::Error::new(
            "focus_changed",
            "Target window is no longer foreground",
        ));
    }
    Ok(())
}
unsafe fn activate_foreground(handle: HWND, cancel: &AtomicBool) -> cu_protocol::Result<()> {
    cancelled(cancel)?;
    if GetForegroundWindow() == handle {
        return Ok(());
    }
    let current_thread = GetCurrentThreadId();
    let target_thread = GetWindowThreadProcessId(handle, None);
    let foreground_window = GetForegroundWindow();
    let foreground_thread = if foreground_window.0.is_null() {
        0
    } else {
        GetWindowThreadProcessId(foreground_window, None)
    };
    let attached_target = target_thread != 0
        && target_thread != current_thread
        && AttachThreadInput(current_thread, target_thread, true).as_bool();
    let attached_foreground = foreground_thread != 0
        && foreground_thread != current_thread
        && foreground_thread != target_thread
        && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
    let _ = ShowWindow(handle, SW_RESTORE);
    let _ = SetWindowPos(
        handle,
        Some(HWND_TOP),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
    );
    let _ = BringWindowToTop(handle);
    let _ = SetActiveWindow(handle);
    let _ = SetFocus(Some(handle));
    let _ = SetForegroundWindow(handle);
    SwitchToThisWindow(handle, true);
    let mut result = Ok(());
    for _ in 0..50 {
        cancelled(cancel)?;
        if GetForegroundWindow() == handle {
            result = Ok(());
            break;
        }
        result = Err(cu_protocol::Error::new(
            "activation_failed",
            "Windows refused to activate the window",
        ));
        thread::sleep(Duration::from_millis(10));
    }
    if attached_foreground {
        let _ = AttachThreadInput(current_thread, foreground_thread, false);
    }
    if attached_target {
        let _ = AttachThreadInput(current_thread, target_thread, false);
    }
    result
}
unsafe fn move_to(handle: HWND, x: f64, y: f64, cancel: &AtomicBool) -> cu_protocol::Result<()> {
    foreground(handle, cancel)?;
    let point = POINT {
        x: x.round() as i32,
        y: y.round() as i32,
    };
    let hit = WindowFromPoint(point);
    if hit != handle && GetAncestor(hit, GA_ROOT) != handle {
        return Err(cu_protocol::Error::new(
            "target_occluded",
            "Target point belongs to another window",
        ));
    }
    SetCursorPos(point.x, point.y).map_err(err)
}
impl Backend for WindowsBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities { protocol_version:VERSION,platform:"windows".into(),backend:"win32-uia-wgc-sendinput".into(),available:true,reason:None,
            actions:vec!["list_apps","launch_app","get_window","activate_window","click","type_text","key","scroll","drag"].into_iter().map(String::from).collect(),screenshot_methods:vec!["windows-graphics-capture".into()],
            limitations:vec!["Interactive non-elevated desktop only; minimized windows rejected".into(),"Input requires foreground and unoccluded target; no secure desktop".into(),"AX and image timestamps are not atomic; set_value is not supported on this backend".into()] }
    }
    fn list_windows(&self) -> cu_protocol::Result<Vec<Window>> {
        setup();
        let mut windows = vec![];
        unsafe {
            EnumWindows(Some(enumerate), LPARAM(&mut windows as *mut _ as isize)).map_err(err)?;
        }
        Ok(windows)
    }
    fn list_apps(&self, apps: &[ApplicationPolicy]) -> cu_protocol::Result<Vec<Application>> {
        let windows = self.list_windows()?;
        Ok(apps
            .iter()
            .map(|app| {
                let expected = app.executable.to_ascii_lowercase();
                let matching = windows
                    .iter()
                    .filter(|window| unsafe {
                        executable_path(window.pid).ok().as_deref() == Some(expected.as_str())
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                Application {
                    id: app.id.clone(),
                    display_name: app.display_name.clone(),
                    is_running: !matching.is_empty(),
                    windows: matching,
                }
            })
            .collect())
    }
    fn launch_app(
        &self,
        app: &ApplicationPolicy,
        cancel: &AtomicBool,
    ) -> cu_protocol::Result<Vec<Window>> {
        if !std::path::Path::new(&app.executable).is_absolute() {
            return Err(cu_protocol::Error::new(
                "permission_denied",
                "Application executable must be an absolute host-approved path",
            ));
        }
        let expected = app.executable.to_ascii_lowercase();
        let existing = self
            .list_windows()?
            .into_iter()
            .filter(|window| unsafe {
                executable_path(window.pid).ok().as_deref() == Some(expected.as_str())
            })
            .collect::<Vec<_>>();
        if !existing.is_empty() {
            return Ok(existing);
        }
        let mut command = ProcessCommand::new(&app.executable);
        command.args(&app.args);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command.spawn().map_err(err)?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            cancelled(cancel)?;
            let windows = self
                .list_windows()?
                .into_iter()
                .filter(|window| unsafe {
                    executable_path(window.pid).ok().as_deref() == Some(expected.as_str())
                })
                .collect::<Vec<_>>();
            if !windows.is_empty() {
                return Ok(windows);
            }
            if Instant::now() >= deadline {
                return Ok(vec![]);
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    fn activate_window(&self, window: &Window, cancel: &AtomicBool) -> cu_protocol::Result<()> {
        let handle = hwnd(window)?;
        unsafe { activate_foreground(handle, cancel) }
    }
    fn validate(&self, window: &Window) -> cu_protocol::Result<()> {
        setup();
        let current = unsafe { window_info(hwnd(window)?)? };
        if current.pid != window.pid
            || current.process_identity != window.process_identity
            || current.bounds != window.bounds
        {
            return Err(cu_protocol::Error::new(
                "stale_window",
                "Window identity or geometry changed; observe again",
            ));
        }
        Ok(())
    }
    fn observe(
        &self,
        window: &Window,
        screenshot: bool,
        text: bool,
    ) -> cu_protocol::Result<Observation> {
        self.validate(window)?;
        let elements = if text {
            unsafe { accessibility(window)? }
        } else {
            vec![]
        };
        let images = if screenshot {
            vec![capture(window)?]
        } else {
            vec![]
        };
        self.validate(window)?;
        let now = now_ms();
        Ok(Observation{id:new_id(),helper_epoch:String::new(),window:window.clone(),captured_at_ms:now,expires_at_ms:now+30_000,elements,images,warnings:vec!["Accessibility tree is bounded to 512 nodes / 16 levels and is not atomic with capture".into()]})
    }
    fn perform(
        &self,
        window: &Window,
        action: &NativeAction,
        cancel: &AtomicBool,
    ) -> cu_protocol::Result<String> {
        setup();
        self.validate(window)?;
        if matches!(action, NativeAction::SetValue { .. }) {
            return Err(cu_protocol::Error::new(
                "unsupported_action",
                "Windows set_value is not exposed; use focus and type_text",
            ));
        }
        let codes = if let NativeAction::Key { keys } = action {
            keys.iter()
                .map(|k| virtual_key(k))
                .collect::<cu_protocol::Result<Vec<_>>>()?
        } else {
            vec![]
        };
        let handle = hwnd(window)?;
        unsafe {
            for key in [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN] {
                if GetAsyncKeyState(key.0 as i32) < 0 {
                    return Err(cu_protocol::Error::new(
                        "user_input_active",
                        "Release physically held modifiers before automation",
                    ));
                }
            }
            cancelled(cancel)?;
            activate_foreground(handle, cancel)?;
            let mut held = HeldInputs::new();
            match action {
                NativeAction::Click {
                    x,
                    y,
                    button,
                    click_count,
                } => {
                    move_to(handle, *x, *y, cancel)?;
                    let (down, up) = match button {
                        Button::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                        Button::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                        Button::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                    };
                    for _ in 0..*click_count {
                        cancelled(cancel)?;
                        held.mouse(down, up)?;
                        held.release_mouse()?;
                        if *click_count > 1 {
                            thread::sleep(Duration::from_millis(20));
                        }
                    }
                }
                NativeAction::TypeText { text } => {
                    for code in text.encode_utf16() {
                        foreground(handle, cancel)?;
                        held.key(code, true, true)?;
                        held.key(code, true, false)?;
                    }
                }
                NativeAction::Key { .. } => {
                    for code in &codes {
                        foreground(handle, cancel)?;
                        held.key(*code, false, true)?;
                    }
                    for code in codes.iter().rev() {
                        held.key(*code, false, false)?;
                    }
                }
                NativeAction::Scroll { x, y, dx, dy } => {
                    move_to(handle, *x, *y, cancel)?;
                    if *dy != 0 {
                        send_mouse(MOUSEEVENTF_WHEEL, (-dy * 120) as u32)?;
                    }
                    if *dx != 0 {
                        send_mouse(MOUSEEVENTF_HWHEEL, (dx * 120) as u32)?;
                    }
                }
                NativeAction::Drag {
                    from_x,
                    from_y,
                    to_x,
                    to_y,
                    duration_ms,
                } => {
                    move_to(handle, *from_x, *from_y, cancel)?;
                    held.mouse(MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP)?;
                    let steps = (*duration_ms / 10).max(1);
                    for step in 1..=steps {
                        let t = step as f64 / steps as f64;
                        move_to(
                            handle,
                            from_x + (to_x - from_x) * t,
                            from_y + (to_y - from_y) * t,
                            cancel,
                        )?;
                        thread::sleep(Duration::from_millis(10));
                    }
                    held.release_mouse()?;
                }
                NativeAction::SetValue { .. } => unreachable!(),
            }
        }
        Ok("sendinput".into())
    }
}
