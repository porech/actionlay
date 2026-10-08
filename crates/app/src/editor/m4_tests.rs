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

#[test]
fn clipboard_survives_editor_changes_and_pastes_an_independent_section_with_undo() {
    let ctx = egui::Context::default();
    let layout = actionlay_layout::catalog::find("training")
        .unwrap()
        .layout();
    let chart = layout
        .nodes
        .iter()
        .position(|node| node.id() == Some("training-chart"))
        .unwrap();
    let mut source = Editor::new(layout, None, None, false);
    source.selection = Some(vec![chart]);
    let expected = source.draft.nodes[chart].clone();
    source.copy_selection(&ctx);
    assert!(!source.dirty());
    drop(source);

    let layout = actionlay_layout::catalog::find("moto").unwrap().layout();
    let mut destination = Editor::new(layout.clone(), None, None, false);
    destination.selection = Some(vec![0, 0]); // Paste must not nest inside the clock.
    destination.paste_selection(&ctx).unwrap();
    assert_eq!(destination.draft.nodes.len(), layout.nodes.len() + 1);
    let pasted = destination.draft.nodes.last().unwrap();
    let mut expected = serde_json::to_value(expected).unwrap();
    expected.as_object_mut().unwrap().remove("id");
    expected["offset"] = json!([24.0, 24.0]);
    assert_eq!(serde_json::to_value(pasted).unwrap(), expected);
    assert!(destination.dirty());
    destination.undo();
    assert_eq!(destination.draft, layout);
    assert!(!destination.dirty());
    destination.redo();
    assert_eq!(destination.draft.nodes.len(), layout.nodes.len() + 1);
    destination.selection = None;
    destination.copy_selection(&ctx); // Empty selection must not erase clipboard.
    destination.paste_selection(&ctx).unwrap();
    assert_eq!(destination.draft.nodes.len(), layout.nodes.len() + 2);
}

