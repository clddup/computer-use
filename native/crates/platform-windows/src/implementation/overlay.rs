//! Passive pointer rendering only: this module never injects input or hides the
//! system cursor. HWND operations are confined to a dedicated PMv2 message pump.
use super::*;
use std::{
    sync::{
        atomic::{AtomicIsize, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

const CLASS_NAME: PCWSTR = w!("CuIndependentPointerOverlay");
const COLOR_KEY: COLORREF = COLORREF(0x00ff00ff);
const POINTER_WIDTH: i32 = 22;
const POINTER_HEIGHT: i32 = 30;
const ACK_TIMEOUT: Duration = Duration::from_millis(250);
static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);
type Reply = SyncSender<cu_protocol::Result<()>>;

enum Command {
    Move {
        target: Window,
        x: i32,
        y: i32,
        expires: Instant,
        reply: Reply,
    },
    Hide(Reply),
    Shutdown(Reply),
}

pub(super) struct Service {
    tx: Sender<Command>,
    state: Mutex<Option<Window>>,
}

// A title is metadata, not an identity. A browser's title may change after a click
// without invalidating the bound process/window or its physical coordinate system.
fn same_target(a: &Window, b: &Window) -> bool {
    a.id == b.id
        && a.pid == b.pid
        && a.process_identity == b.process_identity
        && a.bounds == b.bounds
}

fn live_target(window: &Window) -> bool {
    super::hwnd(window)
        .ok()
        .and_then(|handle| unsafe { super::window_info(handle) }.ok())
        .is_some_and(|current| same_target(window, &current))
}

fn failure(code: &str, message: &str) -> cu_protocol::Error {
    cu_protocol::Error::new(code, message)
}

impl Service {
    pub(super) fn start() -> cu_protocol::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("cu-windows-overlay-pump".into())
            .spawn(move || pump(rx, ready_tx))
            .map_err(|e| failure("presentation_failed", &e.to_string()))?;
        match ready_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(())) => Ok(Self {
                tx,
                state: Mutex::new(None),
            }),
            Ok(Err(message)) => Err(failure("presentation_failed", &message)),
            Err(_) => Err(failure("presentation_failed", "Overlay pump did not start")),
        }
    }

    fn request(&self, command: impl FnOnce(Reply) -> Command) -> cu_protocol::Result<()> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.tx
            .send(command(reply))
            .map_err(|_| failure("cleanup_unconfirmed", "Overlay pump stopped"))?;
        rx.recv_timeout(ACK_TIMEOUT)
            .map_err(|_| failure("cleanup_unconfirmed", "Overlay acknowledgement timed out"))?
    }

    pub(super) fn bind(&self, window: Window) -> cu_protocol::Result<()> {
        if !live_target(&window) {
            self.hide()?;
            return Err(failure(
                "stale_window",
                "Presentation target closed or changed",
            ));
        }
        let mut bound = self
            .state
            .lock()
            .map_err(|_| failure("presentation_failed", "Presentation state poisoned"))?;
        if bound
            .as_ref()
            .is_some_and(|previous| same_target(previous, &window))
        {
            return Ok(());
        }
        self.request(Command::Hide)?;
        *bound = Some(window);
        Ok(())
    }

    pub(super) fn move_pointer(&self, id: usize, x: i32, y: i32) -> cu_protocol::Result<()> {
        let window = self
            .state
            .lock()
            .map_err(|_| failure("presentation_failed", "Presentation state poisoned"))?
            .clone()
            .ok_or_else(|| failure("presentation_unbound", "No observed target is bound"))?;
        if window.id.parse::<usize>().ok() != Some(id) || !live_target(&window) {
            self.hide()?;
            return Err(failure(
                "stale_window",
                "Presentation target closed or changed",
            ));
        }
        // An expired queued update must not resurrect a pointer after a timeout.
        let expires = Instant::now() + ACK_TIMEOUT;
        self.request(|reply| Command::Move {
            target: window,
            x,
            y,
            expires,
            reply,
        })
    }

    pub(super) fn suspend(&self) -> cu_protocol::Result<()> {
        // Hide during WindowFromPoint but retain the complete bound identity.
        self.request(Command::Hide)
    }

    pub(super) fn hide(&self) -> cu_protocol::Result<()> {
        self.request(Command::Hide)?;
        *self
            .state
            .lock()
            .map_err(|_| failure("presentation_failed", "Presentation state poisoned"))? = None;
        Ok(())
    }

    pub(super) fn close(&self) -> cu_protocol::Result<()> {
        self.request(Command::Shutdown)
    }

    pub(super) fn maintain(&self) -> cu_protocol::Result<()> {
        let bound = self
            .state
            .lock()
            .map_err(|_| failure("presentation_failed", "Presentation state poisoned"))?
            .clone();
        if bound.as_ref().is_some_and(|window| !live_target(window)) {
            self.hide()?;
        }
        if OVERLAY_HWND.load(Ordering::Acquire) == 0 {
            return Err(failure(
                "presentation_failed",
                "Overlay pump lost its window",
            ));
        }
        Ok(())
    }
}

