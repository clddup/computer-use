use super::*;

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
struct ThreadAttachment(u32, u32, bool);
impl Drop for ThreadAttachment {
    fn drop(&mut self) {
        if self.2 {
            unsafe {
                let _ = AttachThreadInput(self.0, self.1, false);
            }
        }
    }
}
pub(super) unsafe fn activate_foreground(
    handle: HWND,
    cancel: &AtomicBool,
) -> cu_protocol::Result<()> {
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
    let _target_attachment = ThreadAttachment(current_thread, target_thread, attached_target);
    let _foreground_attachment =
        ThreadAttachment(current_thread, foreground_thread, attached_foreground);
    if IsIconic(handle).as_bool() {
        return Err(cu_protocol::Error::new(
            "invalid_window",
            "Window was minimized; observe a visible window again",
        ));
    }
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
pub(super) fn perform(
    window: &Window,
    action: &NativeAction,
    cancel: &AtomicBool,
) -> cu_protocol::Result<String> {
    setup();
    WindowsBackend.validate(window)?;
    if let NativeAction::Semantic {
        element,
        operation,
        value,
    } = action
    {
        return unsafe { semantic_perform(window, element, *operation, value, cancel) };
    }
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
        WindowsBackend.validate(window)?;
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
            NativeAction::SetValue { .. } | NativeAction::Semantic { .. } => unreachable!(),
        }
    }
    Ok("sendinput".into())
}
