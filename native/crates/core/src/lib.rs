use base64::Engine;
use cu_protocol::*;
use serde::{Deserialize, Serialize};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(Error::new("cancelled", "Control session stopped"))
    } else {
        Ok(())
    }
}
pub fn png_base64(width: u32, height: u32, rgba: &[u8]) -> Result<String> {
    if width == 0
        || height == 0
        || width as u64 * height as u64 > 16_000_000
        || rgba.len() != width as usize * height as usize * 4
    {
        return Err(Error::new("capture_limit", "Invalid or oversized image"));
    }
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| Error::new("capture_failed", e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| Error::new("capture_failed", e.to_string()))?;
    }
    if output.len() > 9_000_000 {
        return Err(Error::new(
            "capture_limit",
            "Compressed image exceeds limit",
        ));
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(output))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationPolicy {
    pub id: String,
    pub display_name: String,
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub allowed_pids: Vec<u32>,
    #[serde(default)]
    pub allowed_apps: Vec<ApplicationPolicy>,
    pub allow_screenshot: bool,
    pub allow_accessibility: bool,
    pub allow_input: bool,
    #[serde(default = "default_duration")]
    pub session_seconds: u64,
}
fn default_duration() -> u64 {
    600
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            allowed_pids: vec![],
            allowed_apps: vec![],
            allow_screenshot: false,
            allow_accessibility: false,
            allow_input: false,
            session_seconds: 600,
        }
    }
}
impl Policy {
    pub fn check(&self, window: &Window) -> Result<()> {
        if !self.allowed_pids.contains(&window.pid) {
            return Err(Error::new(
                "permission_denied",
                "Window PID is not in the host policy",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NativeAction {
    Semantic {
        element: Element,
        operation: SemanticOperation,
        value: Option<String>,
    },
    Click {
        x: f64,
        y: f64,
        button: Button,
        click_count: u32,
    },
    TypeText {
        text: String,
    },
    Key {
        keys: Vec<String>,
    },
    Scroll {
        x: f64,
        y: f64,
        dx: i32,
        dy: i32,
    },
    Drag {
        from_x: f64,
        from_y: f64,
        to_x: f64,
        to_y: f64,
        duration_ms: u32,
    },
    SetValue {
        locator: String,
        value: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOperation {
    Invoke,
    Toggle,
    Select,
    SetValue,
}
impl SemanticOperation {
    pub fn capability(self) -> &'static str {
        match self {
            Self::Invoke => "invoke",
            Self::Toggle => "toggle",
            Self::Select => "select",
            Self::SetValue => "set_value",
        }
    }
}
fn semantic_action(
    observation: &Observation,
    element_id: &str,
    operation: SemanticOperation,
    value: Option<String>,
) -> Result<NativeAction> {
    let element = observation
        .elements
        .iter()
        .find(|e| e.id == element_id)
        .ok_or_else(|| Error::new("stale_reference", "Unknown element"))?;
    resolve_target(
        observation,
        &Target::Element {
            element_id: element_id.into(),
        },
    )?;
    if !element
        .capabilities
        .as_ref()
        .is_some_and(|c| c.iter().any(|s| s == operation.capability()))
    {
        return Err(Error::new(
            "unsupported_action",
            "Element does not advertise the requested semantic capability",
        ));
    }
    Ok(NativeAction::Semantic {
        element: element.clone(),
        operation,
        value,
    })
}

pub fn resolve_target(observation: &Observation, target: &Target) -> Result<(f64, f64)> {
    let point = match target {
        Target::Image { image_id, x, y } => {
            let image = observation
                .images
                .iter()
                .find(|i| &i.id == image_id)
                .ok_or_else(|| Error::new("stale_reference", "Unknown image reference"))?;
            if !x.is_finite()
                || !y.is_finite()
                || *x < 0.0
                || *y < 0.0
                || *x >= image.width as f64
                || *y >= image.height as f64
            {
                return Err(Error::new(
                    "out_of_bounds",
                    "Image coordinates are outside the captured pixels",
                ));
            }
            (
                image.screen_bounds.x + x * image.screen_bounds.width / image.width as f64,
                image.screen_bounds.y + y * image.screen_bounds.height / image.height as f64,
            )
        }
        Target::Element { element_id } => {
            let element = observation
                .elements
                .iter()
                .find(|e| &e.id == element_id)
                .ok_or_else(|| Error::new("stale_reference", "Unknown element reference"))?;
            let rect = element
                .bounds
                .as_ref()
                .ok_or_else(|| Error::new("unsupported_action", "Element has no visible bounds"))?;
            if element.password {
                return Err(Error::new(
                    "permission_denied",
                    "Password elements cannot be targeted",
                ));
            }
            (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
        }
    };
    let b = &observation.window.bounds;
    if !point.0.is_finite()
        || !point.1.is_finite()
        || point.0 < b.x
        || point.1 < b.y
        || point.0 >= b.x + b.width
        || point.1 >= b.y + b.height
    {
        return Err(Error::new(
            "out_of_bounds",
            "Target is outside the authorized window",
        ));
    }
    Ok(point)
}
pub fn resolve_action(observation: &Observation, action: &Action) -> Result<NativeAction> {
    Ok(match action {
        Action::Invoke { element_id } => {
            semantic_action(observation, element_id, SemanticOperation::Invoke, None)?
        }
        Action::Toggle { element_id } => {
            semantic_action(observation, element_id, SemanticOperation::Toggle, None)?
        }
        Action::Select { element_id } => {
            semantic_action(observation, element_id, SemanticOperation::Select, None)?
        }
        Action::Click {
            target,
            button,
            click_count,
        } => {
            if !(1..=3).contains(click_count) {
                return Err(Error::new(
                    "invalid_params",
                    "click_count must be between 1 and 3",
                ));
            }
            let (x, y) = resolve_target(observation, target)?;
            NativeAction::Click {
                x,
                y,
                button: *button,
                click_count: *click_count,
            }
        }
        Action::TypeText { text } => {
            validate_text(text)?;
            NativeAction::TypeText { text: text.clone() }
        }
        Action::Key { keys } => {
            if keys.is_empty() || keys.len() > 5 || keys.iter().any(|k| k.len() > 24) {
                return Err(Error::new(
                    "invalid_params",
                    "Use one to five supported keys",
                ));
            }
            NativeAction::Key { keys: keys.clone() }
        }
        Action::Scroll { target, dx, dy } => {
            let (x, y) = resolve_target(observation, target)?;
            if dx.abs_diff(0) > 20 || dy.abs_diff(0) > 20 {
                return Err(Error::new(
                    "invalid_params",
                    "Scroll is bounded to 20 detents per axis",
                ));
            }
            NativeAction::Scroll {
                x,
                y,
                dx: *dx,
                dy: *dy,
            }
        }
        Action::Drag {
            from,
            to,
            duration_ms,
        } => {
            let (from_x, from_y) = resolve_target(observation, from)?;
            let (to_x, to_y) = resolve_target(observation, to)?;
            if !(50..=3000).contains(duration_ms) {
                return Err(Error::new(
                    "invalid_params",
                    "Drag duration must be 50..3000 ms",
                ));
            }
            NativeAction::Drag {
                from_x,
                from_y,
                to_x,
                to_y,
                duration_ms: *duration_ms,
            }
        }
        Action::SetValue { element_id, value } => {
            validate_text(value)?;
            let element = observation
                .elements
                .iter()
                .find(|e| &e.id == element_id)
                .ok_or_else(|| Error::new("stale_reference", "Unknown element"))?;
            if !element.editable || element.password {
                return Err(Error::new(
                    "unsupported_action",
                    "Element is not a non-password editor",
                ));
            }
            if element.capabilities.is_some() {
                return semantic_action(
                    observation,
                    element_id,
                    SemanticOperation::SetValue,
                    Some(value.clone()),
                );
            }
            NativeAction::SetValue {
                locator: element.locator.clone(),
                value: value.clone(),
            }
        }
    })
}
fn validate_text(text: &str) -> Result<()> {
    if text.len() > 16_384 || text.contains('\0') {
        return Err(Error::new(
            "invalid_params",
            "Text exceeds 16 KiB or contains NUL",
        ));
    }
    Ok(())
}

pub trait Backend {
    fn capabilities(&self) -> Capabilities;
    fn list_windows(&self) -> Result<Vec<Window>>;
    fn list_apps(&self, _apps: &[ApplicationPolicy]) -> Result<Vec<Application>> {
        Err(Error::new(
            "unsupported_action",
            "Application discovery is not implemented on this backend",
        ))
    }
    fn launch_app(&self, _app: &ApplicationPolicy, _cancel: &AtomicBool) -> Result<Vec<Window>> {
        Err(Error::new(
            "unsupported_action",
            "Application launch is not implemented on this backend",
        ))
    }
    fn activate_window(&self, _window: &Window, _cancel: &AtomicBool) -> Result<()> {
        Err(Error::new(
            "unsupported_action",
            "Window activation is not implemented on this backend",
        ))
    }
    fn observe(
        &self,
        window: &Window,
        screenshot: bool,
        accessibility: bool,
    ) -> Result<Observation>;
    fn validate(&self, window: &Window) -> Result<()>;
    fn prepare(&self, window: &Window) -> Result<()> {
        self.validate(window)
    }
    fn preflight(&self, _window: &Window, _action: &NativeAction) -> Result<()> {
        Err(Error::new(
            "unsupported_action",
            "Semantic actions are not implemented on this backend",
        ))
    }
    fn perform(
        &self,
        window: &Window,
        action: &NativeAction,
        cancel: &AtomicBool,
    ) -> Result<String>;
}

pub struct DesktopLease {
    file: std::fs::File,
}
impl DesktopLease {
    pub fn acquire() -> Result<Self> {
        use fs2::FileExt;
        let directory = lease_directory()?;
        std::fs::create_dir_all(&directory)
            .map_err(|e| Error::new("lease_error", e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let meta = std::fs::symlink_metadata(&directory)
                .map_err(|e| Error::new("lease_error", e.to_string()))?;
            if meta.file_type().is_symlink() || meta.uid() != unsafe { libc_getuid() } {
                return Err(Error::new("lease_error", "Unsafe lease directory"));
            }
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| Error::new("lease_error", e.to_string()))?;
        }
        let path = directory.join("desktop.lock");
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::new("lease_error", "Lease file is a symbolic link"));
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| Error::new("lease_error", e.to_string()))?;
        file.try_lock_exclusive().map_err(|_| {
            Error::new(
                "desktop_busy",
                "Another helper controls this user's desktop",
            )
        })?;
        Ok(Self { file })
    }
}
impl Drop for DesktopLease {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}
#[cfg(unix)]
unsafe fn libc_getuid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    getuid()
}
fn lease_directory() -> Result<std::path::PathBuf> {
    // Intentionally one lock per user across displays: conservative, never split by caller input.
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA");
    #[cfg(not(windows))]
    let root = std::env::var_os("HOME");
    root.map(|p| std::path::PathBuf::from(p).join(".independent-computer-use"))
        .ok_or_else(|| Error::new("lease_error", "No per-user data directory"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn obs() -> Observation {
        Observation {
            id: "o".into(),
            helper_epoch: "e".into(),
            window: Window {
                id: "1".into(),
                pid: 1,
                process_identity: "p".into(),
                title: String::new(),
                bounds: Rect {
                    x: -500.0,
                    y: 20.0,
                    width: 400.0,
                    height: 200.0,
                },
            },
            captured_at_ms: 0,
            expires_at_ms: 100,
            elements: vec![],
            images: vec![Image {
                id: "i".into(),
                mime_type: "image/png".into(),
                data: String::new(),
                width: 800,
                height: 400,
                screen_bounds: Rect {
                    x: -500.0,
                    y: 20.0,
                    width: 400.0,
                    height: 200.0,
                },
                capture_method: "test".into(),
            }],
            warnings: vec![],
        }
    }
    #[test]
    fn scaled_negative_origin() {
        assert_eq!(
            resolve_target(
                &obs(),
                &Target::Image {
                    image_id: "i".into(),
                    x: 200.0,
                    y: 100.0
                }
            )
            .unwrap(),
            (-400.0, 70.0)
        );
    }
    #[test]
    fn invalid_coordinates() {
        for x in [-1.0, 800.0, f64::NAN] {
            assert!(resolve_target(
                &obs(),
                &Target::Image {
                    image_id: "i".into(),
                    x,
                    y: 0.0
                }
            )
            .is_err());
        }
    }
    #[test]
    fn stale_image() {
        assert!(resolve_target(
            &obs(),
            &Target::Image {
                image_id: "old".into(),
                x: 0.0,
                y: 0.0
            }
        )
        .is_err());
    }
    #[test]
    fn bounded_text_and_key_sequences() {
        assert!(validate_text(&"中".repeat(5462)).is_err());
        assert!(validate_text("contains\0nul").is_err());
        assert!(validate_text(&"a".repeat(16384)).is_ok());
        for keys in [vec![], vec!["a".into(); 6], vec!["x".repeat(25)]] {
            assert!(resolve_action(&obs(), &Action::Key { keys }).is_err());
        }
    }
    #[test]
    fn bounded_drag_duration() {
        let target = Target::Image {
            image_id: "i".into(),
            x: 100.0,
            y: 100.0,
        };
        for duration_ms in [0, 49, 3001, u32::MAX] {
            assert!(resolve_action(
                &obs(),
                &Action::Drag {
                    from: target.clone(),
                    to: target.clone(),
                    duration_ms
                }
            )
            .is_err());
        }
        for duration_ms in [50, 3000] {
            assert!(resolve_action(
                &obs(),
                &Action::Drag {
                    from: target.clone(),
                    to: target.clone(),
                    duration_ms
                }
            )
            .is_ok());
        }
    }
    #[test]
    fn password_targets_and_non_editors_are_rejected() {
        let mut observation = obs();
        observation.elements.push(Element {
            id: "password".into(),
            role: "edit".into(),
            name: "[password]".into(),
            value: None,
            bounds: Some(Rect {
                x: -400.0,
                y: 40.0,
                width: 100.0,
                height: 20.0,
            }),
            editable: true,
            password: true,
            depth: 1,
            locator: "root/0".into(),
            capabilities: None,
        });
        assert!(resolve_target(
            &observation,
            &Target::Element {
                element_id: "password".into()
            }
        )
        .is_err());
        let action = Action::SetValue {
            element_id: "password".into(),
            value: "fixture".into(),
        };
        assert!(resolve_action(&observation, &action).is_err());
        observation.elements[0].password = false;
        observation.elements[0].editable = false;
        assert!(resolve_action(&observation, &action).is_err());
    }
    #[test]
    fn mapped_pixels_cannot_escape_the_authorized_window() {
        let mut observation = obs();
        observation.images[0].screen_bounds.x = 1000.0;
        assert!(resolve_target(
            &observation,
            &Target::Image {
                image_id: "i".into(),
                x: 0.0,
                y: 0.0
            }
        )
        .is_err());
    }
    #[test]
    fn policy_and_capture_default_to_denial() {
        assert_eq!(
            Policy::default().check(&obs().window).unwrap_err().code,
            "permission_denied"
        );
        assert!(png_base64(0, 1, &[]).is_err());
        assert!(png_base64(1, 1, &[0; 3]).is_err());
        assert!(png_base64(5000, 5000, &[]).is_err());
    }
    #[test]
    fn semantic_actions_require_capabilities_and_retain_identity() {
        let mut observation = obs();
        let element = Element {
            id: "e".into(),
            role: "button".into(),
            name: "Fixture".into(),
            value: None,
            bounds: Some(Rect {
                x: -400.0,
                y: 40.0,
                width: 100.0,
                height: 20.0,
            }),
            editable: true,
            password: false,
            depth: 1,
            locator: "root/0".into(),
            capabilities: None,
        };
        observation.elements.push(element);
        let actions = [
            Action::Invoke {
                element_id: "e".into(),
            },
            Action::Toggle {
                element_id: "e".into(),
            },
            Action::Select {
                element_id: "e".into(),
            },
            Action::SetValue {
                element_id: "e".into(),
                value: "replacement".into(),
            },
        ];
        for action in &actions[..3] {
            assert_eq!(
                resolve_action(&observation, action).unwrap_err().code,
                "unsupported_action"
            );
        }
        observation.elements[0].capabilities = Some(vec![
            "invoke".into(),
            "toggle".into(),
            "select".into(),
            "set_value".into(),
        ]);
        for action in &actions {
            match resolve_action(&observation, action).unwrap() {
                NativeAction::Semantic { element, .. } => {
                    assert_eq!(element.locator, "root/0");
                    assert_eq!(element.name, "Fixture");
                }
                _ => panic!("Semantic intent lost"),
            }
        }
        observation.elements[0].password = true;
        for action in &actions {
            assert!(resolve_action(&observation, action).is_err());
        }
        observation.elements[0].password = false;
        observation.elements[0].bounds = None;
        assert!(resolve_action(&observation, &actions[0]).is_err());
    }
    #[test]
    fn invalid_scroll() {
        assert!(resolve_action(
            &obs(),
            &Action::Scroll {
                target: Target::Image {
                    image_id: "i".into(),
                    x: 0.0,
                    y: 0.0
                },
                dx: i32::MIN,
                dy: 0
            }
        )
        .is_err());
    }
}
