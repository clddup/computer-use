use super::*;

unsafe fn semantic_capabilities(node: &IUIAutomationElement) -> Vec<String> {
    let mut caps = vec![];
    if !node.CurrentIsEnabled().is_ok_and(|v| v.as_bool())
        || !node.CurrentIsPassword().is_ok_and(|v| !v.as_bool())
        || !node.CurrentIsOffscreen().is_ok_and(|v| !v.as_bool())
    {
        return caps;
    }
    if node
        .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
        .is_ok()
    {
        caps.push("invoke".into());
    }
    if node
        .GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
        .is_ok()
    {
        caps.push("toggle".into());
    }
    if node
        .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
        .is_ok()
    {
        caps.push("select".into());
    }
    if let Ok(pattern) = node.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
        caps.push("read_value".into());
        if pattern.CurrentIsReadOnly().is_ok_and(|v| !v.as_bool()) {
            caps.push("set_value".into());
        }
    }
    caps
}
pub(super) unsafe fn semantic_node(
    window: &Window,
    element: &Element,
    operation: SemanticOperation,
) -> cu_protocol::Result<IUIAutomationElement> {
    let automation: IUIAutomation =
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(err)?;
    let walker = automation.ControlViewWalker().map_err(err)?;
    let mut node = automation.ElementFromHandle(hwnd(window)?).map_err(err)?;
    let mut parts = element.locator.split('/');
    if parts.next() != Some("root") {
        return Err(cu_protocol::Error::new(
            "stale_element",
            "Invalid UIA locator",
        ));
    }
    let mut depth = 0;
    for part in parts {
        depth += 1;
        let index = part.parse::<usize>().map_err(err)?;
        if depth > 16 || index >= 128 {
            return Err(cu_protocol::Error::new(
                "stale_element",
                "UIA locator exceeds tree limits",
            ));
        }
        node = walker.GetFirstChildElement(&node).map_err(err)?;
        for _ in 0..index {
            node = walker.GetNextSiblingElement(&node).map_err(err)?;
        }
        if node.CurrentProcessId().map_err(err)? != window.pid as i32
            || node.CurrentIsPassword().map_err(err)?.as_bool()
        {
            return Err(cu_protocol::Error::new(
                "permission_denied",
                "UIA target is outside the authorized process or is a password",
            ));
        }
    }
    let r = node.CurrentBoundingRectangle().map_err(err)?;
    let bounds = Rect {
        x: r.left as f64,
        y: r.top as f64,
        width: (r.right - r.left) as f64,
        height: (r.bottom - r.top) as f64,
    };
    if node.CurrentProcessId().map_err(err)? != window.pid as i32
        || node.CurrentLocalizedControlType().map_err(err)? != element.role
        || node.CurrentName().map_err(err)? != element.name
        || element.bounds.as_ref() != Some(&bounds)
    {
        return Err(cu_protocol::Error::new(
            "stale_element",
            "UIA target identity or geometry changed",
        ));
    }
    if let Some(expected) = &element.value {
        let pattern = node
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .map_err(err)?;
        let current: String = pattern
            .CurrentValue()
            .map_err(err)?
            .to_string()
            .chars()
            .take(4096)
            .collect();
        if &current != expected {
            return Err(cu_protocol::Error::new(
                "stale_element",
                "UIA value changed before semantic dispatch",
            ));
        }
    }
    if !semantic_capabilities(&node)
        .iter()
        .any(|c| c == operation.capability())
    {
        return Err(cu_protocol::Error::new(
            "unsupported_action",
            "Live UIA target does not support this operation",
        ));
    }
    Ok(node)
}
pub(super) unsafe fn semantic_perform(
    window: &Window,
    element: &Element,
    operation: SemanticOperation,
    value: &Option<String>,
    cancel: &AtomicBool,
) -> cu_protocol::Result<String> {
    let _guard = ComGuard::new()?;
    let node = semantic_node(window, element, operation)?;
    cancelled(cancel)?;
    let mechanism = match operation {
        SemanticOperation::Invoke => {
            node.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                .map_err(err)?
                .Invoke()
                .map_err(err)?;
            "uia-invoke"
        }
        SemanticOperation::Toggle => {
            node.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
                .map_err(err)?
                .Toggle()
                .map_err(err)?;
            "uia-toggle"
        }
        SemanticOperation::Select => {
            node.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                UIA_SelectionItemPatternId,
            )
            .map_err(err)?
            .Select()
            .map_err(err)?;
            "uia-selection"
        }
        SemanticOperation::SetValue => {
            let value = value.as_ref().ok_or_else(|| {
                cu_protocol::Error::new("invalid_params", "Missing replacement value")
            })?;
            node.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .map_err(err)?
                .SetValue(&BSTR::from(value.as_str()))
                .map_err(err)?;
            "uia-value-pattern"
        }
    };
    cancelled(cancel)?;
    Ok(mechanism.into())
}

pub(super) unsafe fn accessibility(window: &Window) -> cu_protocol::Result<Vec<Element>> {
    let _guard = ComGuard::new()?;
    let automation: IUIAutomation =
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(err)?;
    let root = automation.ElementFromHandle(hwnd(window)?).map_err(err)?;
    let walker = automation.ControlViewWalker().map_err(err)?;
    let mut stack = vec![(root, 0u32, String::from("root"))];
    let mut elements = vec![];
    let start = Instant::now();
    while let Some((node, depth, locator)) = stack.pop() {
        if elements.len() >= 512 || start.elapsed() > Duration::from_secs(3) {
            break;
        }
        if node.CurrentProcessId().unwrap_or(-1) != window.pid as i32 {
            continue;
        }
        let password = node
            .CurrentIsPassword()
            .map(|b| b.as_bool())
            .unwrap_or(true);
        let rect = node
            .CurrentBoundingRectangle()
            .ok()
            .filter(|r| r.right > r.left && r.bottom > r.top)
            .map(|r| Rect {
                x: r.left as f64,
                y: r.top as f64,
                width: (r.right - r.left) as f64,
                height: (r.bottom - r.top) as f64,
            });
        let pattern = if password {
            None
        } else {
            node.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .ok()
        };
        elements.push(Element {
            id: new_id(),
            role: node
                .CurrentLocalizedControlType()
                .map(|v| v.to_string())
                .unwrap_or_default(),
            name: if password {
                String::from("[password]")
            } else {
                node.CurrentName()
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            },
            value: pattern
                .as_ref()
                .and_then(|p| p.CurrentValue().ok())
                .map(|v| v.to_string().chars().take(4096).collect()),
            bounds: rect,
            editable: pattern
                .as_ref()
                .is_some_and(|p| p.CurrentIsReadOnly().is_ok_and(|b| !b.as_bool())),
            password,
            depth,
            locator: locator.clone(),
            capabilities: Some(semantic_capabilities(&node)),
        });
        if password || depth >= 16 {
            continue;
        }
        let mut child = walker.GetFirstChildElement(&node).ok();
        let mut children = Vec::new();
        while let Some(current) = child {
            if children.len() >= 128 {
                break;
            }
            child = walker.GetNextSiblingElement(&current).ok();
            children.push((current, depth + 1, format!("{locator}/{}", children.len())));
        }
        stack.extend(children.into_iter().rev());
    }
    Ok(elements)
}
