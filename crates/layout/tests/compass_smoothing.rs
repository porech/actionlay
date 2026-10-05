use actionlay_layout::{Layout, Node, Widget};

#[test]
fn smoothing_settings_belong_to_each_widget_and_survive_layout_save() {
    let json = r#"{"version":1,"nodes":[
        {"type":"compass","metric":"heading","smoothing":{"enabled":false,"seconds":0.75,"deadband":2,"max_rate":90,"min_speed":1}},
        {"type":"compass","metric":"heading","smoothing":{"seconds":0.25,"deadband":0.5}},
        {"type":"compass","metric":"heading"}
    ]}"#;
    let layout = Layout::from_json(json).unwrap().layout;
    let saved = layout.to_json().unwrap();
    assert_eq!(Layout::from_json(&saved).unwrap().layout, layout);
    let Node::Known(Widget::Compass(first)) = &layout.nodes[0] else {
        panic!("expected compass")
    };
    let filter = first.smoothing.as_ref().unwrap();
    assert_eq!(filter.enabled, Some(false));
    assert_eq!(filter.seconds, Some(0.75));
    let Node::Known(Widget::Compass(second)) = &layout.nodes[1] else {
        panic!("expected compass")
    };
    assert_eq!(second.smoothing.as_ref().unwrap().seconds, Some(0.25));
    let Node::Known(Widget::Compass(third)) = &layout.nodes[2] else {
        panic!("expected compass")
    };
    assert_eq!(third.smoothing, None);
}

#[test]
fn invalid_filter_thresholds_are_rejected_even_when_disabled() {
    for (key, value) in [
        ("seconds", -1.0),
        ("deadband", 181.0),
        ("max_rate", -2.0),
        ("min_speed", 101.0),
    ] {
        let json = format!(
            r#"{{"version":1,"nodes":[{{"type":"compass","metric":"heading","smoothing":{{"enabled":false,"{key}":{value}}}}}]}}"#
        );
        assert!(Layout::from_json(&json).is_err(), "accepted {key}={value}");
    }
}
