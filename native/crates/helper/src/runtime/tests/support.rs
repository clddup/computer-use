use super::*;
use cu_core::{ApplicationPolicy, NativeAction};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
pub(super) struct State {
    pub windows: Vec<Window>,
    pub apps: Vec<Application>,
    pub launch_windows: Vec<Window>,
    pub launch_error: Option<Error>,
    pub launch_cancel: bool,
    pub activation_error: Option<Error>,
    pub activation_cancel: bool,
    pub activation_window: Option<Window>,
    pub perform_error: Option<Error>,
    pub preflight_error: Option<Error>,
    pub elements: Vec<Element>,
    pub launches: usize,
    pub activations: usize,
    pub inputs: usize,
    pub cleanups: usize,
    pub cleanup_error: Option<Error>,
}

pub(super) struct FakeBackend {
    pub state: Rc<RefCell<State>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for FakeBackend {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl Backend for FakeBackend {
    fn end_session(&self) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.cleanups += 1;
        state.cleanup_error.clone().map_or(Ok(()), Err)
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            protocol_version: MAX_VERSION,
            platform: "fixture".into(),
            backend: "fake".into(),
            available: true,
            reason: None,
            actions: vec!["click".into(), "invoke".into()],
            screenshot_methods: vec![],
            limitations: vec![],
        }
    }
    fn list_windows(&self) -> Result<Vec<Window>> {
        Ok(self.state.borrow().windows.clone())
    }
    fn list_apps(&self, apps: &[ApplicationPolicy]) -> Result<Vec<Application>> {
        Ok(self
            .state
            .borrow()
            .apps
            .iter()
            .filter(|a| apps.iter().any(|p| p.id == a.id))
            .cloned()
            .collect())
    }
    fn launch_app(&self, _app: &ApplicationPolicy, cancel: &AtomicBool) -> Result<Vec<Window>> {
        let mut state = self.state.borrow_mut();
        state.launches += 1;
        if state.launch_cancel {
            cancel.store(true, Ordering::SeqCst);
        }
        match &state.launch_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.launch_windows.clone()),
        }
    }
    fn activate_window(&self, _window: &Window, cancel: &AtomicBool) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.activations += 1;
        if state.activation_cancel {
            cancel.store(true, Ordering::SeqCst);
        }
        if let Some(window) = state.activation_window.clone() {
            state.windows = vec![window];
        }
        match &state.activation_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
    fn validate(&self, window: &Window) -> Result<()> {
        if self.state.borrow().windows.iter().any(|w| {
            w.id == window.id
                && w.pid == window.pid
                && w.process_identity == window.process_identity
                && w.bounds == window.bounds
        }) {
            Ok(())
        } else {
            Err(Error::new(
                "stale_window",
                "Fixture window changed or closed",
            ))
        }
    }
    fn observe(
        &self,
        window: &Window,
        _screenshot: bool,
        _accessibility: bool,
    ) -> Result<Observation> {
        self.validate(window)?;
        Ok(Observation {
            id: new_id(),
            helper_epoch: String::new(),
            window: window.clone(),
            captured_at_ms: now_ms(),
            expires_at_ms: now_ms() + 30000,
            elements: self.state.borrow().elements.clone(),
            images: vec![],
            warnings: vec![],
        })
    }
    fn preflight(&self, window: &Window, _action: &NativeAction) -> Result<()> {
        self.validate(window)?;
        self.state
            .borrow()
            .preflight_error
            .clone()
            .map_or(Ok(()), Err)
    }
    fn perform(
        &self,
        _window: &Window,
        _action: &NativeAction,
        _cancel: &AtomicBool,
    ) -> Result<String> {
        let mut state = self.state.borrow_mut();
        state.inputs += 1;
        match &state.perform_error {
            Some(error) => Err(error.clone()),
            None => Ok("fixture".into()),
        }
    }
}

pub(super) type TestRuntime = Runtime<FakeBackend, ()>;

pub(super) fn window() -> Window {
    Window {
        id: "window".into(),
        pid: 1,
        process_identity: "process:1".into(),
        title: "Fixture".into(),
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
    }
}

pub(super) fn policy() -> Policy {
    Policy {
        allowed_pids: vec![1],
        allow_input: true,
        ..Policy::default()
    }
}

pub(super) fn app_policy() -> Policy {
    Policy {
        allowed_apps: vec![ApplicationPolicy {
            id: "fixture".into(),
            display_name: "Fixture".into(),
            executable: "/host-approved/fixture".into(),
            args: vec![],
        }],
        ..Policy::default()
    }
}

pub(super) fn setup(policy: Policy) -> (TestRuntime, String) {
    let cancel = Arc::new(AtomicBool::new(false));
    let state = Rc::new(RefCell::new(State {
        windows: vec![window()],
        ..State::default()
    }));
    let backend = FakeBackend {
        state,
        cancel: cancel.clone(),
    };
    let mut runtime = Runtime::new(policy, backend, cancel, || Ok(()));
    let id = runtime.handle(Command::Start {}, MAX_VERSION).unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    (runtime, id)
}

pub(super) fn list(runtime: &mut TestRuntime, id: &str) {
    runtime
        .handle(
            Command::ListWindows {
                session_id: id.into(),
            },
            MAX_VERSION,
        )
        .unwrap();
}

pub(super) fn observation(runtime: &mut TestRuntime, id: &str) -> String {
    list(runtime, id);
    runtime
        .handle(
            Command::Observe {
                session_id: id.into(),
                window: window(),
                screenshot: false,
                accessibility: false,
            },
            MAX_VERSION,
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}

pub(super) fn act(
    runtime: &mut TestRuntime,
    id: &str,
    observation_id: &str,
    action_id: &str,
) -> Value {
    runtime
        .handle(
            Command::Act {
                session_id: id.into(),
                action_id: action_id.into(),
                observation_id: observation_id.into(),
                action: Action::TypeText {
                    text: "fixture".into(),
                },
            },
            MAX_VERSION,
        )
        .unwrap()
}
