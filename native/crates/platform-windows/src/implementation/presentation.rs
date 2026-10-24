//! A process-local mailbox owned by the long-lived parent helper, not probe children.
use super::*;
use std::sync::{Mutex, OnceLock};

static SERVICE: OnceLock<Mutex<Option<super::overlay::Service>>> = OnceLock::new();
static FENCE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn fence_error() -> cu_protocol::Result<()> {
    let fence = FENCE.get_or_init(|| Mutex::new(None)).lock().map_err(|_| {
        cu_protocol::Error::new("cleanup_unconfirmed", "Presentation fence poisoned")
    })?;
    if let Some(message) = fence.as_ref() {
        return Err(cu_protocol::Error::new(
            "cleanup_unconfirmed",
            message.clone(),
        ));
    }
    Ok(())
}

fn record_fence(error: &cu_protocol::Error) {
    if let Ok(mut fence) = FENCE.get_or_init(|| Mutex::new(None)).lock() {
        fence.get_or_insert_with(|| error.message.clone());
    }
}

fn with_service<T>(
    start: bool,
    f: impl FnOnce(Option<&super::overlay::Service>) -> cu_protocol::Result<T>,
) -> cu_protocol::Result<T> {
    if start {
        fence_error()?;
    }
    let mut service = SERVICE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| {
            cu_protocol::Error::new("presentation_failed", "Presentation mailbox poisoned")
        })?;
    if start && service.is_none() {
        *service = Some(super::overlay::Service::start()?);
    }
    f(service.as_ref())
}

/// Same-target metadata observations preserve the last pointer location. A different
/// identity (including geometry) synchronously destroys the previous window first.
pub fn present(window: &Window) -> cu_protocol::Result<()> {
    with_service(true, |service| service.unwrap().bind(window.clone()))
}

/// Called only after SetCursorPos succeeds. No service is started by input alone.
pub(super) fn update_pointer(handle: HWND, x: i32, y: i32) -> cu_protocol::Result<()> {
    with_service(false, |service| match service {
        Some(service) => service.move_pointer(handle.0 as usize, x, y),
        None => Ok(()),
    })
}

/// Hide with acknowledgement before WindowFromPoint. HTTRANSPARENT alone is not
/// sufficient to guarantee cross-thread hit testing; never skip an arbitrary HWND.
pub(super) fn suspend_pointer() -> cu_protocol::Result<()> {
    with_service(false, |service| match service {
        Some(service) => service.suspend(),
        None => Ok(()),
    })
}

pub fn close_presentation() -> cu_protocol::Result<()> {
    let mut service = SERVICE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| {
            cu_protocol::Error::new("cleanup_unconfirmed", "Presentation mailbox poisoned")
        })?;
    let result = service
        .as_ref()
        .map(|current| current.close())
        .unwrap_or(Ok(()));
    // Do not retain a Service after a timeout or a pump-side failure. Its
    // sender may refer to a dead or already-shutting-down GUI thread, and a
    // later helper session must never reuse that state.
    *service = None;
    if let Err(error) = &result {
        record_fence(error);
    }
    result
}

pub fn is_presentation_window(id: &str) -> bool {
    id.parse::<usize>()
        .ok()
        .map(|id| super::overlay::is_overlay_window(HWND(id as *mut _)))
        .unwrap_or(false)
}

pub fn maintain_presentation(cancelled: bool) -> Option<cu_protocol::Error> {
    if cancelled {
        return close_presentation().err();
    }
    with_service(false, |service| match service {
        Some(service) => service.maintain(),
        None => Ok(()),
    })
    .err()
}
