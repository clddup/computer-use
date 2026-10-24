#[cfg(windows)]
mod implementation;
#[cfg(windows)]
pub use implementation::{
    close_presentation, is_presentation_window, maintain_presentation, present, CaptureManager,
    WindowsBackend,
};
