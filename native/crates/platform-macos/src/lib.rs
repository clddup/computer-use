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
            return Err(Error::new(
                error["code"].as_str().unwrap_or("os_error"),
                error["message"]
                    .as_str()
                    .unwrap_or("macOS operation failed"),
            ));
        }
        serde_json::from_value(value["result"].clone())
            .map_err(|e| Error::new("bridge_failed", e.to_string()))
    }
    fn query<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
        let input = CString::new(value.to_string())
            .map_err(|e| Error::new("invalid_params", e.to_string()))?;
        unsafe { decode(cu_macos_query(input.as_ptr())) }
    }
    pub fn permissions(request: bool) -> Result<serde_json::Value> {
        query(json!({"method":"permissions","request":request}))
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
            self.prepare(window)?;
            cu_core::cancelled(cancel)
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
}
#[cfg(target_os = "macos")]
pub use implementation::{permissions, MacosBackend};
