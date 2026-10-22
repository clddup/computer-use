mod accessibility;
mod applications;
mod capture;
mod input;
use accessibility::{accessibility, semantic_node, semantic_perform};
use capture::capture;

use cu_core::{
    cancelled, new_id, now_ms, png_base64, ApplicationPolicy, Backend, NativeAction,
    SemanticOperation,
};
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
        (*(param.0 as *mut Vec<Window>)).push(window);
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
impl Backend for WindowsBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities { protocol_version:VERSION,platform:"windows".into(),backend:"win32-uia-wgc-sendinput".into(),available:true,reason:None,
            actions:vec!["list_apps","launch_app","get_window","activate_window","click","type_text","key","scroll","drag","invoke","toggle","select","set_value"].into_iter().map(String::from).collect(),screenshot_methods:vec!["windows-graphics-capture".into()],
            limitations:vec!["Interactive non-elevated desktop only; minimized windows rejected".into(),"Input requires foreground and unoccluded target; no secure desktop".into(),"AX and image timestamps are not atomic; semantic UIA actions do not inject mouse/keyboard input or explicitly activate the window; providers may change focus".into()] }
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
        applications::list_apps(apps)
    }
    fn launch_app(
        &self,
        app: &ApplicationPolicy,
        cancel: &AtomicBool,
    ) -> cu_protocol::Result<Vec<Window>> {
        applications::launch_app(app, cancel)
    }
    fn activate_window(&self, window: &Window, cancel: &AtomicBool) -> cu_protocol::Result<()> {
        self.validate(window)?;
        cancelled(cancel)?;
        let handle = hwnd(window)?;
        unsafe { input::activate_foreground(handle, cancel) }
            .map_err(cu_protocol::Error::after_dispatch)?;
        self.validate(window)
            .map_err(cu_protocol::Error::after_dispatch)?;
        cancelled(cancel).map_err(cu_protocol::Error::after_dispatch)
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
    fn preflight(&self, window: &Window, action: &NativeAction) -> cu_protocol::Result<()> {
        setup();
        self.validate(window)?;
        if let NativeAction::Semantic {
            element, operation, ..
        } = action
        {
            unsafe {
                let _guard = ComGuard::new()?;
                semantic_node(window, element, *operation)?;
            }
            Ok(())
        } else {
            Err(cu_protocol::Error::new(
                "unsupported_action",
                "Expected a semantic action",
            ))
        }
    }
    fn perform(
        &self,
        window: &Window,
        action: &NativeAction,
        cancel: &AtomicBool,
    ) -> cu_protocol::Result<String> {
        input::perform(window, action, cancel)
    }
}
