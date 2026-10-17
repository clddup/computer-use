#[cfg(target_os = "linux")]
mod implementation;
#[cfg(target_os = "linux")]
pub use implementation::LinuxBackend;
