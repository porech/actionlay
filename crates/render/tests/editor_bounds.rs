use actionlay_layout::{Layout, geom::ScaleMode};
use actionlay_render::{Renderer, tiny_skia::Pixmap};
use actionlay_telemetry::Telemetry;

#[test]
fn selection_uses_the_rendered_text_box_and_nested_parent() {
    let layout=Layout::from_json(r#"{"version":1,"nodes":[{"type":"frame","anchor":"bottom-right","offset_relative":[-0.02,-0.02],"size":[400,300],"children":[{"type":"text","anchor":"center","text":"ActionLay","size":40}]}]}"#).unwrap().layout;
    let mut renderer = Renderer::new();
    let telemetry = Telemetry::preview();
    let mut image = Pixmap::new(960, 540).unwrap();
    renderer.render_editor_into(&layout, &telemetry, 30.0, &mut image);
    let boxes = renderer.hit_boxes();
    assert_eq!(boxes.len(), 2);
    assert_eq!(boxes[0].path, vec![0]);
    assert_eq!(boxes[1].path, vec![0, 0]);
    assert_eq!(boxes[1].parent, boxes[0].rect);
    assert!(boxes[1].rect.w > 100.0 && boxes[1].rect.h == 40.0);
    let text = boxes[1].rect;
    let parent = boxes[1].parent;
    assert!((text.x + text.w / 2.0 - parent.x - parent.w / 2.0).abs() < 0.01);
    assert!((text.y + text.h / 2.0 - parent.y - parent.h / 2.0).abs() < 0.01);
    let before = image.data().to_vec();
    renderer.render_telemetry_into(&layout, &telemetry, 30.0, &mut image);
    assert_eq!(
        before,
        image.data(),
        "recording hit boxes must not change rendering"
    );
    renderer.set_scale_mode(ScaleMode::Height);
    let mut bigger = Pixmap::new(1920, 1080).unwrap();
    renderer.render_editor_into(&layout, &telemetry, 30.0, &mut bigger);
    assert_eq!(renderer.hit_boxes()[1].rect, text);
}
