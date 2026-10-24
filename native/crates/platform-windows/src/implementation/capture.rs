use super::*;
use std::sync::{atomic::Ordering, Arc, Mutex};

const START_TIMEOUT: Duration = Duration::from_secs(3);
const FRAME_TIMEOUT: Duration = Duration::from_secs(3);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_PIXELS: u64 = 16_000_000;

type CaptureError = Box<dyn std::error::Error + Send + Sync>;

struct RgbaFrame {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

// One retained frame, not a queue. Encoding and GPU readback never hold this lock.
struct LatestSlot {
    generation: u64,
    active: bool,
    latest: Option<Arc<RgbaFrame>>,
    failure: Option<String>,
}
impl LatestSlot {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            active: true,
            latest: None,
            failure: None,
        }
    }
    fn publish(&mut self, generation: u64, frame: RgbaFrame) {
        if self.active && self.generation == generation && self.failure.is_none() {
            self.latest = Some(Arc::new(frame));
        }
    }
    fn fail(&mut self, message: String) {
        self.failure.get_or_insert(message);
        self.latest = None;
    }
    fn invalidate(&mut self) {
        self.active = false;
        self.latest = None;
    }
}

#[derive(Clone)]
struct CaptureFlags {
    generation: u64,
    slot: Arc<Mutex<LatestSlot>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}
impl CaptureFlags {
    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire) || self.cancel.load(Ordering::Acquire)
    }
    fn fail(&self, message: String) {
        self.slot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail(message);
    }
}

struct ContinuousCapture(CaptureFlags);
impl GraphicsCaptureApiHandler for ContinuousCapture {
    type Flags = CaptureFlags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> std::result::Result<Self, Self::Error> {
        Ok(Self(ctx.flags))
    }
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> std::result::Result<(), Self::Error> {
        if self.0.stopping() {
            control.stop();
            return Ok(());
        }
        let copied = (|| -> std::result::Result<RgbaFrame, CaptureError> {
            let width = frame.width();
            let height = frame.height();
            let bytes = rgba_bytes(width, height)?;
            let mut buffer = frame.buffer()?;
            let data = buffer.as_nopadding_buffer()?;
            if data.len() != bytes {
                return Err("WGC returned an invalid RGBA buffer length".into());
            }
            Ok(RgbaFrame {
                width,
                height,
                data: data.to_vec(),
            })
        })();
        match copied {
            Ok(frame) if !self.0.stopping() => {
                self.0
                    .slot
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .publish(self.0.generation, frame);
                Ok(())
            }
            Ok(_) => {
                control.stop();
                Ok(())
            }
            Err(error) => {
                self.0.fail(error.to_string());
                control.stop();
                Err(error)
            }
        }
    }
    fn on_closed(&mut self) -> std::result::Result<(), Self::Error> {
        self.0.fail("WGC capture item closed".into());
        self.0.stop.store(true, Ordering::Release);
        Ok(())
    }
}

fn rgba_bytes(width: u32, height: u32) -> std::result::Result<usize, CaptureError> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_PIXELS {
        return Err("Capture must contain 1 to 16 million pixels".into());
    }
    Ok((pixels * 4) as usize)
}

enum SupervisorEvent {
    Ready,
    // Only Ok confirms that CaptureControl::stop returned successfully.
    Done(std::result::Result<(), String>),
}

struct Session {
    target: Window,
    flags: CaptureFlags,
    events: mpsc::Receiver<SupervisorEvent>,
    ready: bool,
    done: Option<std::result::Result<(), String>>,
}
impl Session {
    fn poll(&mut self) {
        while self.done.is_none() {
            match self.events.try_recv() {
                Ok(SupervisorEvent::Ready) => self.ready = true,
                Ok(SupervisorEvent::Done(result)) => self.done = Some(result),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.done = Some(Err(
                        "Capture supervisor disconnected without cleanup confirmation".into(),
                    ));
                }
            }
        }
    }
    fn invalidate(&self) {
        self.flags.stop.store(true, Ordering::Release);
        self.flags
            .slot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .invalidate();
    }
}

