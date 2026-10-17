#[cfg(windows)]
mod implementation;
#[cfg(windows)]
pub use implementation::WindowsBackend;
