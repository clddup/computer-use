#[cfg(target_os = "macos")]
mod implementation {
    use cu_core::{Backend, NativeAction};
    use cu_protocol::*;
    use serde_json::json;
    use std::{
        ffi::{c_char, c_void, CStr, CString},
        sync::atomic::{AtomicBool, Ordering},
    };
    pub struct MacosBackend;
    extern "C" {
        fn cu_macos_query(input: *const c_char) -> *mut c_char;
        fn cu_macos_perform(
            input: *const c_char,
            context: *const c_void,
            cancel: extern "C" fn(*const c_void) -> i32,
        ) -> *mut c_char;
        fn cu_macos_free(value: *mut c_char);
        fn cu_macos_capture(
            input: *const c_char,
            context: *const c_void,
            cancel: extern "C" fn(*const c_void) -> i32,
        ) -> *mut c_char;
        fn cu_macos_maintain(cancelled: i32) -> i32;
        fn cu_macos_is_presentation_window(id: u32) -> i32;
    }
    extern "C" fn is_cancelled(context: *const c_void) -> i32 {
        unsafe { (*(context as *const AtomicBool)).load(Ordering::SeqCst) as i32 }
    }
    unsafe fn decode<T: serde::de::DeserializeOwned>(pointer: *mut c_char) -> Result<T> {
        if pointer.is_null() {
            return Err(Error::new(
                "bridge_failed",
                "macOS bridge returned no result",
            ));
        }
        let bytes = CStr::from_ptr(pointer).to_bytes().to_vec();
        cu_macos_free(pointer);
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| Error::new("bridge_failed", e.to_string()))?;
        if let Some(error) = value.get("error") {
            return Err(bridge_error(error));
        }
        serde_json::from_value(value["result"].clone())
            .map_err(|e| Error::new("bridge_failed", e.to_string()))
    }
    fn bridge_error(value: &serde_json::Value) -> Error {
        let mut error = Error::new(
            value["code"].as_str().unwrap_or("os_error"),
            value["message"]
                .as_str()
                .unwrap_or("macOS operation failed"),
        );
        error.may_have_executed = value["may_have_executed"].as_bool().unwrap_or(false);
        error
    }
    fn query<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
        let input = CString::new(value.to_string())
            .map_err(|e| Error::new("invalid_params", e.to_string()))?;
        unsafe { decode(cu_macos_query(input.as_ptr())) }
    }
    pub fn permissions(accessibility: bool, screen_recording: bool) -> Result<serde_json::Value> {
        query(
            json!({"method":"permissions","request_accessibility":accessibility,"request_screen_recording":screen_recording}),
        )
    }
    pub fn present(window: &Window, screenshot: bool) -> Result<()> {
        let _: bool = query(json!({"method":"present","window":window,"screenshot":screenshot}))?;
        Ok(())
    }
    pub fn capture(window: &Window, cancel: &AtomicBool) -> Result<Image> {
        let input = CString::new(
            serde_json::to_string(window)
                .map_err(|error| Error::new("invalid_window", error.to_string()))?,
        )
        .map_err(|error| Error::new("invalid_window", error.to_string()))?;
        unsafe {
            decode(cu_macos_capture(
                input.as_ptr(),
                cancel as *const _ as *const c_void,
                is_cancelled,
            ))
        }
    }
    pub fn maintain(cancelled: bool) -> Option<Error> {
        if unsafe { cu_macos_maintain(cancelled as i32) } == 0 {
            return None;
        }
        Some(match query(json!({"method":"presentation_error"})) {
            Ok(value) => bridge_error(&value),
            Err(error) => error,
        })
    }
    pub fn is_presentation_window(id: &str) -> bool {
        id.parse()
            .is_ok_and(|id| unsafe { cu_macos_is_presentation_window(id) != 0 })
    }
    pub fn close_presentation() -> Result<()> {
        let _: bool = query(json!({"method":"close_presentation"}))?;
        Ok(())
    }
    impl Backend for MacosBackend {
        fn capabilities(&self) -> Capabilities {
            query(json!({"method":"capabilities"})).unwrap_or_else(|e| Capabilities {
                protocol_version: VERSION,
                platform: "macos".into(),
                backend: "ax-screencapturekit-coregraphics".into(),
                available: false,
                reason: Some(e.to_string()),
                actions: vec![],
                screenshot_methods: vec![],
                limitations: vec![],
            })
        }
        fn list_windows(&self) -> Result<Vec<Window>> {
            query(json!({"method":"list"}))
        }
        fn list_apps(&self, apps: &[cu_core::ApplicationPolicy]) -> Result<Vec<Application>> {
            query(json!({"method":"list_apps","apps":apps}))
        }
        fn launch_app(
            &self,
            app: &cu_core::ApplicationPolicy,
            cancel: &AtomicBool,
        ) -> Result<Vec<Window>> {
            cu_core::cancelled(cancel)?;
            let windows = query(json!({"method":"launch_app","app":app}))?;
            cu_core::cancelled(cancel).map_err(Error::after_dispatch)?;
            Ok(windows)
        }
        fn validate(&self, window: &Window) -> Result<()> {
            let _: bool = query(json!({"method":"validate","window":window}))?;
            Ok(())
        }
        fn prepare(&self, window: &Window) -> Result<()> {
            let _: bool = query(json!({"method":"prepare","window":window}))?;
            Ok(())
        }
        fn activate_window(&self, window: &Window, cancel: &AtomicBool) -> Result<()> {
            cu_core::cancelled(cancel)?;
            self.prepare(window).map_err(|error| {
                if error.code == "bridge_failed" {
                    error.after_dispatch()
                } else {
                    error
                }
            })?;
            cu_core::cancelled(cancel).map_err(Error::after_dispatch)
        }
        fn observe(
            &self,
            window: &Window,
            screenshot: bool,
            accessibility: bool,
        ) -> Result<Observation> {
            query(
                json!({"method":"observe","window":window,"screenshot":screenshot,"accessibility":accessibility}),
            )
        }
        fn perform(
            &self,
            window: &Window,
            action: &NativeAction,
            cancel: &AtomicBool,
        ) -> Result<String> {
            if matches!(action, NativeAction::Semantic { .. }) {
                return Err(Error::new(
                    "unsupported_action",
                    "UIA semantic actions are Windows-only",
                ));
            }
            let input = CString::new(json!({"window":window,"action":action}).to_string())
                .map_err(|e| Error::new("invalid_params", e.to_string()))?;
            unsafe {
                decode(cu_macos_perform(
                    input.as_ptr(),
                    cancel as *const _ as *const c_void,
                    is_cancelled,
                ))
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn preserves_bridge_dispatch_uncertainty() {
            let error = bridge_error(
                &json!({"code":"focus_failed", "message":"Activation began", "may_have_executed":true}),
            );
            assert!(error.may_have_executed);
            assert_eq!(error.code, "focus_failed");
        }
        #[test]
        fn old_bridge_errors_default_to_no_dispatch() {
            assert!(
                !bridge_error(&json!({"code":"invalid_window", "message":"Before activation"}))
                    .may_have_executed
            );
        }
    }
}
#[cfg(target_os = "macos")]
pub use implementation::{
    capture, close_presentation, is_presentation_window, maintain, permissions, present,
    MacosBackend,
};
