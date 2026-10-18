// Generated from the Rust wire schema. Run protocol:generate; do not edit.

export type Action =
  | {
      element_id: string;
      kind: "invoke";
    }
  | {
      element_id: string;
      kind: "toggle";
    }
  | {
      element_id: string;
      kind: "select";
    }
  | {
      button: Button;
      click_count?: number;
      kind: "click";
      target: Target;
    }
  | {
      kind: "type_text";
      text: string;
    }
  | {
      keys: string[];
      kind: "key";
    }
  | {
      dx: number;
      dy: number;
      kind: "scroll";
      target: Target;
    }
  | {
      duration_ms: number;
      from: Target;
      kind: "drag";
      to: Target;
    }
  | {
      element_id: string;
      kind: "set_value";
      value: string;
    };
export type Button = "left" | "right" | "middle";
export type Target =
  | {
      element_id: string;
      kind: "element";
    }
  | {
      image_id: string;
      kind: "image";
      x: number;
      y: number;
    };
export type ActionState = "rejected" | "dispatched" | "verified" | "unknown";
export type Command =
  | {
      method: "capabilities";
      params: {};
    }
  | {
      method: "start";
      params: {};
    }
  | {
      method: "list_apps";
      params: {
        session_id: string;
      };
    }
  | {
      method: "launch_app";
      params: {
        app_id: string;
        session_id: string;
      };
    }
  | {
      method: "list_windows";
      params: {
        session_id: string;
      };
    }
  | {
      method: "get_window";
      params: {
        session_id: string;
        window: Window;
      };
    }
  | {
      method: "activate_window";
      params: {
        session_id: string;
        window: Window;
      };
    }
  | {
      method: "observe";
      params: {
        accessibility: boolean;
        screenshot: boolean;
        session_id: string;
        window: Window;
      };
    }
  | {
      method: "act";
      params: {
        action: Action;
        action_id: string;
        observation_id: string;
        session_id: string;
      };
    }
  | {
      method: "action_status";
      params: {
        action_id: string;
        session_id: string;
      };
    }
  | {
      method: "stop";
      params: {
        session_id: string;
      };
    };

export interface ProtocolTypes {
  action: Action;
  action_outcome: ActionOutcome;
  application: Application;
  capabilities: Capabilities;
  launch_result: LaunchResult;
  observation: Observation;
  request: Request;
  response: Response;
  window: Window;
}
export interface ActionOutcome {
  action_id: string;
  helper_epoch: string;
  mechanism: string;
  message: string;
  state: ActionState;
}
export interface Application {
  display_name: string;
  id: string;
  is_running: boolean;
  windows: Window[];
}
export interface Window {
  bounds: Rect;
  id: string;
  pid: number;
  process_identity: string;
  title: string;
}
export interface Rect {
  height: number;
  width: number;
  x: number;
  y: number;
}
export interface Capabilities {
  actions: string[];
  available: boolean;
  backend: string;
  limitations: string[];
  platform: string;
  protocol_version: number;
  reason?: string | null;
  screenshot_methods: string[];
}
export interface LaunchResult {
  app_id: string;
  message: string;
  state: string;
  windows: Window[];
}
export interface Observation {
  captured_at_ms: number;
  elements: Element[];
  expires_at_ms: number;
  helper_epoch: string;
  id: string;
  images: Image[];
  warnings: string[];
  window: Window;
}
export interface Element {
  bounds?: Rect | null;
  capabilities?: string[] | null;
  depth: number;
  editable: boolean;
  id: string;
  locator: string;
  name: string;
  password: boolean;
  role: string;
  value?: string | null;
}
export interface Image {
  capture_method: string;
  data: string;
  height: number;
  id: string;
  mime_type: string;
  screen_bounds: Rect;
  width: number;
}
export interface Request {
  command: Command;
  id: string;
  version: number;
}
export interface Response {
  error?: Error | null;
  id: string;
  result?: unknown;
  version: number;
}
export interface Error {
  code: string;
  may_have_executed: boolean;
  message: string;
}
