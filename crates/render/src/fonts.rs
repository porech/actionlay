//! Document-scoped font resolution. Packaged faces precede installed faces.
use actionlay_layout::Layout;
use cosmic_text::fontdb::{self, Family, Query, Weight};
use std::collections::BTreeSet;
use std::sync::OnceLock;

fn installed() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    })
}

pub(crate) fn database(layout: &Layout) -> fontdb::Database {
    let mut db = fontdb::Database::new();
    for (name, data) in &layout.loaded_assets {
        if is_font(name) {
            db.load_font_data(data.as_ref().clone());
        }
    }
    let packaged: Vec<_> = db.faces().cloned().collect();
    for face in installed().faces() {
        // Keep bundled Roboto deterministic, unless a document supplies it.
        if face.families.iter().any(|(name, _)| name == crate::FAMILY) {
            continue;
        }
        let overridden = packaged.iter().any(|p| {
            p.weight == face.weight
                && p.style == face.style
                && p.families
                    .iter()
                    .any(|(name, _)| face.families.iter().any(|(other, _)| other == name))
        });
        if !overridden {
            db.push_face_info(face.clone());
        }
    }
    let mut fallback = fontdb::Database::new();
    for data in crate::text::FACES {
        fallback.load_font_data(data.to_vec());
    }
    for face in fallback.faces() {
        if !packaged.iter().any(|p| {
            p.weight == face.weight && p.families.iter().any(|(name, _)| name == crate::FAMILY)
        }) {
            db.push_face_info(face.clone());
        }
    }
    db
}

pub fn is_font(name: &str) -> bool {
    std::path::Path::new(name).extension().is_some_and(|ext| {
        ["ttf", "otf", "ttc"]
            .iter()
            .any(|kind| ext.eq_ignore_ascii_case(kind))
    })
}

pub fn valid_font(data: &[u8]) -> bool {
    let mut db = fontdb::Database::new();
    db.load_font_data(data.to_vec());
    db.faces().next().is_some()
}

pub fn families(layout: &Layout) -> Vec<String> {
    database(layout)
        .faces()
        .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Editor diagnostic cache excludes geometry and telemetry changes.
pub fn configuration_key(layout: &Layout) -> String {
    fn visit(value: &serde_json::Value, fields: &mut Vec<serde_json::Value>) {
        match value {
            serde_json::Value::Object(object) => {
                if object.contains_key("font") || object.contains_key("weight") {
                    fields.push(serde_json::json!([
                        object.get("font"),
                        object.get("weight")
                    ]));
                }
                for value in object.values() {
                    visit(value, fields);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    visit(value, fields);
                }
            }
            _ => {}
        }
    }
    let mut fields = Vec::new();
    visit(&serde_json::to_value(layout).unwrap(), &mut fields);
    format!(
        "{fields:?}:{:?}:{:?}",
        requested_faces(layout),
        layout
            .loaded_assets
            .iter()
            .map(|(name, data)| (name, std::sync::Arc::as_ptr(data) as usize))
            .collect::<Vec<_>>()
    )
}

pub(crate) fn query(db: &fontdb::Database, family: &str, weight: u16) -> Option<fontdb::ID> {
    db.query(&Query {
        families: &[Family::Name(family)],
        weight: Weight(weight),
        ..Default::default()
    })
}

/// Family references are retained even when a machine cannot resolve them.
pub(crate) fn requested(layout: &Layout) -> BTreeSet<String> {
    fn visit(value: &serde_json::Value, names: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(font) = fields.get("font").and_then(serde_json::Value::as_str) {
                    names.insert(font.into());
                }
                for value in fields.values() {
                    visit(value, names);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    visit(value, names);
                }
            }
            _ => {}
        }
    }
    let mut names = BTreeSet::new();
    visit(&serde_json::to_value(layout).unwrap(), &mut names);
    names
}

pub fn warnings(layout: &Layout) -> Vec<String> {
    let db = database(layout);
    let mut warnings = Vec::new();
    for (name, data) in &layout.loaded_assets {
        if is_font(name) && !valid_font(data) {
            warnings.push(format!("Unusable packaged font: {name}"));
        }
    }
    for (family, weight) in requested_faces(layout) {
        match query(&db, &family, weight).and_then(|id| db.face(id)) {
            None => {
                warnings.push(format!("Font {family} is unavailable; using Roboto"));
            }
            Some(face) if face.weight.0 != weight => warnings.push(format!(
                "Font {family} weight {weight} is unavailable; using Roboto"
            )),
            _ => {}
        }
    }
    warnings
}

