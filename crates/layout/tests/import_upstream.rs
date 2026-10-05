use actionlay_layout::{Layout, Node, Widget, catalog::UPSTREAM_PRESETS, import};

#[test]
fn all_thirteen_pinned_layouts_convert_to_the_embedded_known_widget_library() {
    assert_eq!(UPSTREAM_PRESETS.len(), 13);
    for preset in UPSTREAM_PRESETS {
        let name = format!("{}.xml", preset.id.trim_start_matches("upstream-"));
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/upstream")
                .join(&name),
        )
        .unwrap();
        let converted = import::xml(
            &text,
            &name,
            import::reference_size(&name).unwrap_or([1920, 1080]),
        )
        .unwrap();
        assert_eq!(
            converted.layout,
            preset.layout(),
            "stale conversion: {name}"
        );
        fn known(nodes: &[Node]) {
            for n in nodes {
                match n {
                    Node::Known(Widget::Group(g)) => known(&g.children),
                    Node::Known(Widget::Frame(f)) => known(&f.children),
                    Node::Known(_) => {}
                    Node::Unknown(_) => panic!("unconverted upstream widget"),
                }
            }
        }
        known(&converted.layout.nodes);
        assert_eq!(
            Layout::from_json(&converted.layout.to_json().unwrap())
                .unwrap()
                .layout,
            converted.layout
        );
    }
}

#[test]
fn unsupported_components_are_preserved_and_reported() {
    let converted = import::xml(
        r#"<layout><component type="future-widget" x="30" custom="hello"/></layout>"#,
        "custom.xml",
        [1920, 1080],
    )
    .unwrap();
    assert!(!converted.warnings.is_empty());
    let saved = converted.layout.to_json().unwrap();
    assert!(saved.contains("future-widget") && saved.contains("hello"));
}

#[test]
fn centred_text_aligns_to_its_xml_origin_without_guessing_text_width() {
    let converted = import::xml(r#"<layout><frame x="100" y="40" width="300" height="80"><component type="text" x="150" y="10" align="centre">Long arbitrary text</component></frame></layout>"#, "custom.xml", [1920,1080]).unwrap();
    let Node::Known(Widget::Frame(f)) = &converted.layout.nodes[0] else {
        panic!()
    };
    let Node::Known(Widget::Group(g)) = &f.children[0] else {
        panic!()
    };
    assert_eq!(g.common.offset, Some([150.0, 10.0]));
    assert_eq!(g.size, None);
    let Node::Known(child) = &g.children[0] else {
        panic!()
    };
    assert_eq!(
        child.common().anchor,
        Some(actionlay_layout::geom::Anchor::Top)
    );
}