// This thread is the sole owner of the library control. Both start_free_threaded
// (its internal recv/join) and stop (PostThreadMessage loop/join) can hang in 1.5.0.
// Never execute either on the helper owner thread. A deadline expiring does NOT
// mean this thread or WGC stopped: the manager is permanently fenced in that case.
fn supervise(raw_hwnd: usize, flags: CaptureFlags, events: mpsc::Sender<SupervisorEvent>) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let item = windows_capture::window::Window::from_raw_hwnd(raw_hwnd as *mut _);
        let settings = Settings::new(
            item,
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::WithBorder,
            SecondaryWindowSettings::Exclude,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            flags.clone(),
        );
        let control =
            ContinuousCapture::start_free_threaded(settings).map_err(|e| e.to_string())?;
        let owner_gone = events.send(SupervisorEvent::Ready).is_err();
        while !owner_gone && !flags.stopping() && !control.is_finished() {
            if flags
                .slot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .failure
                .is_some()
            {
                break;
            }
            thread::sleep(POLL_INTERVAL);
        }
        // Even a naturally finished stream must be joined and its result checked.
        control.stop().map_err(|e| e.to_string())
    }))
    .unwrap_or_else(|_| Err("Capture supervisor panicked; WGC cleanup is unconfirmed".into()));
    let _ = events.send(SupervisorEvent::Done(outcome));
}

fn same_target(left: &Window, right: &Window) -> bool {
    left.id == right.id
        && left.pid == right.pid
        && left.process_identity == right.process_identity
        && left.bounds == right.bounds
}

/// Owned by the parent helper, not by a per-request screenshot worker.
/// Call maintain on the helper's idle tick (including metadata-only observations),
/// and close on stop/shutdown. Errors with code cleanup_unconfirmed require helper
/// process termination/replacement; dropping a Rust thread cannot revoke WGC.
pub struct CaptureManager {
    cancel: Arc<AtomicBool>,
    session: Option<Session>,
    generation: u64,
    // Permanent fence: never start a second stream after uncertain cleanup.
    cleanup_unconfirmed: Option<String>,
}
impl CaptureManager {
    pub fn new(cancel: Arc<AtomicBool>) -> Self {
        Self {
            cancel,
            session: None,
            generation: 0,
            cleanup_unconfirmed: None,
        }
    }

    fn check_fence(&self) -> cu_protocol::Result<()> {
        match &self.cleanup_unconfirmed {
            Some(message) => Err(cu_protocol::Error::new(
                "cleanup_unconfirmed",
                message.clone(),
            )),
            None => Ok(()),
        }
    }

    pub fn capture(&mut self, window: &Window) -> cu_protocol::Result<Image> {
        self.check_fence()?;
        let result = self.capture_inner(window);
        match result {
            Ok(image) => Ok(image),
            Err(error) => {
                // Cleanup errors take priority: the caller must not assume that a
                // cancelled/failed screenshot revoked the visible sharing session.
                self.close().map_err(|cleanup| {
                    cu_protocol::Error::new(
                        "cleanup_unconfirmed",
                        format!(
                            "{}; original capture error: {}",
                            cleanup.message, error.message
                        ),
                    )
                })?;
                Err(error)
            }
        }
    }

    fn capture_inner(&mut self, window: &Window) -> cu_protocol::Result<Image> {
        cancelled(&self.cancel)?;
        WindowsBackend.validate(window)?;
        if self
            .session
            .as_ref()
            .is_some_and(|s| !same_target(&s.target, window))
        {
            // Invalidate old generation before waiting. No new session until Done(Ok).
            self.close()?;
        }
        if self.session.is_none() {
            self.generation = self
                .generation
                .checked_add(1)
                .ok_or_else(|| err("Capture generation exhausted"))?;
            let flags = CaptureFlags {
                generation: self.generation,
                slot: Arc::new(Mutex::new(LatestSlot::new(self.generation))),
                stop: Arc::new(AtomicBool::new(false)),
                cancel: self.cancel.clone(),
            };
            let (tx, rx) = mpsc::channel();
            let raw_hwnd = hwnd(window)?.0 as usize;
            let supervisor_flags = flags.clone();
            thread::Builder::new()
                .name("wgc-supervisor".into())
                .spawn(move || supervise(raw_hwnd, supervisor_flags, tx))
                .map_err(err)?;
            self.session = Some(Session {
                target: window.clone(),
                flags,
                events: rx,
                ready: false,
                done: None,
            });
        }
        let startup_deadline = Instant::now() + START_TIMEOUT;
        let mut frame_deadline = None;
        let frame = loop {
            cancelled(&self.cancel)?;
            self.check_session()?;
            let session = self.session.as_ref().unwrap();
            if session.ready {
                let deadline =
                    *frame_deadline.get_or_insert_with(|| Instant::now() + FRAME_TIMEOUT);
                let latest = session
                    .flags
                    .slot
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .latest
                    .clone();
                if let Some(frame) = latest {
                    break frame;
                }
                if Instant::now() >= deadline {
                    return Err(cu_protocol::Error::new(
                        "capture_timeout",
                        "WGC first frame deadline exceeded",
                    ));
                }
            } else if Instant::now() >= startup_deadline {
                return Err(cu_protocol::Error::new(
                    "capture_timeout",
                    "WGC startup deadline exceeded",
                ));
            }
            thread::sleep(POLL_INTERVAL);
        };
        if (frame.width as f64 - window.bounds.width).abs() > 2.0
            || (frame.height as f64 - window.bounds.height).abs() > 2.0
        {
            return Err(cu_protocol::Error::new(
                "capture_mapping_changed",
                "WGC frame dimensions do not match the current window bounds",
            ));
        }
        let data = png_base64(frame.width, frame.height, &frame.data)?;
        cancelled(&self.cancel)?;
        self.check_session()?;
        WindowsBackend.validate(window)?;
        Ok(Image {
            id: new_id(),
            mime_type: "image/png".into(),
            data,
            width: frame.width,
            height: frame.height,
            screen_bounds: window.bounds.clone(),
            capture_method: "windows-graphics-capture".into(),
        })
    }

