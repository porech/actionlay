//! Portable layouts: layout.json and explicitly declared document assets.
use crate::{Layout, LayoutError, Loaded};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Component, Path};
use std::sync::Arc;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const SUFFIX: &str = "actionlay-layout";
const MAX_ASSET: u64 = 32 * 1024 * 1024;
const MAX_TOTAL: u64 = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 256;

fn invalid(message: impl Into<String>) -> LayoutError {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

pub fn is_package(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(SUFFIX))
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', ':'])
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && !name.starts_with('/')
        && name != "layout.json"
}

/// The manifest remains an ordinary JSON field, so older readers preserve it.
pub fn asset_names(layout: &Layout) -> Result<Vec<String>, LayoutError> {
    let Some(value) = layout.extra.get("assets") else {
        return Ok(Vec::new());
    };
    let names = value
        .as_array()
        .ok_or_else(|| invalid("assets must be an array of relative paths"))?;
    if names.len() >= MAX_ENTRIES {
        return Err(invalid("too many layout assets"));
    }
    let mut seen = BTreeSet::new();
    for name in names {
        let name = name.as_str().ok_or_else(|| invalid("invalid asset name"))?;
        if !safe_name(name) || !seen.insert(name.to_owned()) {
            return Err(invalid(format!("unsafe or duplicate asset path: {name}")));
        }
    }
    Ok(seen.into_iter().collect())
}

pub fn attach(layout: &mut Layout, name: String, bytes: Vec<u8>) -> Result<(), LayoutError> {
    if !safe_name(&name) || bytes.len() as u64 > MAX_ASSET {
        return Err(invalid("invalid or oversized asset"));
    }
    let mut names = asset_names(layout)?;
    if !names.contains(&name) {
        names.push(name.clone());
    }
    if names.len() >= MAX_ENTRIES {
        return Err(invalid("too many layout assets"));
    }
    layout
        .extra
        .insert("assets".into(), serde_json::json!(names));
    layout.loaded_assets.insert(name, Arc::new(bytes));
    Ok(())
}

pub fn load_loose_assets(layout: &mut Layout, directory: &Path) -> Result<(), LayoutError> {
    let names = asset_names(layout)?;
    if names.is_empty() {
        return Ok(());
    }
    let directory = directory.canonicalize()?;
    let mut total = 0;
    for name in names {
        let path = directory.join(&name).canonicalize()?;
        if !path.starts_with(&directory) {
            return Err(invalid("asset escapes layout directory"));
        }
        let size = std::fs::metadata(&path)?.len();
        total += size;
        if size > MAX_ASSET || total > MAX_TOTAL {
            return Err(invalid("layout assets are too large"));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_ASSET + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_ASSET {
            return Err(invalid("asset is too large"));
        }
        layout.loaded_assets.insert(name, Arc::new(bytes));
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<Loaded, LayoutError> {
    let mut archive =
        ZipArchive::new(std::fs::File::open(path)?).map_err(|e| invalid(e.to_string()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(invalid("too many package entries"));
    }
    let mut entries = BTreeSet::new();
    let mut total = 0;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| invalid(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        if entry.name() != "layout.json" && !safe_name(entry.name()) {
            return Err(invalid("unsafe package path"));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(invalid("package links are not supported"));
        }
        if !entries.insert(entry.name().to_owned()) {
            return Err(invalid("duplicate package entry"));
        }
        total += entry.size();
        if entry.size() > MAX_ASSET || total > MAX_TOTAL {
            return Err(invalid("package is too large"));
        }
    }
    let text = {
        let mut entry = archive
            .by_name("layout.json")
            .map_err(|e| invalid(e.to_string()))?;
        let mut text = String::new();
        (&mut entry).take(MAX_ASSET + 1).read_to_string(&mut text)?;
        if text.len() as u64 > MAX_ASSET {
            return Err(invalid("layout JSON is too large"));
        }
        text
    };
    let mut loaded = Layout::from_json(&text)?;
    for name in asset_names(&loaded.layout)? {
        let mut entry = archive
            .by_name(&name)
            .map_err(|e| invalid(format!("{name}: {e}")))?;
        let mut bytes = Vec::new();
        (&mut entry).take(MAX_ASSET + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_ASSET {
            return Err(invalid("asset is too large"));
        }
        loaded.layout.loaded_assets.insert(name, Arc::new(bytes));
    }
    Ok(loaded)
}

/// Synchronize a neighbouring temporary file, then replace the destination.
pub fn save(layout: &Layout, path: &Path) -> Result<(), LayoutError> {
    let text = layout.to_json()?;
    let names = asset_names(layout)?;
    let mut total = text.len() as u64;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    {
        let mut archive = ZipWriter::new(file.as_file_mut());
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        archive
            .start_file("layout.json", options)
            .map_err(|e| invalid(e.to_string()))?;
        archive.write_all(text.as_bytes())?;
        for name in names {
            let bytes = layout
                .loaded_assets
                .get(&name)
                .ok_or_else(|| invalid(format!("missing asset: {name}")))?;
            total += bytes.len() as u64;
            if bytes.len() as u64 > MAX_ASSET || total > MAX_TOTAL {
                return Err(invalid("package is too large"));
            }
            archive
                .start_file(name, options)
                .map_err(|e| invalid(e.to_string()))?;
            archive.write_all(bytes)?;
        }
        archive.finish().map_err(|e| invalid(e.to_string()))?;
    }
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_assets_and_unknown_fields_round_trip() {
        let mut layout = crate::default_layout();
        layout
            .extra
            .insert("future".into(), serde_json::json!({"x": 42}));
        attach(&mut layout, "assets/image.png".into(), vec![1, 2, 3]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.actionlay-layout");
        save(&layout, &path).unwrap();
        let loaded = Layout::load(&path).unwrap().layout;
        assert_eq!(loaded, layout);
        assert!(attach(&mut layout, "../escape.ttf".into(), vec![]).is_err());
    }

    #[test]
    fn declared_assets_are_validated_without_unknown_field_warnings() {
        let mut layout = crate::default_layout();
        attach(&mut layout, "assets/test.png".into(), vec![1, 2, 3]).unwrap();
        assert!(
            !crate::validate::validate(&layout)
                .iter()
                .any(|issue| issue.message.contains("assets"))
        );
        layout
            .extra
            .insert("assets".into(), serde_json::json!(["../escape"]));
        assert!(layout.to_json().is_err());
    }
    #[test]
    fn missing_assets_do_not_replace_existing_package() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.actionlay-layout");
        std::fs::write(&path, b"original").unwrap();
        let mut layout = crate::default_layout();
        layout
            .extra
            .insert("assets".into(), serde_json::json!(["assets/missing.ttf"]));
        assert!(save(&layout, &path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }
    #[test]
    fn unsafe_archives_and_missing_manifests_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "../escape",
            "/absolute",
            "C:/escape",
            "assets\\escape",
            "safe",
        ] {
            let path = dir.path().join("bad.actionlay-layout");
            let mut zip = ZipWriter::new(std::fs::File::create(&path).unwrap());
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(b"bad").unwrap();
            zip.finish().unwrap();
            assert!(load(&path).is_err());
        }
    }
}
