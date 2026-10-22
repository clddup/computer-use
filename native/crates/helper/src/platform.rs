use cu_core::Backend;

#[cfg(windows)]
pub(crate) fn backend() -> impl Backend {
    cu_platform_windows::WindowsBackend
}
#[cfg(target_os = "macos")]
pub(crate) fn backend() -> impl Backend {
    cu_platform_macos::MacosBackend
}
#[cfg(target_os = "linux")]
pub(crate) fn backend() -> impl Backend {
    cu_platform_linux::LinuxBackend
}
