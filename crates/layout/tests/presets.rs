use actionlay_layout::{Layout, catalog::PRESETS, geom::ScaleMode, scale::auto_scale_mode};
use serde_json::json;

#[test]
fn all_bundled_presets_are_valid_known_and_match_the_schema() {
    let validator = jsonschema::validator_for(&actionlay_layout::json_schema()).unwrap();
    for preset in PRESETS {
        let loaded = Layout::from_json(preset.json).unwrap();
        assert!(
            loaded.warnings.is_empty(),
            "{}: {:?}",
            preset.id,
            loaded.warnings
        );
        assert!(
            validator.is_valid(&serde_json::from_str::<serde_json::Value>(preset.json).unwrap()),
            "{} fails schema",
            preset.id
        );
        let again = Layout::from_json(&loaded.layout.to_json().unwrap()).unwrap();
        assert!(
            again.warnings.is_empty(),
            "{}: {:?}",
            preset.id,
            again.warnings
        );
        assert_eq!(again.layout, loaded.layout);
        assert_eq!(
            auto_scale_mode(&loaded.layout, 1920, 1080),
            ScaleMode::Height
        );
        assert_eq!(
            auto_scale_mode(&loaded.layout, 1080, 1920),
            ScaleMode::Fit,
            "{} needs to fit portrait width",
            preset.id
        );
    }
}

#[test]
fn invalid_bar_ranges_zones_and_geometry_are_rejected() {
    for node in [
        json!({"type":"bar","metric":"alt","min":100,"max":100}),
        json!({"type":"bar","metric":"alt","size":[0,40]}),
        json!({"type":"bar","metric":"alt","radius":-1}),
        json!({"type":"bar","metric":"alt","stale_secs":-1}),
        json!({"type":"zone_bar","metric":"hr","zones":[]}),
        json!({"type":"zone_bar","metric":"hr","zones":[{"up_to":50,"color":"accent"}]}),
        json!({"type":"zone_bar","metric":"hr","zones":[{"up_to":80,"color":"accent"},{"up_to":60,"color":"primary"},{"up_to":100,"color":"accent"}]}),
    ] {
        assert!(Layout::from_json(&json!({"version":1,"nodes":[node]}).to_string()).is_err());
    }
    for ty in ["bar", "zone_bar"] {
        let layout = Layout::from_json(
            &json!({"version":1,"nodes":[{"type":ty,"metric":"speed"}]}).to_string(),
        )
        .unwrap();
        assert!(layout.warnings.is_empty());
    }
}

#[test]
fn relative_offsets_use_the_parent_dimensions_and_keep_scaled_offsets() {
    let common = actionlay_layout::model::Common {
        offset: Some([10.0, -10.0]),
        offset_relative: Some([0.02, -0.03]),
        ..Default::default()
    };
    let actual = common.offset_in(actionlay_layout::geom::Rect::new(0.0, 0.0, 1920.0, 1080.0));
    assert!((actual[0] - 48.4).abs() < 1e-4 && (actual[1] + 42.4).abs() < 1e-4);
    let mut layout = PRESETS[1].layout();
    let actionlay_layout::Node::Known(actionlay_layout::Widget::Frame(f)) = &mut layout.nodes[0]
    else {
        panic!()
    };
    f.common.offset_relative = Some([f32::NAN, 0.0]);
    assert!(layout.to_json().is_err());
}
