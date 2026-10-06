use actionlay_layout::{Layout, model::Units};
use actionlay_render::{Renderer, regional};
use actionlay_telemetry::{GpsLock, Metric, Snapshot, Value};

#[test]
fn default_units_follow_user_preferences_and_explicit_choices_win() {
    let snapshot = Snapshot::for_test(
        0.0,
        None,
        GpsLock::Unknown,
        &[(Metric::Speed, Value::Present(10.0))],
    );
    let make = |layout_units: Option<&str>, widget_units: Option<&str>| {
        let mut value = serde_json::json!({"version":1,"nodes":[{"type":"metric","metric":"speed","format":"{value:.1} {unit}"}]});
        if let Some(units) = layout_units {
            value["units"] = units.into();
        }
        if let Some(units) = widget_units {
            value["nodes"][0]["units"] = units.into();
        }
        Layout::from_json(&value.to_string()).unwrap().layout
    };
    let mut renderer = Renderer::new();
    let mut pixels = |layout| {
        renderer
            .render(&layout, &snapshot, 640, 360)
            .data()
            .to_vec()
    };
    regional::configure(Some(Units::Metric));
    let metric = pixels(make(Some("metric"), None));
    let imperial = pixels(make(Some("imperial"), None));
    assert_ne!(metric, imperial);
    assert_eq!(pixels(make(None, None)), metric);
    regional::configure(Some(Units::Imperial));
    assert_eq!(pixels(make(Some("default"), Some("default"))), imperial);
    assert_eq!(
        pixels(make(Some("metric"), Some("default"))),
        metric,
        "layout overrides user preference"
    );
    assert_eq!(
        pixels(make(Some("metric"), Some("mph"))),
        imperial,
        "widget overrides layout"
    );
    regional::configure(Some(Units::Metric));
    assert_eq!(
        pixels(make(None, Some("mph"))),
        imperial,
        "explicit widget units survive preference changes"
    );
    assert_eq!(pixels(make(None, Some("default"))), metric);
}