    fn check_session(&mut self) -> cu_protocol::Result<()> {
        if let Some(session) = self.session.as_mut() {
            session.poll();
            if let Some(result) = &session.done {
                return Err(err(match result {
                    Ok(()) => "WGC stream ended unexpectedly".into(),
                    Err(message) => message.clone(),
                }));
            }
            if let Some(message) = &session
                .flags
                .slot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .failure
            {
                return Err(err(message));
            }
        }
        Ok(())
    }

    /// Poll even when no screenshot is requested; this never starts a stream.
    pub fn maintain(&mut self) -> cu_protocol::Result<()> {
        self.check_fence()?;
        let result = cancelled(&self.cancel)
            .and_then(|_| self.check_session())
            .and_then(|_| match &self.session {
                Some(session) => WindowsBackend.validate(&session.target),
                None => Ok(()),
            });
        if result.is_err() {
            self.close()?;
        }
        result
    }

    /// Bounded wait for actual stop completion, independent of the cancel flag.
    pub fn close(&mut self) -> cu_protocol::Result<()> {
        self.close_until(Instant::now() + CLOSE_TIMEOUT)
    }
    fn close_until(&mut self, deadline: Instant) -> cu_protocol::Result<()> {
        if let Some(session) = self.session.as_mut() {
            session.invalidate();
            // A timed-out operation has already spent the cleanup budget. Poll
            // late completion, but never spend another budget in Drop/retries.
            if self.cleanup_unconfirmed.is_some() {
                session.poll();
                if matches!(session.done, Some(Ok(()))) {
                    self.session = None;
                }
                return self.check_fence();
            }
            loop {
                session.poll();
                match &session.done {
                    Some(Ok(())) => {
                        self.session = None;
                        break;
                    }
                    Some(Err(message)) => {
                        self.cleanup_unconfirmed
                            .get_or_insert_with(|| format!("WGC cleanup_unconfirmed: {message}"));
                        break;
                    }
                    None if Instant::now() >= deadline => {
                        self.cleanup_unconfirmed.get_or_insert_with(||
                            "WGC cleanup_unconfirmed: supervisor did not confirm shutdown before deadline; replace helper process".into());
                        break;
                    }
                    None => thread::sleep(POLL_INTERVAL),
                }
            }
        }
        self.check_fence()
    }
}
impl Drop for CaptureManager {
    fn drop(&mut self) {
        // Best effort only. Explicit close is required so the helper can report
        // cleanup_unconfirmed rather than treating a detached supervisor as stopped.
        let _ = self.close();
    }
}

