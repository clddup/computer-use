use super::*;
fn obs() -> Observation {
    Observation {
        id: "o".into(),
        helper_epoch: "e".into(),
        window: Window {
            id: "1".into(),
            pid: 1,
            process_identity: "p".into(),
            title: String::new(),
            bounds: Rect {
                x: -500.0,
                y: 20.0,
                width: 400.0,
                height: 200.0,
            },
        },
        captured_at_ms: 0,
        expires_at_ms: 100,
        elements: vec![],
        images: vec![Image {
            id: "i".into(),
            mime_type: "image/png".into(),
            data: String::new(),
            width: 800,
            height: 400,
            screen_bounds: Rect {
                x: -500.0,
                y: 20.0,
                width: 400.0,
                height: 200.0,
            },
            capture_method: "test".into(),
        }],
        warnings: vec![],
    }
}
#[test]
fn scaled_negative_origin() {
    assert_eq!(
        resolve_target(
            &obs(),
            &Target::Image {
                image_id: "i".into(),
                x: 200.0,
                y: 100.0
            }
        )
        .unwrap(),
        (-400.0, 70.0)
    );
}
#[test]
fn invalid_coordinates() {
    for x in [-1.0, 800.0, f64::NAN] {
        assert!(resolve_target(
            &obs(),
            &Target::Image {
                image_id: "i".into(),
                x,
                y: 0.0
            }
        )
        .is_err());
    }
}
#[test]
fn stale_image() {
    assert!(resolve_target(
        &obs(),
        &Target::Image {
            image_id: "old".into(),
            x: 0.0,
            y: 0.0
        }
    )
    .is_err());
}
#[test]
fn bounded_text_and_key_sequences() {
    assert!(validate_text(&"中".repeat(5462)).is_err());
    assert!(validate_text("contains\0nul").is_err());
    assert!(validate_text(&"a".repeat(16384)).is_ok());
    for keys in [vec![], vec!["a".into(); 6], vec!["x".repeat(25)]] {
        assert!(resolve_action(&obs(), &Action::Key { keys }).is_err());
    }
}
#[test]
fn bounded_drag_duration() {
    let target = Target::Image {
        image_id: "i".into(),
        x: 100.0,
        y: 100.0,
    };
    for duration_ms in [0, 49, 3001, u32::MAX] {
        assert!(resolve_action(
            &obs(),
            &Action::Drag {
                from: target.clone(),
                to: target.clone(),
                duration_ms
            }
        )
        .is_err());
    }
    for duration_ms in [50, 3000] {
        assert!(resolve_action(
            &obs(),
            &Action::Drag {
                from: target.clone(),
                to: target.clone(),
                duration_ms
            }
        )
        .is_ok());
    }
}
#[test]
fn password_targets_and_non_editors_are_rejected() {
    let mut observation = obs();
    observation.elements.push(Element {
        id: "password".into(),
        role: "edit".into(),
        name: "[password]".into(),
        value: None,
        bounds: Some(Rect {
            x: -400.0,
            y: 40.0,
            width: 100.0,
            height: 20.0,
        }),
        editable: true,
        password: true,
        depth: 1,
        locator: "root/0".into(),
        capabilities: None,
    });
    assert!(resolve_target(
        &observation,
        &Target::Element {
            element_id: "password".into()
        }
    )
    .is_err());
    let action = Action::SetValue {
        element_id: "password".into(),
        value: "fixture".into(),
    };
    assert!(resolve_action(&observation, &action).is_err());
    observation.elements[0].password = false;
    observation.elements[0].editable = false;
    assert!(resolve_action(&observation, &action).is_err());
}
#[test]
fn mapped_pixels_cannot_escape_the_authorized_window() {
    let mut observation = obs();
    observation.images[0].screen_bounds.x = 1000.0;
    assert!(resolve_target(
        &observation,
        &Target::Image {
            image_id: "i".into(),
            x: 0.0,
            y: 0.0
        }
    )
    .is_err());
}
#[test]
fn policy_and_capture_default_to_denial() {
    assert_eq!(
        Policy::default().check(&obs().window).unwrap_err().code,
        "permission_denied"
    );
    assert!(png_base64(0, 1, &[]).is_err());
    assert!(png_base64(1, 1, &[0; 3]).is_err());
    assert!(png_base64(5000, 5000, &[]).is_err());
}
#[test]
fn semantic_actions_require_capabilities_and_retain_identity() {
    let mut observation = obs();
    let element = Element {
        id: "e".into(),
        role: "button".into(),
        name: "Fixture".into(),
        value: None,
        bounds: Some(Rect {
            x: -400.0,
            y: 40.0,
            width: 100.0,
            height: 20.0,
        }),
        editable: true,
        password: false,
        depth: 1,
        locator: "root/0".into(),
        capabilities: None,
    };
    observation.elements.push(element);
    let actions = [
        Action::Invoke {
            element_id: "e".into(),
        },
        Action::Toggle {
            element_id: "e".into(),
        },
        Action::Select {
            element_id: "e".into(),
        },
        Action::SetValue {
            element_id: "e".into(),
            value: "replacement".into(),
        },
    ];
    for action in &actions[..3] {
        assert_eq!(
            resolve_action(&observation, action).unwrap_err().code,
            "unsupported_action"
        );
    }
    observation.elements[0].capabilities = Some(vec![
        "invoke".into(),
        "toggle".into(),
        "select".into(),
        "set_value".into(),
    ]);
    for action in &actions {
        match resolve_action(&observation, action).unwrap() {
            NativeAction::Semantic { element, .. } => {
                assert_eq!(element.locator, "root/0");
                assert_eq!(element.name, "Fixture");
            }
            _ => panic!("Semantic intent lost"),
        }
    }
    observation.elements[0].password = true;
    for action in &actions {
        assert!(resolve_action(&observation, action).is_err());
    }
    observation.elements[0].password = false;
    observation.elements[0].bounds = None;
    assert!(resolve_action(&observation, &actions[0]).is_err());
}
#[test]
fn invalid_scroll() {
    assert!(resolve_action(
        &obs(),
        &Action::Scroll {
            target: Target::Image {
                image_id: "i".into(),
                x: 0.0,
                y: 0.0
            },
            dx: i32::MIN,
            dy: 0
        }
    )
    .is_err());
}