pub(super) fn is_overlay_window(hwnd: HWND) -> bool {
    !hwnd.0.is_null() && OVERLAY_HWND.load(Ordering::Acquire) == hwnd.0 as isize
}

struct OwnedWindow(HWND);
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = ShowWindow(self.0, SW_HIDE);
            let _ = DestroyWindow(self.0);
        }
        OVERLAY_HWND.store(0, Ordering::Release);
    }
}

unsafe fn hide_confirmed(hwnd: HWND) -> cu_protocol::Result<()> {
    let _ = ShowWindow(hwnd, SW_HIDE);
    if IsWindowVisible(hwnd).as_bool() {
        Err(failure(
            "cleanup_unconfirmed",
            "Overlay is still visible after hide",
        ))
    } else {
        Ok(())
    }
}

fn pump(rx: Receiver<Command>, ready: SyncSender<std::result::Result<(), String>>) {
    unsafe {
        // Do not rely on process DPI setup succeeding after another library has
        // already selected a context. This thread must use physical coordinates.
        if SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_invalid() {
            let _ = ready.send(Err("Could not select per-monitor-v2 DPI awareness".into()));
            return;
        }
        let owned = match create_window() {
            Ok(hwnd) => OwnedWindow(hwnd),
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };
        let hwnd = owned.0;
        OVERLAY_HWND.store(hwnd.0 as isize, Ordering::Release);
        if ready.send(Ok(())).is_err() {
            return;
        }
        let mut visible_target: Option<Window> = None;
        let mut last_validation = Instant::now();
        loop {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    return;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if last_validation.elapsed() >= Duration::from_millis(100) {
                if visible_target
                    .as_ref()
                    .is_some_and(|target| !live_target(target))
                {
                    let _ = hide_confirmed(hwnd);
                    visible_target = None;
                }
                last_validation = Instant::now();
            }
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(Command::Move {
                    target,
                    x,
                    y,
                    expires,
                    reply,
                }) => {
                    let result = if Instant::now() >= expires || !live_target(&target) {
                        let _ = hide_confirmed(hwnd);
                        Err(failure(
                            "stale_window",
                            "Pointer update expired or target changed",
                        ))
                    } else {
                        // Never use HWND_TOPMOST. This popup stays in the ordinary
                        // z-order and does not activate or change the target's owner.
                        let result = SetWindowPos(
                            hwnd,
                            Some(HWND_TOP),
                            x.saturating_add(2),
                            y.saturating_add(2),
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                        )
                        .map_err(|e| failure("presentation_failed", &e.to_string()));
                        if result.is_ok() && Instant::now() < expires {
                            visible_target = Some(target);
                            let _ = InvalidateRect(Some(hwnd), None, false);
                            result
                        } else {
                            let _ = hide_confirmed(hwnd);
                            visible_target = None;
                            result.and(Err(failure(
                                "presentation_failed",
                                "Pointer update deadline elapsed",
                            )))
                        }
                    };
                    let _ = reply.send(result);
                }
                Ok(Command::Hide(reply)) => {
                    let result = hide_confirmed(hwnd);
                    visible_target = None;
                    let _ = reply.send(result);
                }
                Ok(Command::Shutdown(reply)) => {
                    let result = hide_confirmed(hwnd).and_then(|_| {
                        DestroyWindow(hwnd)
                            .map_err(|e| failure("cleanup_unconfirmed", &e.to_string()))
                    });
                    if result.is_ok() {
                        OVERLAY_HWND.store(0, Ordering::Release);
                    }
                    let _ = reply.send(result);
                    return;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

unsafe fn create_window() -> std::result::Result<HWND, String> {
    let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
    let class = WNDCLASSW {
        hInstance: instance.into(),
        lpszClassName: CLASS_NAME,
        lpfnWndProc: Some(window_proc),
        style: CS_HREDRAW | CS_VREDRAW,
        ..Default::default()
    };
    if RegisterClassW(&class) == 0 && GetLastError().0 != ERROR_CLASS_ALREADY_EXISTS.0 {
        return Err(format!("RegisterClassW failed: {:?}", GetLastError()));
    }
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
        CLASS_NAME,
        w!("Computer Use Pointer"),
        WS_POPUP,
        0,
        0,
        POINTER_WIDTH,
        POINTER_HEIGHT,
        None,
        None,
        Some(instance.into()),
        None,
    )
    .map_err(|e| e.to_string())?;
    let configured = SetLayeredWindowAttributes(hwnd, COLOR_KEY, 255, LWA_COLORKEY)
        .and_then(|_| SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE));
    if let Err(error) = configured {
        let _ = DestroyWindow(hwnd);
        return Err(format!("Overlay configuration failed: {error}"));
    }
    Ok(hwnd)
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // HTTRANSPARENT is a supplementary defense, not our cross-thread
        // transparency guarantee. Layered + WS_EX_TRANSPARENT supplies that.
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            draw_pointer(dc);
            let _ = EndPaint(hwnd, &paint);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            OVERLAY_HWND.store(0, Ordering::Release);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn draw_pointer(dc: HDC) {
    let rect = RECT {
        left: 0,
        top: 0,
        right: POINTER_WIDTH,
        bottom: POINTER_HEIGHT,
    };
    let clear = CreateSolidBrush(COLOR_KEY);
    let _ = FillRect(dc, &rect, clear);
    let _ = DeleteObject(clear.into());
    // Use stock pen/brush resources so a failed allocation never leaves a GDI
    // object selected or leaks it. A white contour surrounds a black body.
    let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
    let old_brush = SelectObject(dc, GetStockObject(WHITE_BRUSH));
    let outline = [
        POINT { x: 2, y: 1 },
        POINT { x: 2, y: 25 },
        POINT { x: 8, y: 19 },
        POINT { x: 13, y: 29 },
        POINT { x: 18, y: 27 },
        POINT { x: 13, y: 17 },
        POINT { x: 21, y: 17 },
    ];
    let _ = Polygon(dc, &outline);
    let _ = SelectObject(dc, GetStockObject(BLACK_BRUSH));
    let body = [
        POINT { x: 5, y: 5 },
        POINT { x: 5, y: 20 },
        POINT { x: 9, y: 16 },
        POINT { x: 14, y: 26 },
        POINT { x: 16, y: 25 },
        POINT { x: 11, y: 14 },
        POINT { x: 17, y: 14 },
    ];
    let _ = Polygon(dc, &body);
    let _ = SelectObject(dc, old_brush);
    let _ = SelectObject(dc, old_pen);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> Window {
        Window {
            id: "123".into(),
            pid: 12,
            process_identity: "12:creation-time".into(),
            title: "First".into(),
            bounds: cu_protocol::Rect {
                x: -1920.0,
                y: -200.0,
                width: 1280.0,
                height: 720.0,
            },
        }
    }
    #[test]
    fn title_changes_do_not_rebind_pointer() {
        let original = target();
        let mut changed = original.clone();
        changed.title = "After navigation".into();
        assert!(same_target(&original, &changed));
    }
    #[test]
    fn window_process_reuse_and_geometry_change_invalidate_pointer() {
        let original = target();
        for field in 0..4 {
            let mut changed = original.clone();
            match field {
                0 => changed.id = "124".into(),
                1 => changed.pid += 1,
                2 => changed.process_identity.push_str("-new"),
                _ => changed.bounds.x += 1.0,
            }
            assert!(!same_target(&original, &changed));
        }
    }
    #[test]
    fn exact_hwnd_filter_never_matches_null_or_arbitrary_windows() {
        // No window creation and no mutation of the global registry in tests.
        assert!(!is_overlay_window(HWND::default()));
        assert!(!is_overlay_window(HWND(usize::MAX as *mut _)));
    }
}