// Compatibility for Backend/worker callers. Production should own one manager
// in the parent helper; this wrapper intentionally closes before returning.
pub(super) fn capture(window: &Window) -> cu_protocol::Result<Image> {
    let mut manager = CaptureManager::new(Arc::new(AtomicBool::new(false)));
    let result = manager.capture(window);
    manager.close()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Window {
        Window {
            id: "1".into(),
            pid: 2,
            process_identity: "2:created".into(),
            title: "title".into(),
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        }
    }
    fn frame(value: u8) -> RgbaFrame {
        RgbaFrame {
            width: 1,
            height: 1,
            data: vec![value; 4],
        }
    }
    fn fake_manager() -> (
        CaptureManager,
        mpsc::Sender<SupervisorEvent>,
        Arc<Mutex<LatestSlot>>,
    ) {
        let cancel = Arc::new(AtomicBool::new(false));
        let slot = Arc::new(Mutex::new(LatestSlot::new(1)));
        let (tx, rx) = mpsc::channel();
        let mut manager = CaptureManager::new(cancel.clone());
        manager.session = Some(Session {
            target: window(),
            flags: CaptureFlags {
                generation: 1,
                slot: slot.clone(),
                stop: Arc::new(AtomicBool::new(false)),
                cancel,
            },
            events: rx,
            ready: false,
            done: None,
        });
        (manager, tx, slot)
    }
    #[test]
    fn latest_is_bounded_and_generation_fenced() {
        let mut slot = LatestSlot::new(7);
        for value in 0..100 {
            slot.publish(7, frame(value));
        }
        assert_eq!(slot.latest.as_ref().unwrap().data[0], 99);
        slot.publish(6, frame(1));
        assert_eq!(slot.latest.as_ref().unwrap().data[0], 99);
        slot.invalidate();
        slot.publish(7, frame(2));
        assert!(slot.latest.is_none());
    }
    #[test]
    fn failure_removes_frame_and_prevents_late_publish() {
        let mut slot = LatestSlot::new(1);
        slot.publish(1, frame(1));
        slot.fail("closed".into());
        slot.publish(1, frame(2));
        assert!(slot.latest.is_none());
    }
    #[test]
    fn metadata_only_keeps_target_but_identity_or_geometry_does_not() {
        let original = window();
        let mut updated = original.clone();
        updated.title = "new title".into();
        assert!(same_target(&original, &updated));
        updated.bounds.x = 1.0;
        assert!(!same_target(&original, &updated));
        updated = original.clone();
        updated.process_identity = "reused pid".into();
        assert!(!same_target(&original, &updated));
    }
    #[test]
    fn close_requires_successful_completion_not_ready() {
        let (mut manager, tx, slot) = fake_manager();
        tx.send(SupervisorEvent::Ready).unwrap();
        let error = manager.close_until(Instant::now()).unwrap_err();
        assert_eq!(error.code, "cleanup_unconfirmed");
        assert!(!slot.lock().unwrap().active);
        assert!(manager.session.is_some());
        // Late confirmation can retire resources but must never remove the fence.
        tx.send(SupervisorEvent::Done(Ok(()))).unwrap();
        assert_eq!(manager.close().unwrap_err().code, "cleanup_unconfirmed");
        assert!(manager.session.is_none());
        assert!(manager.check_fence().is_err());
    }
    #[test]
    fn successful_close_is_idempotent_and_revokes_slot() {
        let (mut manager, tx, slot) = fake_manager();
        slot.lock().unwrap().publish(1, frame(1));
        tx.send(SupervisorEvent::Done(Ok(()))).unwrap();
        manager.close().unwrap();
        manager.close().unwrap();
        assert!(manager.session.is_none());
        assert!(slot.lock().unwrap().latest.is_none());
    }
    #[test]
    fn supervisor_failure_or_disconnect_is_not_cleanup_confirmation() {
        for explicit_error in [false, true] {
            let (mut manager, tx, _) = fake_manager();
            if explicit_error {
                tx.send(SupervisorEvent::Done(Err("stop failed".into())))
                    .unwrap();
            }
            drop(tx);
            assert_eq!(manager.close().unwrap_err().code, "cleanup_unconfirmed");
            assert!(manager.check_fence().is_err());
        }
    }
    #[test]
    fn cancellation_maintain_closes_without_touching_windows() {
        let (mut manager, tx, _) = fake_manager();
        manager.cancel.store(true, Ordering::Release);
        tx.send(SupervisorEvent::Done(Ok(()))).unwrap();
        assert!(manager.maintain().is_err());
        assert!(manager.session.is_none());
    }
    #[test]
    fn rgba_allocation_is_bounded() {
        assert_eq!(rgba_bytes(4000, 4000).unwrap(), 64_000_000);
        assert!(rgba_bytes(4001, 4000).is_err());
        assert!(rgba_bytes(0, 1).is_err());
        assert!(rgba_bytes(u32::MAX, u32::MAX).is_err());
    }
}
