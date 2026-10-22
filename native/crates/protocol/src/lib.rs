use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const MAX_VERSION: u32 = 2;
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub id: String,
    pub command: Command,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Command {
    Capabilities {},
    Start {},
    ListApps {
        session_id: String,
    },
    LaunchApp {
        session_id: String,
        app_id: String,
    },
    ListWindows {
        session_id: String,
    },
    GetWindow {
        session_id: String,
        window: Window,
    },
    ActivateWindow {
        session_id: String,
        window: Window,
    },
    Observe {
        session_id: String,
        window: Window,
        screenshot: bool,
        accessibility: bool,
    },
    Act {
        session_id: String,
        action_id: String,
        observation_id: String,
        action: Action,
    },
    ActionStatus {
        session_id: String,
        action_id: String,
    },
    Stop {
        session_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub id: String,
    pub pid: u32,
    pub process_identity: String,
    pub title: String,
    pub bounds: Rect,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub id: String,
    pub display_name: String,
    pub is_running: bool,
    pub windows: Vec<Window>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LaunchResult {
    pub app_id: String,
    pub state: String,
    pub windows: Vec<Window>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Element {
    pub id: String,
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub bounds: Option<Rect>,
    pub editable: bool,
    pub password: bool,
    pub depth: u32,
    pub locator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub id: String,
    pub mime_type: String,
    pub data: String,
    pub width: u32,
    pub height: u32,
    pub screen_bounds: Rect,
    pub capture_method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub id: String,
    pub helper_epoch: String,
    pub window: Window,
    pub captured_at_ms: u64,
    pub expires_at_ms: u64,
    pub elements: Vec<Element>,
    pub images: Vec<Image>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Element { element_id: String },
    Image { image_id: String, x: f64, y: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Invoke {
        element_id: String,
    },
    Toggle {
        element_id: String,
    },
    Select {
        element_id: String,
    },
    Click {
        target: Target,
        button: Button,
        #[serde(default = "default_click_count")]
        click_count: u32,
    },
    TypeText {
        text: String,
    },
    Key {
        keys: Vec<String>,
    },
    Scroll {
        target: Target,
        dx: i32,
        dy: i32,
    },
    Drag {
        from: Target,
        to: Target,
        duration_ms: u32,
    },
    SetValue {
        element_id: String,
        value: String,
    },
}

fn default_click_count() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Rejected,
    Dispatched,
    Verified,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionOutcome {
    pub action_id: String,
    pub helper_epoch: String,
    pub state: ActionState,
    pub mechanism: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub protocol_version: u32,
    pub platform: String,
    pub backend: String,
    pub available: bool,
    pub reason: Option<String>,
    pub actions: Vec<String>,
    pub screenshot_methods: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Error {
    pub code: String,
    pub message: String,
    pub may_have_executed: bool,
}
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            may_have_executed: false,
        }
    }
    /// Marks failure after an operation may have crossed its side-effect boundary.
    pub fn after_dispatch(mut self) -> Self {
        self.may_have_executed = true;
        self
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<Error>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ProtocolTypes {
    pub request: Request,
    pub response: Response,
    pub window: Window,
    pub application: Application,
    pub launch_result: LaunchResult,
    pub observation: Observation,
    pub action: Action,
    pub action_outcome: ActionOutcome,
    pub capabilities: Capabilities,
}

pub type Result<T> = std::result::Result<T, Error>;
