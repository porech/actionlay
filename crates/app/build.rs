fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/actionlay.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/icons/actionlay.ico")
            .set("ProductName", "ActionLay")
            .set(
                "FileDescription",
                "ActionLay — action-camera telemetry dashboards",
            )
            .compile()
            .expect("compile Windows icon and version resources");
    }
}