#[test]
fn clipboard_keeps_whole_containers_and_assets_without_overwriting_destination_assets() {
    let ctx = egui::Context::default();
    let layout = Layout::from_json(r#"{"version":1,"nodes":[{"type":"frame","id":"section","size":[400,200],"children":[{"type":"text","id":"child","text":"Altitude","future_asset":"assets/custom.ttf"}]}]}"#).unwrap().layout;
    let mut source = Editor::new(layout, None, None, false);
    actionlay_layout::package::attach(&mut source.draft, "assets/custom.ttf".into(), vec![1, 2])
        .unwrap();
    source.selection = Some(vec![0]);
    source.additional_selection = vec![vec![0, 0]]; // Do not duplicate selected descendants.
    source.copy_selection(&ctx);
    let mut destination = Editor::new(Editor::blank(), None, None, false);
    actionlay_layout::package::attach(
        &mut destination.draft,
        "assets/custom.ttf".into(),
        vec![3, 4],
    )
    .unwrap();
    let before = destination.draft.clone();
    destination.paste_selection(&ctx).unwrap();
    assert_eq!(destination.draft.nodes.len(), 1);
    let node = serde_json::to_value(&destination.draft.nodes[0]).unwrap();
    assert!(node.get("id").is_none());
    assert!(node["children"][0].get("id").is_none());
    assert_eq!(node["children"][0]["text"], "Altitude");
    assert_eq!(
        node["children"][0]["future_asset"],
        "assets/copy-2-custom.ttf"
    );
    assert_eq!(
        &**destination
            .draft
            .loaded_assets
            .get("assets/custom.ttf")
            .unwrap(),
        &[3, 4]
    );
    assert_eq!(
        &**destination
            .draft
            .loaded_assets
            .get("assets/copy-2-custom.ttf")
            .unwrap(),
        &[1, 2]
    );
    let mut package = std::io::Cursor::new(Vec::new());
    actionlay_layout::package::write_to(&destination.draft, &mut package).unwrap();
    package.set_position(0);
    assert_eq!(
        actionlay_layout::package::load_reader(package)
            .unwrap()
            .layout,
        destination.draft
    );
    destination.undo();
    assert_eq!(destination.draft, before);
    destination.redo();
    assert_eq!(destination.draft.loaded_assets.len(), 2);
}

#[test]
fn clipboard_detaches_nested_widgets_preserving_geometry_and_inherited_opacity() {
    let ctx = egui::Context::default();
    let layout = Layout::from_json(r#"{"version":1,"nodes":[{"type":"frame","offset":[400,200],"size":[400,200],"opacity":0.5,"children":[{"type":"frame","id":"nested","anchor":"bottom-right","offset":[-20,-20],"size":[100,60],"opacity":0.8}]}]}"#).unwrap().layout;
    let mut source = Editor::new(layout, None, None, false);
    measure(&mut source);
    let original = rect_by_id(&source, "nested");
    source.selection = Some(vec![0, 0]);
    source.copy_selection(&ctx);
    let mut destination = Editor::new(Editor::blank(), None, None, false);
    destination.paste_selection(&ctx).unwrap();
    measure(&mut destination);
    let placed = destination
        .renderer
        .hit_boxes()
        .iter()
        .find(|hit| hit.path == vec![0])
        .unwrap()
        .rect;
    assert_rect(
        Rect::new(original.x + 24.0, original.y + 24.0, original.w, original.h),
        placed,
    );
    let value = serde_json::to_value(&destination.draft.nodes[0]).unwrap();
    assert!((value["opacity"].as_f64().unwrap() - 0.4).abs() < 0.00001);
}

#[test]
fn native_and_browser_clipboard_events_transfer_nodes_once_between_editors() {
    let ctx = egui::Context::default();
    let mut source = editor();
    source.selection = Some(vec![0]);
    let input = |events| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input(vec![egui::Event::Copy]), |ui| {
        source.ui(ui, None, None, None, 0.0);
    });
    output.textures_delta.clear();
    drop(source);
    let mut destination = Editor::new(Editor::blank(), None, None, false);
    let mut output = ctx.run_ui(
        input(vec![egui::Event::Paste(
            "unrelated system clipboard text".into(),
        )]),
        |ui| {
            destination.ui(ui, None, None, None, 0.0);
        },
    );
    output.textures_delta.clear();
    assert_eq!(destination.draft.nodes.len(), 1);
    assert!(matches!(
        destination.draft.nodes[0],
        Node::Known(Widget::Frame(_))
    ));
    drop(destination);
    ctx.tex_manager().write().take_delta().clear();
}

#[test]
fn locked_section_keeps_children_selected_as_a_whole_and_can_move_resize_and_copy() {
    let layout = Layout::from_json(r#"{"version":1,"nodes":[{"type":"frame","id":"section","offset":[100,100],"size":[300,200],"children":[{"type":"frame","id":"child","offset":[20,20],"size":[100,60]}]}]}"#).unwrap().layout;
    let mut e = Editor::new(layout, None, None, false);
    measure(&mut e);
    e.selection = Some(vec![0]);
    e.selection_locked = true;
    e.snap = false;
    let context = egui::Context::default();
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 540.0));
    let pass = |e: &mut Editor, events| {
        measure(e);
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(canvas),
                events,
                ..Default::default()
            },
            |ui| {
                let response = ui.allocate_rect(canvas, egui::Sense::click_and_drag());
                e.canvas_input(
                    ui,
                    &response,
                    canvas,
                    Rect::new(0.0, 0.0, 1920.0, 1080.0),
                    0.5,
                );
            },
        );
        output.textures_delta.clear();
    };
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::SHIFT,
    };
    pass(&mut e, vec![]);
    let child_point = egui::pos2(80.0, 75.0);
    pass(&mut e, vec![egui::Event::PointerMoved(child_point)]);
    pass(&mut e, vec![button(child_point, true)]);
    pass(&mut e, vec![button(child_point, false)]);
    assert_eq!(e.selected_paths(), vec![vec![0]]);
    let blank = egui::pos2(500.0, 400.0);
    pass(&mut e, vec![egui::Event::PointerMoved(blank)]);
    pass(&mut e, vec![button(blank, true)]);
    pass(&mut e, vec![button(blank, false)]);
    assert_eq!(e.selected_paths(), vec![vec![0]]);
    pass(&mut e, vec![egui::Event::PointerMoved(child_point)]);
    pass(&mut e, vec![button(child_point, true)]);
    pass(
        &mut e,
        vec![egui::Event::PointerMoved(
            child_point + egui::vec2(30.0, 20.0),
        )],
    );
    pass(
        &mut e,
        vec![button(child_point + egui::vec2(30.0, 20.0), false)],
    );
    measure(&mut e);
    assert_rect(
        Rect::new(160.0, 140.0, 300.0, 200.0),
        rect_by_id(&e, "section"),
    );
    assert_rect(
        Rect::new(180.0, 160.0, 100.0, 60.0),
        rect_by_id(&e, "child"),
    );
    let handle = egui::pos2(230.0, 170.0);
    pass(&mut e, vec![egui::Event::PointerMoved(handle)]);
    pass(&mut e, vec![button(handle, true)]);
    pass(
        &mut e,
        vec![egui::Event::PointerMoved(handle + egui::vec2(20.0, 15.0))],
    );
    pass(&mut e, vec![button(handle + egui::vec2(20.0, 15.0), false)]);
    measure(&mut e);
    assert_rect(
        Rect::new(160.0, 140.0, 340.0, 230.0),
        rect_by_id(&e, "section"),
    );
    e.copy_selection(&context);
    let mut destination = Editor::new(Editor::blank(), None, None, false);
    destination.paste_selection(&context).unwrap();
    assert_eq!(destination.draft.nodes.len(), 1);
    let Node::Known(widget) = &destination.draft.nodes[0] else {
        panic!()
    };
    assert_eq!(widget.children().len(), 1);
    e.selection_locked = false;
    let point = [190.0, 170.0];
    assert_eq!(
        e.preview_hit(e.renderer.hit_boxes(), point, false)
            .unwrap()
            .path,
        vec![0, 0]
    );
    drop(destination);
    drop(e);
    context.tex_manager().write().take_delta().clear();
}

