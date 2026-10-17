use cu_core::{cancelled, new_id, now_ms, png_base64, Backend, NativeAction};
use cu_protocol::*;
use cu_protocol::{Button, Window};
use std::{
    sync::atomic::AtomicBool,
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{xproto::*, xtest::ConnectionExt as _},
    rust_connection::RustConnection,
};
use zbus::{
    blocking::{Connection as BusConnection, Proxy},
    zvariant::OwnedObjectPath,
};

pub struct LinuxBackend;
fn err(e: impl std::fmt::Display) -> Error {
    Error::new("os_error", e.to_string())
}
struct X11 {
    connection: RustConnection,
    screen: usize,
}
impl X11 {
    fn open() -> Result<Self> {
        if std::env::var_os("DISPLAY").is_none() {
            return Err(Error::new(
                "unsupported_environment",
                "An X11/XWayland DISPLAY is required; native Wayland is unsupported",
            ));
        }
        let (connection, screen) = x11rb::connect(None).map_err(err)?;
        connection
            .xtest_get_version(2, 2)
            .map_err(err)?
            .reply()
            .map_err(|_| Error::new("unsupported_environment", "XTEST extension is unavailable"))?;
        Ok(Self { connection, screen })
    }
    fn root(&self) -> u32 {
        self.connection.setup().roots[self.screen].root
    }
    fn atom(&self, name: &str) -> Result<u32> {
        Ok(self
            .connection
            .intern_atom(false, name.as_bytes())
            .map_err(err)?
            .reply()
            .map_err(err)?
            .atom)
    }
    fn property(&self, window: u32, name: &str) -> Result<Vec<u32>> {
        Ok(self
            .connection
            .get_property(false, window, self.atom(name)?, AtomEnum::ANY, 0, 4096)
            .map_err(err)?
            .reply()
            .map_err(err)?
            .value32()
            .map(|v| v.collect())
            .unwrap_or_default())
    }
    fn info(&self, id: u32) -> Result<Window> {
        let attrs = self
            .connection
            .get_window_attributes(id)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        if attrs.map_state != MapState::VIEWABLE {
            return Err(Error::new("invalid_window", "Window is not viewable"));
        }
        let pid = *self.property(id, "_NET_WM_PID")?.first().ok_or_else(|| {
            Error::new(
                "unsupported_window",
                "Window does not expose a process identity",
            )
        })?;
        let geometry = self
            .connection
            .get_geometry(id)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let origin = self
            .connection
            .translate_coordinates(id, self.root(), 0, 0)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let title = self
            .connection
            .get_property(
                false,
                id,
                self.atom("_NET_WM_NAME")?,
                AtomEnum::ANY,
                0,
                1024,
            )
            .map_err(err)?
            .reply()
            .map_err(err)?
            .value;
        Ok(Window {
            id: id.to_string(),
            pid,
            process_identity: process_identity(pid)?,
            title: String::from_utf8_lossy(&title).into_owned(),
            bounds: Rect {
                x: origin.dst_x as f64,
                y: origin.dst_y as f64,
                width: geometry.width as f64,
                height: geometry.height as f64,
            },
        })
    }
    fn validate(&self, window: &Window) -> Result<()> {
        let current = self.info(window.id.parse().map_err(err)?)?;
        if current.pid != window.pid
            || current.process_identity != window.process_identity
            || current.bounds != window.bounds
        {
            return Err(Error::new(
                "stale_window",
                "Window identity or geometry changed",
            ));
        }
        Ok(())
    }
    fn foreground(&self, window: &Window, cancel: &AtomicBool) -> Result<()> {
        cancelled(cancel)?;
        self.validate(window)?;
        if self
            .property(self.root(), "_NET_ACTIVE_WINDOW")?
            .first()
            .map(|v| v.to_string())
            != Some(window.id.clone())
        {
            return Err(Error::new(
                "focus_changed",
                "Selected X11 window is not active",
            ));
        }
        Ok(())
    }
    fn fake(&self, kind: u8, detail: u8, x: i16, y: i16) -> Result<()> {
        self.connection
            .xtest_fake_input(kind, detail, 0, self.root(), x, y, 0)
            .map_err(err)?
            .check()
            .map_err(err)?;
        self.connection.flush().map_err(err)
    }
    fn move_to(&self, window: &Window, x: f64, y: f64, cancel: &AtomicBool) -> Result<()> {
        self.foreground(window, cancel)?;
        if x < i16::MIN as f64 || x > i16::MAX as f64 || y < i16::MIN as f64 || y > i16::MAX as f64
        {
            return Err(Error::new(
                "out_of_bounds",
                "XTEST coordinate range exceeded",
            ));
        }
        self.fake(MOTION_NOTIFY_EVENT, 0, x.round() as i16, y.round() as i16)?;
        let pointer = self
            .connection
            .query_pointer(self.root())
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let id: u32 = window.id.parse().map_err(err)?;
        let mut frame = id;
        for _ in 0..32 {
            let tree = self
                .connection
                .query_tree(frame)
                .map_err(err)?
                .reply()
                .map_err(err)?;
            if tree.parent == self.root() {
                break;
            }
            if tree.parent == 0 {
                return Err(Error::new(
                    "stale_window",
                    "Window no longer belongs to this root",
                ));
            }
            frame = tree.parent;
        }
        if pointer.child != frame {
            return Err(Error::new(
                "target_occluded",
                "Pointer is over a different X11 top-level frame",
            ));
        }
        Ok(())
    }
    fn capture(&self, window: &Window) -> Result<Image> {
        let b = &window.bounds;
        let width = b.width as u16;
        let height = b.height as u16;
        if width as u64 * height as u64 > 16_000_000
            || b.x < 0.0
            || b.y < 0.0
            || b.x + b.width > self.connection.setup().roots[self.screen].width_in_pixels as f64
            || b.y + b.height > self.connection.setup().roots[self.screen].height_in_pixels as f64
        {
            return Err(Error::new(
                "capture_limit",
                "Window region is outside the root screen or exceeds size limit",
            ));
        }
        let image = self
            .connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                self.root(),
                b.x as i16,
                b.y as i16,
                width,
                height,
                u32::MAX,
            )
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let setup = self.connection.setup();
        let format = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == image.depth)
            .ok_or_else(|| Error::new("capture_format", "Unsupported X11 depth"))?;
        let visual = setup.roots[self.screen]
            .allowed_depths
            .iter()
            .flat_map(|d| &d.visuals)
            .find(|v| v.visual_id == image.visual)
            .ok_or_else(|| Error::new("capture_format", "Unsupported X11 visual"))?;
        let bytes = (format.bits_per_pixel / 8) as usize;
        if ![2, 3, 4].contains(&bytes) {
            return Err(Error::new("capture_format", "Unsupported pixel packing"));
        }
        let pad = format.scanline_pad as usize;
        if ![8, 16, 32].contains(&pad) {
            return Err(Error::new(
                "capture_format",
                "Unsupported X11 scanline padding",
            ));
        }
        let stride = (width as usize * format.bits_per_pixel as usize).div_ceil(pad) * (pad / 8);
        if image.data.len() < stride * height as usize {
            return Err(Error::new("capture_format", "Truncated X11 image"));
        }
        let component = |pixel: u32, mask: u32| -> u8 {
            if mask == 0 {
                return 0;
            }
            let shift = mask.trailing_zeros();
            (((pixel & mask) >> shift) * 255 / (mask >> shift)) as u8
        };
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = y * stride + x * bytes;
                let mut pixel = 0u32;
                for i in 0..bytes {
                    let shift = if setup.image_byte_order == ImageOrder::LSB_FIRST {
                        i * 8
                    } else {
                        (bytes - 1 - i) * 8
                    };
                    pixel |= (image.data[offset + i] as u32) << shift;
                }
                rgba.extend_from_slice(&[
                    component(pixel, visual.red_mask),
                    component(pixel, visual.green_mask),
                    component(pixel, visual.blue_mask),
                    255,
                ]);
            }
        }
        Ok(Image {
            id: new_id(),
            mime_type: "image/png".into(),
            data: png_base64(width as u32, height as u32, &rgba)?,
            width: width as u32,
            height: height as u32,
            screen_bounds: b.clone(),
            capture_method: "x11-screen-region".into(),
        })
    }
}
fn process_identity(pid: u32) -> Result<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(err)?;
    let tail = stat
        .rsplit_once(')')
        .ok_or_else(|| Error::new("invalid_window", "Cannot parse process identity"))?
        .1;
    let start = tail
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| Error::new("invalid_window", "Missing process start time"))?;
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").map_err(err)?;
    Ok(format!("{}:{pid}:{start}", boot.trim()))
}
type Node = (String, OwnedObjectPath);
fn registry() -> Node {
    (
        "org.a11y.atspi.Registry".into(),
        OwnedObjectPath::try_from("/org/a11y/atspi/accessible/root").unwrap(),
    )
}
struct A11y {
    connection: BusConnection,
}
impl A11y {
    fn open() -> Result<Self> {
        let session = BusConnection::session().map_err(err)?;
        let bus =
            Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus").map_err(err)?;
        let address: String = bus.call("GetAddress", &()).map_err(err)?;
        Ok(Self {
            connection: zbus::blocking::connection::Builder::address(address.as_str())
                .map_err(err)?
                .build()
                .map_err(err)?,
        })
    }
    fn proxy<'a>(&'a self, node: &'a Node, interface: &'a str) -> Result<Proxy<'a>> {
        Proxy::new(
            &self.connection,
            node.0.as_str(),
            node.1.as_str(),
            interface,
        )
        .map_err(err)
    }
    fn children(&self, node: &Node) -> Result<Vec<Node>> {
        self.proxy(node, "org.a11y.atspi.Accessible")?
            .call("GetChildren", &())
            .map_err(err)
    }
    fn pid(&self, node: &Node) -> Result<u32> {
        Proxy::new(
            &self.connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .map_err(err)?
        .call("GetConnectionUnixProcessID", &(node.0.as_str(),))
        .map_err(err)
    }
    fn extents(&self, node: &Node) -> Option<Rect> {
        let (x, y, w, h): (i32, i32, i32, i32) = self
            .proxy(node, "org.a11y.atspi.Component")
            .ok()?
            .call("GetExtents", &(0u32,))
            .ok()?;
        if w <= 0 || h <= 0 {
            return None;
        }
        Some(Rect {
            x: x as f64,
            y: y as f64,
            width: w as f64,
            height: h as f64,
        })
    }
    fn root(&self, window: &Window) -> Result<Node> {
        let mut matches = vec![];
        for app in self.children(&registry())? {
            if self.pid(&app).ok() != Some(window.pid) {
                continue;
            }
            for node in self.children(&app)? {
                let proxy = self.proxy(&node, "org.a11y.atspi.Accessible")?;
                let name: String = proxy.get_property("Name").unwrap_or_default();
                if let Some(b) = self.extents(&node) {
                    if name == window.title
                        && (b.x - window.bounds.x).abs() < 32.0
                        && (b.y - window.bounds.y).abs() < 64.0
                        && (b.width - window.bounds.width).abs() < 64.0
                        && (b.height - window.bounds.height).abs() < 64.0
                    {
                        matches.push(node.clone());
                    }
                }
            }
        }
        if matches.len() != 1 {
            return Err(Error::new(
                "ambiguous_window",
                "No unique AT-SPI window for the selected PID, title and bounds",
            ));
        }
        Ok(matches.remove(0))
    }
    fn tree(&self, window: &Window) -> Result<Vec<Element>> {
        let mut stack = vec![(self.root(window)?, 0)];
        let mut output = vec![];
        let deadline = Instant::now() + Duration::from_secs(3);
        while let Some((node, depth)) = stack.pop() {
            if output.len() >= 512 || Instant::now() > deadline {
                break;
            }
            let proxy = self.proxy(&node, "org.a11y.atspi.Accessible")?;
            let role: String = proxy.call("GetRoleName", &()).unwrap_or_default();
            let password = role.to_ascii_lowercase().contains("password");
            let states: Vec<u32> = proxy.call("GetState", &()).unwrap_or_default();
            let editable = states.first().is_some_and(|v| v & (1 << 7) != 0) && !password;
            let value = if password {
                None
            } else {
                self.proxy(&node, "org.a11y.atspi.Text")
                    .ok()
                    .and_then(|p| p.call::<_, _, String>("GetText", &(0i32, 4096i32)).ok())
            };
            output.push(Element {
                id: new_id(),
                role,
                name: if password {
                    "[password]".into()
                } else {
                    proxy.get_property("Name").unwrap_or_default()
                },
                value,
                bounds: self.extents(&node),
                editable,
                password,
                depth,
                locator: format!("{}|{}", node.0, node.1),
            });
            if depth < 16 && !password {
                let mut children = self.children(&node).unwrap_or_default();
                children.truncate(128);
                stack.extend(children.into_iter().rev().map(|n| (n, depth + 1)));
            }
        }
        Ok(output)
    }
}
async fn async_bus() -> Result<zbus::Connection> {
    let session = zbus::Connection::session().await.map_err(err)?;
    let bus = zbus::Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")
        .await
        .map_err(err)?;
    let address: String = bus.call("GetAddress", &()).await.map_err(err)?;
    zbus::connection::Builder::address(address.as_str())
        .map_err(err)?
        .build()
        .await
        .map_err(err)
}
async fn async_text(window: &Window, action: &NativeAction, cancel: &AtomicBool) -> Result<String> {
    let bus = async_bus().await?;
    let node = if let NativeAction::SetValue { locator, .. } = action {
        let (name, path) = locator
            .split_once('|')
            .ok_or_else(|| Error::new("stale_element", "Invalid AT-SPI reference"))?;
        (
            name.to_string(),
            OwnedObjectPath::try_from(path).map_err(err)?,
        )
    } else {
        let root = registry();
        let registry = zbus::Proxy::new(
            &bus,
            root.0.as_str(),
            root.1.as_str(),
            "org.a11y.atspi.Accessible",
        )
        .await
        .map_err(err)?;
        let apps: Vec<Node> = registry.call("GetChildren", &()).await.map_err(err)?;
        let mut candidates = vec![];
        for app in apps {
            let dbus = zbus::Proxy::new(
                &bus,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
            )
            .await
            .map_err(err)?;
            let pid: u32 = dbus
                .call("GetConnectionUnixProcessID", &(app.0.as_str(),))
                .await
                .map_err(err)?;
            if pid != window.pid {
                continue;
            }
            let app_proxy = zbus::Proxy::new(
                &bus,
                app.0.as_str(),
                app.1.as_str(),
                "org.a11y.atspi.Accessible",
            )
            .await
            .map_err(err)?;
            let children: Vec<Node> = app_proxy.call("GetChildren", &()).await.map_err(err)?;
            for candidate in children.into_iter().take(128) {
                let accessible = zbus::Proxy::new(
                    &bus,
                    candidate.0.as_str(),
                    candidate.1.as_str(),
                    "org.a11y.atspi.Accessible",
                )
                .await
                .map_err(err)?;
                let name: String = accessible.get_property("Name").await.unwrap_or_default();
                if name != window.title {
                    continue;
                }
                let component = zbus::Proxy::new(
                    &bus,
                    candidate.0.as_str(),
                    candidate.1.as_str(),
                    "org.a11y.atspi.Component",
                )
                .await
                .map_err(err)?;
                if let Ok((x, y, w, h)) = component
                    .call::<_, _, (i32, i32, i32, i32)>("GetExtents", &(0u32,))
                    .await
                {
                    let b = &window.bounds;
                    if (x as f64 - b.x).abs() < 32.0
                        && (y as f64 - b.y).abs() < 64.0
                        && (w as f64 - b.width).abs() < 64.0
                        && (h as f64 - b.height).abs() < 64.0
                    {
                        candidates.push(candidate.clone());
                    }
                }
            }
        }
        if candidates.len() != 1 {
            return Err(Error::new(
                "ambiguous_window",
                "No unique AT-SPI window for focused text input",
            ));
        }
        let mut stack = vec![candidates.remove(0)];
        let mut found = None;
        let mut count = 0;
        while let Some(node) = stack.pop() {
            count += 1;
            if count > 512 {
                break;
            }
            let proxy = zbus::Proxy::new(
                &bus,
                node.0.as_str(),
                node.1.as_str(),
                "org.a11y.atspi.Accessible",
            )
            .await
            .map_err(err)?;
            let states: Vec<u32> = proxy.call("GetState", &()).await.map_err(err)?;
            if states.first().is_some_and(|v| v & (1 << 12) != 0) {
                found = Some(node);
                break;
            }
            let children: Vec<Node> = proxy.call("GetChildren", &()).await.unwrap_or_default();
            stack.extend(children.into_iter().take(128));
        }
        found.ok_or_else(|| {
            Error::new(
                "unsupported_action",
                "No focused AT-SPI editor; generic Unicode keyboard injection is not supported",
            )
        })?
    };
    let dbus = zbus::Proxy::new(
        &bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await
    .map_err(err)?;
    let pid: u32 = dbus
        .call("GetConnectionUnixProcessID", &(node.0.as_str(),))
        .await
        .map_err(err)?;
    if pid != window.pid {
        return Err(Error::new(
            "stale_element",
            "AT-SPI node belongs to a different process",
        ));
    }
    let accessible = zbus::Proxy::new(
        &bus,
        node.0.as_str(),
        node.1.as_str(),
        "org.a11y.atspi.Accessible",
    )
    .await
    .map_err(err)?;
    let role: String = accessible.call("GetRoleName", &()).await.map_err(err)?;
    if role.to_ascii_lowercase().contains("password") {
        return Err(Error::new(
            "permission_denied",
            "Password editor is not permitted",
        ));
    }
    let editor = zbus::Proxy::new(
        &bus,
        node.0.as_str(),
        node.1.as_str(),
        "org.a11y.atspi.EditableText",
    )
    .await
    .map_err(err)?;
    X11::open()?.foreground(window, cancel)?;
    let accepted: bool = match action {
        NativeAction::SetValue { value, .. } => editor
            .call("SetTextContents", &(value.as_str(),))
            .await
            .map_err(err)?,
        NativeAction::TypeText { text } => {
            let text_proxy = zbus::Proxy::new(
                &bus,
                node.0.as_str(),
                node.1.as_str(),
                "org.a11y.atspi.Text",
            )
            .await
            .map_err(err)?;
            let caret: i32 = text_proxy.get_property("CaretOffset").await.map_err(err)?;
            editor
                .call(
                    "InsertText",
                    &(caret, text.as_str(), text.chars().count() as i32),
                )
                .await
                .map_err(err)?
        }
        _ => return Err(Error::new("unsupported_action", "Not a text action")),
    };
    if !accepted {
        return Err(Error::new(
            "input_failed",
            "AT-SPI editor rejected the operation",
        ));
    }
    Ok("at-spi-editable-text".into())
}
fn text_action(window: &Window, action: &NativeAction, cancel: &AtomicBool) -> Result<String> {
    async_io::block_on(futures_lite::future::race(
        async_text(window, action, cancel),
        async {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                cancelled(cancel)?;
                if Instant::now() > deadline {
                    return Err(Error::new(
                        "input_timeout",
                        "AT-SPI text operation timed out; result may be unknown",
                    ));
                }
                async_io::Timer::after(Duration::from_millis(10)).await;
            }
        },
    ))
}
fn keysym(key: &str) -> Result<u32> {
    Ok(match key.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => 0xffe3,
        "shift" => 0xffe1,
        "alt" => 0xffe9,
        "meta" | "win" => 0xffeb,
        "enter" | "return" => 0xff0d,
        "tab" => 0xff09,
        "space" => 0x20,
        "escape" | "esc" => 0xff1b,
        "backspace" => 0xff08,
        "delete" => 0xffff,
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        "home" => 0xff50,
        "end" => 0xff57,
        "pageup" => 0xff55,
        "pagedown" => 0xff56,
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphanumeric() => s.as_bytes()[0] as u32,
        _ => {
            return Err(Error::new(
                "unsupported_key",
                format!("Unsupported key: {key}"),
            ))
        }
    })
}
struct Held<'a> {
    x: &'a X11,
    keys: Vec<u8>,
    buttons: Vec<u8>,
}
impl Drop for Held<'_> {
    fn drop(&mut self) {
        for key in self.keys.drain(..).rev() {
            let _ = self.x.fake(KEY_RELEASE_EVENT, key, 0, 0);
        }
        for button in self.buttons.drain(..).rev() {
            let _ = self.x.fake(BUTTON_RELEASE_EVENT, button, 0, 0);
        }
    }
}
impl Backend for LinuxBackend {
    fn capabilities(&self) -> Capabilities {
        let state = X11::open();
        Capabilities{protocol_version:VERSION,platform:"linux".into(),backend:"at-spi-x11-xtest".into(),available:state.is_ok(),reason:state.err().map(|e|e.to_string()),actions:vec!["click","type_text","key","scroll","drag","set_value"].into_iter().map(String::from).collect(),screenshot_methods:vec!["x11-screen-region".into()],limitations:vec!["X11/EWMH/XTEST with session D-Bus and AT-SPI; native Wayland unsupported".into(),"Screenshots include occluding content in the authorized screen region".into(),"Unicode text requires a focused AT-SPI EditableText editor; not keyboard injection".into(),"WSLg/XWayland testing does not validate native Wayland".into()]}
    }
    fn list_windows(&self) -> Result<Vec<Window>> {
        let x = X11::open()?;
        let ids = x.property(x.root(), "_NET_CLIENT_LIST")?;
        Ok(ids.into_iter().filter_map(|id| x.info(id).ok()).collect())
    }
    fn validate(&self, window: &Window) -> Result<()> {
        X11::open()?.validate(window)
    }
    fn observe(
        &self,
        window: &Window,
        screenshot: bool,
        accessibility: bool,
    ) -> Result<Observation> {
        let x = X11::open()?;
        x.validate(window)?;
        let elements = if accessibility {
            A11y::open()?.tree(window)?
        } else {
            vec![]
        };
        let images = if screenshot {
            vec![x.capture(window)?]
        } else {
            vec![]
        };
        x.validate(window)?;
        let timestamp = now_ms();
        Ok(Observation{id:new_id(),helper_epoch:String::new(),window:window.clone(),captured_at_ms:timestamp,expires_at_ms:timestamp+30_000,elements,images,warnings:vec!["Screen-region capture may include other windows; tree and screenshot are not atomic".into()]})
    }
    fn prepare(&self, window: &Window) -> Result<()> {
        let x = X11::open()?;
        x.validate(window)?;
        let id: u32 = window.id.parse().map_err(err)?;
        let event = ClientMessageEvent::new(32, id, x.atom("_NET_ACTIVE_WINDOW")?, [2, 0, 0, 0, 0]);
        x.connection
            .send_event(
                false,
                x.root(),
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(err)?
            .check()
            .map_err(err)?;
        x.connection.flush().map_err(err)?;
        Ok(())
    }
    fn perform(
        &self,
        window: &Window,
        action: &NativeAction,
        cancel: &AtomicBool,
    ) -> Result<String> {
        let x = X11::open()?;
        x.validate(window)?;
        for _ in 0..20 {
            cancelled(cancel)?;
            if x.foreground(window, cancel).is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        x.foreground(window, cancel)?;
        if matches!(
            action,
            NativeAction::TypeText { .. } | NativeAction::SetValue { .. }
        ) {
            return text_action(window, action, cancel);
        }
        let mut held = Held {
            x: &x,
            keys: vec![],
            buttons: vec![],
        };
        match action {
            NativeAction::Click {
                x: cx,
                y,
                button,
                click_count,
            } => {
                x.move_to(window, *cx, *y, cancel)?;
                let button = match button {
                    Button::Left => 1,
                    Button::Middle => 2,
                    Button::Right => 3,
                };
                for _ in 0..*click_count {
                    cancelled(cancel)?;
                    held.buttons.push(button);
                    x.fake(BUTTON_PRESS_EVENT, button, 0, 0)?;
                    x.fake(BUTTON_RELEASE_EVENT, button, 0, 0)?;
                    held.buttons.clear();
                    if *click_count > 1 {
                        thread::sleep(Duration::from_millis(20));
                    }
                }
            }
            NativeAction::Key { keys } => {
                let setup = x.connection.setup();
                let map = x
                    .connection
                    .get_keyboard_mapping(
                        setup.min_keycode,
                        setup.max_keycode - setup.min_keycode + 1,
                    )
                    .map_err(err)?
                    .reply()
                    .map_err(err)?;
                let mut codes = vec![];
                for key in keys {
                    let symbol = keysym(key)?;
                    let index = map
                        .keysyms
                        .chunks(map.keysyms_per_keycode as usize)
                        .position(|group| group.first() == Some(&symbol))
                        .ok_or_else(|| {
                            Error::new(
                                "unsupported_key",
                                "Key not present in current layout's base level",
                            )
                        })?;
                    codes.push(setup.min_keycode + index as u8);
                }
                for code in codes {
                    x.foreground(window, cancel)?;
                    held.keys.push(code);
                    x.fake(KEY_PRESS_EVENT, code, 0, 0)?;
                }
                while let Some(&code) = held.keys.last() {
                    x.fake(KEY_RELEASE_EVENT, code, 0, 0)?;
                    held.keys.pop();
                }
            }
            NativeAction::Scroll { x: cx, y, dx, dy } => {
                x.move_to(window, *cx, *y, cancel)?;
                for (delta, negative, positive) in [(*dy, 4, 5), (*dx, 6, 7)] {
                    for _ in 0..delta.unsigned_abs() {
                        x.foreground(window, cancel)?;
                        let button = if delta < 0 { negative } else { positive };
                        held.buttons.push(button);
                        x.fake(BUTTON_PRESS_EVENT, button, 0, 0)?;
                        x.fake(BUTTON_RELEASE_EVENT, button, 0, 0)?;
                        held.buttons.clear();
                    }
                }
            }
            NativeAction::Drag {
                from_x,
                from_y,
                to_x,
                to_y,
                duration_ms,
            } => {
                x.move_to(window, *from_x, *from_y, cancel)?;
                held.buttons.push(1);
                x.fake(BUTTON_PRESS_EVENT, 1, 0, 0)?;
                let steps = (*duration_ms / 10).max(1);
                for step in 1..=steps {
                    let t = step as f64 / steps as f64;
                    x.move_to(
                        window,
                        from_x + (to_x - from_x) * t,
                        from_y + (to_y - from_y) * t,
                        cancel,
                    )?;
                    thread::sleep(Duration::from_millis(10));
                }
                x.fake(BUTTON_RELEASE_EVENT, 1, 0, 0)?;
                held.buttons.clear();
            }
            _ => unreachable!(),
        }
        Ok("xtest".into())
    }
}