/// Embed installed custom faces for the three weights supported by text styles.
/// Already packaged faces are reused instead of embedding duplicate files.
pub fn embed_used(layout: &mut Layout) -> Result<Vec<String>, String> {
    let db = database(layout);
    let names = requested_faces(layout);
    let warnings = warnings(layout);
    let mut seen = BTreeSet::new();
    let mut index = 0;
    for (family, weight) in names {
        if family == crate::FAMILY {
            continue;
        }
        if let Some(id) = query(&db, &family, weight) {
            let face = db.face(id).unwrap();
            if face.weight.0 != weight {
                continue;
            }
            if !seen.insert(face.post_script_name.clone()) {
                continue;
            }
            if let Some(bytes) = db.with_face_data(id, |data, _| data.to_vec()) {
                if layout
                    .loaded_assets
                    .values()
                    .any(|data| data.as_ref() == &bytes)
                {
                    continue;
                }
                let name = loop {
                    index += 1;
                    let name = format!("assets/export-font-{index}.ttf");
                    if !layout.loaded_assets.contains_key(&name) {
                        break name;
                    }
                };
                actionlay_layout::package::attach(layout, name, bytes)
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(warnings)
}

pub fn omit_fonts(layout: &mut Layout) -> Result<(), String> {
    let names = actionlay_layout::package::asset_names(layout).map_err(|e| e.to_string())?;
    let names: Vec<_> = names.into_iter().filter(|name| !is_font(name)).collect();
    layout.loaded_assets.retain(|name, _| !is_font(name));
    if names.is_empty() {
        layout.extra.remove("assets");
    } else {
        layout
            .extra
            .insert("assets".into(), serde_json::json!(names));
    }
    Ok(())
}

fn requested_faces(layout: &Layout) -> BTreeSet<(String, u16)> {
    use actionlay_layout::style::{ResolvedTheme, TextKind, TextStyleOpt};
    use actionlay_layout::{Node, Widget};
    fn visit(nodes: &[Node], theme: &ResolvedTheme, result: &mut BTreeSet<(String, u16)>) {
        let default = TextStyleOpt::default();
        for node in nodes {
            let Node::Known(widget) = node else {
                continue;
            };
            let mut record = |opt: &TextStyleOpt, kind| {
                let style = opt.resolve(kind, theme);
                result.insert((style.font, style.weight.value()));
            };
            match widget {
                Widget::Text(node) => record(&node.style, TextKind::Text),
                Widget::Metric(node) => record(&node.style, TextKind::Metric),
                Widget::MetricUnit(node) => record(&node.style, TextKind::MetricUnit),
                Widget::Datetime(node) => record(&node.style, TextKind::Datetime),
                Widget::Chart(node) | Widget::GradientChart(node) => record(
                    node.value_style.as_ref().unwrap_or(&default),
                    TextKind::Text,
                ),
                Widget::Map(node) => record(
                    node.label_style.as_ref().unwrap_or(&default),
                    TextKind::Text,
                ),
                Widget::GMeter(node) => record(
                    node.value_style.as_ref().unwrap_or(&default),
                    TextKind::Text,
                ),
                Widget::Gauge(actionlay_layout::model::GaugeNode { dial, .. })
                | Widget::Compass(actionlay_layout::model::CompassNode { dial, .. }) => {
                    record(
                        dial.value_style.as_ref().unwrap_or(&default),
                        TextKind::Metric,
                    );
                    record(
                        dial.label_style.as_ref().unwrap_or(&default),
                        TextKind::Text,
                    );
                }
                Widget::Bar(bar)
                | Widget::ZoneBar(actionlay_layout::model::ZoneBarNode { bar, .. }) => record(
                    bar.value_style.as_ref().unwrap_or(&default),
                    TextKind::Metric,
                ),
                _ => {}
            }
            visit(widget.children(), theme, result);
        }
    }
    let mut result = BTreeSet::new();
    visit(
        &layout.nodes,
        &layout.theme.clone().unwrap_or_default().resolve(),
        &mut result,
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // Rename the embedded test face without adding a redistributable fixture.
    fn custom_font() -> Vec<u8> {
        let mut data = crate::text::FACES[0].to_vec();
        let name = [0, b'R', 0, b'o', 0, b'b', 0, b'o', 0, b't', 0, b'o'];
        for index in 0..data.len() - name.len() {
            if data[index..index + name.len()] == name {
                data[index + 1] = b'X';
            }
        }
        data
    }

    #[test]
    fn packaged_font_is_resolved_only_inside_its_document() {
        let mut layout = actionlay_layout::default_layout();
        let bytes = custom_font();
        assert!(valid_font(&bytes));
        actionlay_layout::package::attach(&mut layout, "assets/custom.ttf".into(), bytes).unwrap();
        let db = database(&layout);
        assert!(query(&db, "Xoboto", 400).is_some());
        let other = database(&actionlay_layout::default_layout());
        assert!(query(&other, "Xoboto", 400).is_none());
        layout.theme.get_or_insert_default().font = Some("Xoboto".into());
        embed_used(&mut layout).unwrap();
        assert!(layout.loaded_assets.values().any(|data| valid_font(data)));
    }

    #[test]
    fn unusable_asset_and_missing_family_warn_without_losing_reference() {
        let mut layout = actionlay_layout::default_layout();
        layout.theme.get_or_insert_default().font = Some("ActionLay missing font 123".into());
        actionlay_layout::package::attach(&mut layout, "assets/bad.ttf".into(), vec![1, 2, 3])
            .unwrap();
        let warnings = warnings(&layout);
        assert!(warnings.iter().any(|warning| warning.contains("Unusable")));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("unavailable"))
        );
        assert_eq!(
            layout.theme.unwrap().font.as_deref(),
            Some("ActionLay missing font 123")
        );
    }

    #[test]
    fn excluding_fonts_keeps_images_and_family_references() {
        let mut layout = actionlay_layout::default_layout();
        layout.theme.get_or_insert_default().font = Some("Xoboto".into());
        actionlay_layout::package::attach(&mut layout, "assets/custom.ttf".into(), custom_font())
            .unwrap();
        actionlay_layout::package::attach(&mut layout, "assets/image.png".into(), vec![1, 2, 3])
            .unwrap();
        omit_fonts(&mut layout).unwrap();
        assert_eq!(
            actionlay_layout::package::asset_names(&layout).unwrap(),
            vec!["assets/image.png"]
        );
        assert_eq!(layout.loaded_assets.len(), 1);
        assert_eq!(layout.theme.unwrap().font.as_deref(), Some("Xoboto"));
    }
}