#[test]
fn selection_lock_hits_each_selected_root_without_selecting_unselected_widgets() {
    let mut e = editor();
    measure(&mut e);
    e.selection = Some(vec![0]);
    e.additional_selection = vec![vec![1]];
    e.selection_locked = true;
    for id in ["a", "b"] {
        let r = rect_by_id(&e, id);
        let hit = e
            .preview_hit(e.renderer.hit_boxes(), [r.x + 10.0, r.y + 10.0], true)
            .unwrap();
        assert!(e.selected_paths().contains(&hit.path));
    }
    assert!(
        e.preview_hit(e.renderer.hit_boxes(), [550.0, 250.0], false)
            .is_none()
    );
    assert!(!e.dirty());
}

#[test]
fn shift_can_toggle_section_scaling_during_one_resize_gesture_with_undo() {
    let mut e = Editor::new(Layout::from_json(r#"{"version":1,"nodes":[{"type":"frame","id":"section","offset":[100,100],"size":[300,200],"children":[{"type":"frame","id":"child","offset":[20,20],"size":[100,60]}]}]}"#).unwrap().layout,None,None,false);
    let original = e.draft.clone();
    e.selection = Some(vec![0]);
    e.selection_locked = true;
    let context = egui::Context::default();
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 540.0));
    let pass = |e: &mut Editor, mut events: Vec<egui::Event>, shift| {
        events.insert(
            0,
            egui::Event::ModifiersChanged(egui::Modifiers {
                shift,
                ..Default::default()
            }),
        );
        measure(e);
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(canvas),
                events,
                ..Default::default()
            },
            |ui| {
                let response = ui.allocate_rect(canvas, egui::Sense::click_and_drag());
                e.canvas_input(
                    ui,
                    &response,
                    canvas,
                    Rect::new(0.0, 0.0, 1920.0, 1080.0),
                    0.5,
                );
            },
        );
        output.textures_delta.clear();
    };
    let handle = egui::pos2(200.0, 150.0);
    let end = handle + egui::vec2(30.0, 0.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::SHIFT,
    };
    pass(&mut e, vec![], true);
    pass(&mut e, vec![egui::Event::PointerMoved(handle)], true);
    pass(&mut e, vec![button(handle, true)], true);
    pass(&mut e, vec![egui::Event::PointerMoved(end)], true);
    measure(&mut e);
    let factor = (300.0 * 360.0 + 200.0 * 200.0) / (300.0 * 300.0 + 200.0 * 200.0);
    assert_rect(
        Rect::new(100.0, 100.0, 300.0 * factor, 200.0 * factor),
        rect_by_id(&e, "section"),
    );
    assert_rect(
        Rect::new(
            100.0 + 20.0 * factor,
            100.0 + 20.0 * factor,
            100.0 * factor,
            60.0 * factor,
        ),
        rect_by_id(&e, "child"),
    );
    pass(&mut e, vec![], false);
    measure(&mut e);
    assert_rect(
        Rect::new(100.0, 100.0, 360.0, 200.0),
        rect_by_id(&e, "section"),
    );
    assert_rect(
        Rect::new(120.0, 120.0, 100.0, 60.0),
        rect_by_id(&e, "child"),
    );
    pass(&mut e, vec![], true);
    pass(&mut e, vec![button(end, false)], true);
    let resized = e.draft.clone();
    assert_eq!(e.undo.len(), 1);
    e.undo();
    assert_eq!(e.draft, original);
    e.redo();
    assert_eq!(e.draft, resized);
    drop(e);
    context.tex_manager().write().take_delta().clear();
}
