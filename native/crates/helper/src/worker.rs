use crate::platform::backend;
use cu_core::{cancelled, Backend, NativeAction};
use cu_protocol::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{self, Read},
    sync::atomic::AtomicBool,
};

#[derive(Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub(crate) enum Probe {
    List,
    Observe {
        window: Window,
        screenshot: bool,
        accessibility: bool,
    },
    Validate {
        window: Window,
    },
    Prepare {
        window: Window,
    },
    Semantic {
        window: Window,
        action: Box<cu_core::NativeAction>,
        execute: bool,
    },
    Activate {
        window: Window,
    },
}

impl Probe {
    pub(crate) fn may_execute(&self) -> bool {
        matches!(
            self,
            Self::Activate { .. } | Self::Prepare { .. } | Self::Semantic { execute: true, .. }
        )
    }
}

pub(crate) fn probe_main() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let mut bytes = vec![];
    io::stdin()
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::new("invalid_params", e.to_string()))?;
    let result = serde_json::from_slice::<Probe>(&bytes)
        .map_err(|e| Error::new("invalid_params", e.to_string()))
        .and_then(|probe| execute(probe, &backend(), &cancel));
    serde_json::to_writer(io::stdout(), &result)
        .map_err(|e| Error::new("worker_failed", e.to_string()))?;
    Ok(())
}

pub(crate) fn execute(probe: Probe, backend: &impl Backend, cancel: &AtomicBool) -> Result<Value> {
    match probe {
        Probe::List => backend.list_windows().map(|v| json!(v)),
        Probe::Observe {
            window,
            screenshot,
            accessibility,
        } => backend
            .observe(&window, screenshot, accessibility)
            .map(|v| json!(v)),
        Probe::Validate { window } => backend.validate(&window).map(|_| json!({"valid":true})),
        Probe::Prepare { window } => backend.prepare(&window).map(|_| json!({"prepared":true})),
        Probe::Semantic {
            window,
            action,
            execute,
        } => semantic(backend, &window, &action, execute, cancel),
        Probe::Activate { window } => activate(backend, &window, cancel),
    }
}

fn semantic(
    backend: &impl Backend,
    window: &Window,
    action: &NativeAction,
    execute: bool,
    cancel: &AtomicBool,
) -> Result<Value> {
    backend.preflight(window, action)?;
    cancelled(cancel)?;
    if !execute {
        return Ok(json!({"prepared":true}));
    }
    let mechanism = backend
        .perform(window, action, cancel)
        .map_err(Error::after_dispatch)?;
    cancelled(cancel).map_err(Error::after_dispatch)?;
    Ok(json!(mechanism))
}

fn activate(backend: &impl Backend, window: &Window, cancel: &AtomicBool) -> Result<Value> {
    backend.validate(window)?;
    cancelled(cancel)?;
    backend.activate_window(window, cancel)?;
    backend.validate(window).map_err(Error::after_dispatch)?;
    cancelled(cancel).map_err(Error::after_dispatch)?;
    Ok(json!({"activated":true}))
}
