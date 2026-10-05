use super::*;

fn editor() -> Editor {
    Editor::new(
        Layout::from_json(
            r#"{"version":1,"nodes":[
        {"type":"frame","id":"a","offset":[100,100],"size":[200,100]},
        {"type":"frame","id":"b","anchor":"bottom-right","offset":[-200,-100],"size":[200,100]},
        {"type":"group","id":"target","offset":[500,200],"size":[400,300],"children":[]}
    ]}"#,
        )
        .unwrap()
        .layout,
        None,
        None,
        false,
    )
}

fn measure(e: &mut Editor) {
    let mut image = Pixmap::new(960, 540).unwrap();
    e.renderer
        .render_editor_into(&e.draft, &e.demo, 30.0, &mut image);
}

fn rect_by_id(e: &Editor, id: &str) -> Rect {
    e.renderer
        .hit_boxes()
        .iter()
        .find(|hit| node_at(&e.draft.nodes, &hit.path).unwrap().id() == Some(id))
        .unwrap()
        .rect
}

fn assert_rect(a: Rect, b: Rect) {
    for (a, b) in [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)] {
        assert!((a - b).abs() < 0.01, "{a} != {b}");
    }
}

#[test]
fn grouping_ungrouping_and_undo_keep_visual_positions() {
    let mut e = editor();
    measure(&mut e);
    let a = rect_by_id(&e, "a");
    let b = rect_by_id(&e, "b");
    e.selection = Some(vec![0]);
    e.additional_selection = vec![vec![1]];
    let original = e.draft.clone();
    e.group_selection().unwrap();
    measure(&mut e);
    assert_eq!(e.draft.nodes.len(), 2);
    assert_rect(a, rect_by_id(&e, "a"));
    assert_rect(b, rect_by_id(&e, "b"));
    e.ungroup().unwrap();
    measure(&mut e);
    assert_eq!(e.draft.nodes.len(), 3);
    assert_rect(a, rect_by_id(&e, "a"));
    assert_rect(b, rect_by_id(&e, "b"));
    e.undo();
    e.undo();
    assert_eq!(e.draft, original);
}

#[test]
fn reparenting_remaps_shifted_destination_and_rejects_cycles() {
    let mut e = editor();
    measure(&mut e);
    let a = rect_by_id(&e, "a");
    let b = rect_by_id(&e, "b");
    e.selection = Some(vec![0]);
    e.additional_selection = vec![vec![1]];
    e.reparent(vec![2]).unwrap();
    measure(&mut e);
    assert_eq!(e.draft.nodes.len(), 1);
    assert_eq!(e.selected_paths(), vec![vec![0, 0], vec![0, 1]]);
    assert_rect(a, rect_by_id(&e, "a"));
    assert_rect(b, rect_by_id(&e, "b"));
    e.selection = Some(vec![0]);
    e.additional_selection.clear();
    let before = e.draft.clone();
    assert!(e.reparent(vec![0, 0]).is_err());
    assert_eq!(before, e.draft);
    e.selection = Some(vec![0, 0]);
    e.reparent(Vec::new()).unwrap();
    measure(&mut e);
    assert_rect(a, rect_by_id(&e, "a"));
}

#[test]
fn zero_sized_group_reparenting_keeps_its_child_origin() {
    let mut e = editor();
    e.draft.nodes[0] = serde_json::from_value(json!({"type":"group","offset":[100,100],"children":[{"type":"text","id":"child","offset":[40,30],"text":"Hello"}]})).unwrap();
    measure(&mut e);
    let child = rect_by_id(&e, "child");
    e.selection = Some(vec![0]);
    e.reparent(vec![2]).unwrap();
    measure(&mut e);
    assert_rect(child, rect_by_id(&e, "child"));
}

#[test]
fn selection_toggles_and_ancestor_normalization_prevent_double_edits() {
    let mut primary = None;
    let mut additional = Vec::new();
    select_path(&mut primary, &mut additional, vec![0], false);
    select_path(&mut primary, &mut additional, vec![1], true);
    assert_eq!(
        normalized_paths(
            primary
                .iter()
                .cloned()
                .chain(additional.iter().cloned())
                .collect()
        ),
        vec![vec![0], vec![1]]
    );
    select_path(&mut primary, &mut additional, vec![1], true);
    assert_eq!(primary, Some(vec![0]));
    assert_eq!(
        normalized_paths(vec![vec![0], vec![0, 1], vec![2]]),
        vec![vec![0], vec![2]]
    );
}

#[test]
fn multiselection_duplicate_delete_each_use_one_history_entry() {
    let mut e = editor();
    e.selection = Some(vec![0]);
    e.additional_selection = vec![vec![1]];
    e.duplicate();
    assert_eq!(e.draft.nodes.len(), 5);
    assert_eq!(e.undo.len(), 1);
    assert_eq!(e.selected_paths().len(), 2);
    assert!(e.draft.nodes[3].id().is_none());
    assert!(e.draft.nodes[4].id().is_none());
    e.delete();
    assert_eq!(e.draft.nodes.len(), 3);
    assert_eq!(e.undo.len(), 2);
    e.undo();
    assert_eq!(e.draft.nodes.len(), 5);
}

#[test]
fn snapping_uses_nearest_widget_edge_or_centre_on_both_axes() {
    let mut rect = Rect::new(197.0, 154.0, 100.0, 50.0);
    let guides = snap_to_rects(&mut rect, &[Rect::new(200.0, 100.0, 100.0, 100.0)], 8.0);
    assert_eq!(rect.x, 200.0);
    assert_eq!(rect.y, 150.0);
    assert_eq!(guides, [Some(200.0), Some(150.0)]);
}

