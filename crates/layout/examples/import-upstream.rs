//! Convert the pinned XML fixtures once into the embedded upstream library.
fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("layouts/upstream");
    std::fs::create_dir_all(&dir).unwrap();
    for file in std::fs::read_dir(root.join("tests/upstream")).unwrap() {
        let path = file.unwrap().path();
        if path.extension().is_none_or(|s| s != "xml") {
            continue;
        }
        let name = path.file_name().unwrap().to_str().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let size = actionlay_layout::import::reference_size(name).unwrap_or([1920, 1080]);
        match actionlay_layout::import::xml(&text, name, size) {
            Ok(result) => {
                let out = dir
                    .join(path.file_stem().unwrap())
                    .with_extension("ovl.json");
                std::fs::write(out, result.layout.to_json().unwrap() + "\n").unwrap();
                println!(
                    "{name}: {} nodes, {} conversion notes",
                    result.layout.nodes.len(),
                    result.warnings.len()
                );
            }
            Err(e) => panic!("{name}: {e}"),
        }
    }
}
