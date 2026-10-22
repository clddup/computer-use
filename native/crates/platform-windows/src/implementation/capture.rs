use super::*;

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
pub(super) fn capture(window: &Window) -> cu_protocol::Result<Image> {
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