#[test]
fn package_save_preserves_assets_and_failure_keeps_document_dirty() {
    let mut e = editor();
    actionlay_layout::package::attach(&mut e.draft, "assets/test.png".into(), vec![1, 2, 3])
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved.actionlay-layout");
    e.save_to(&path).unwrap();
    assert!(!e.dirty());
    assert_eq!(Layout::load(&path).unwrap().layout, e.draft);
    e.draft.name = Some("Changed".into());
    assert!(
        e.save_to(&dir.path().join("missing/fail.actionlay-layout"))
            .is_err()
    );
    assert!(e.dirty());
    assert_eq!(Layout::load(&path).unwrap().layout.name, None);
}

#[test]
fn dragging_multiple_widgets_moves_each_once_and_undoes_as_one_gesture() {
    let mut e = editor();
    measure(&mut e);
    e.snap = false;
    let a = rect_by_id(&e, "a");
    let b = rect_by_id(&e, "b");
    e.selection = Some(vec![0]);
    e.additional_selection = vec![vec![1]];
    let context = egui::Context::default();
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 540.0));
    let mut pass = |events| {
        let input = egui::RawInput {
            screen_rect: Some(canvas),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            let response = ui.allocate_rect(canvas, egui::Sense::click_and_drag());
            e.canvas_input(
                ui,
                &response,
                canvas,
                Rect::new(0.0, 0.0, 1920.0, 1080.0),
                0.5,
            );
        });
        output.textures_delta.clear();
    };
    pass(Vec::new());
    pass(vec![egui::Event::PointerMoved(egui::pos2(80.0, 75.0))]);
    pass(vec![egui::Event::PointerButton {
        pos: egui::pos2(80.0, 75.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    }]);
    pass(vec![egui::Event::PointerMoved(egui::pos2(105.0, 95.0))]);
    pass(vec![egui::Event::PointerMoved(egui::pos2(130.0, 115.0))]);
    pass(vec![egui::Event::PointerButton {
        pos: egui::pos2(130.0, 115.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    measure(&mut e);
    assert_rect(
        Rect::new(a.x + 100.0, a.y + 80.0, a.w, a.h),
        rect_by_id(&e, "a"),
    );
    assert_rect(
        Rect::new(b.x + 100.0, b.y + 80.0, b.w, b.h),
        rect_by_id(&e, "b"),
    );
    assert_eq!(e.undo.len(), 1);
    e.undo();
    measure(&mut e);
    assert_rect(a, rect_by_id(&e, "a"));
    assert_rect(b, rect_by_id(&e, "b"));
    context.tex_manager().write().take_delta().clear();
}

#[test]
fn automatic_anchor_uses_the_drop_region_without_changing_position() {
    let parent = Rect::new(0.0, 0.0, 1920.0, 1080.0);
    let rect = Rect::new(1650.0, 800.0, 200.0, 100.0);
    assert_eq!(nearest_anchor(rect, parent), Anchor::BottomRight);
    let mut node = template("frame");
    let mut value = serde_json::to_value(node).unwrap();
    value["anchor"] = serde_json::to_value(nearest_anchor(rect, parent)).unwrap();
    value["size"] = json!([rect.w, rect.h]);
    node = serde_json::from_value(value).unwrap();
    set_relative_position(&mut node, parent, rect);
    let Node::Known(widget) = node else { panic!() };
    assert_rect(
        rect,
        geom::place(
            parent,
            widget.common().anchor.unwrap(),
            widget.common().offset_in(parent),
            [rect.w, rect.h],
        ),
    );
}

#[test]
fn grouping_from_different_parents_keeps_inherited_opacity() {
    let layout=Layout::from_json(r#"{"version":1,"nodes":[
        {"type":"group","opacity":0.5,"offset":[100,100],"children":[{"type":"text","id":"a","text":"A","opacity":0.8}]},
        {"type":"text","id":"b","offset":[400,100],"text":"B"}
    ]}"#).unwrap().layout;
    let mut e = Editor::new(layout, None, None, false);
    measure(&mut e);
    let a = rect_by_id(&e, "a");
    let b = rect_by_id(&e, "b");
    e.selection = Some(vec![1]);
    e.additional_selection = vec![vec![0, 0]];
    e.group_selection().unwrap();
    measure(&mut e);
    assert_rect(a, rect_by_id(&e, "a"));
    assert_rect(b, rect_by_id(&e, "b"));
    let value = serde_json::to_value(&e.draft).unwrap();
    assert!(
        (value["nodes"][1]["children"][0]["opacity"]
            .as_f64()
            .unwrap()
            - 0.4)
            .abs()
            < 0.0001
    );
}

#[test]
fn copying_from_different_parents_keeps_selection_geometry() {
    let mut e = editor();
    e.draft.nodes[2]=serde_json::from_value(json!({"type":"group","offset":[500,200],"size":[400,300],"children":[{"type":"text","id":"child","offset":[30,40],"text":"Hello"}]})).unwrap();
    measure(&mut e);
    let child = rect_by_id(&e, "child");
    e.selection = Some(vec![0]);
    e.additional_selection = vec![vec![2, 0]];
    e.duplicate();
    measure(&mut e);
    let copy = e
        .renderer
        .hit_boxes()
        .iter()
        .find(|hit| hit.path == vec![4])
        .unwrap()
        .rect;
    assert_rect(
        Rect::new(child.x + 24.0, child.y + 24.0, child.w, child.h),
        copy,
    );
}
