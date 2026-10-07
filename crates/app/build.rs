use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    for name in ["GITHUB_REF", "GITHUB_SHA"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    // Cargo caches build-script output. Watch the actual worktree HEAD and
    // branch reference so a local rebuild cannot retain a previous commit ID.
    for reference in [
        Some("HEAD".to_owned()),
        git(&["symbolic-ref", "-q", "HEAD"]),
        Some("refs/tags".to_owned()),
        Some("packed-refs".to_owned()),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let version = std::env::var("CARGO_PKG_VERSION").expect("workspace version");
    let stable_tag = format!("v{version}");
    let reference = std::env::var("GITHUB_REF").ok();
    let stable = reference.as_deref().map_or_else(
        || {
            git(&["tag", "--points-at", "HEAD", "--list", &stable_tag]).as_deref()
                == Some(stable_tag.as_str())
        },
        |reference| reference == format!("refs/tags/{stable_tag}"),
    );
    let display = if stable {
        version
    } else if let Some(sha) = std::env::var("GITHUB_SHA")
        .ok()
        .or_else(|| git(&["rev-parse", "HEAD"]))
    {
        format!("{version}-dev+{}", &sha[..sha.len().min(7)])
    } else {
        format!("{version}-dev")
    };
    println!("cargo:rustc-env=ACTIONLAY_BUILD_VERSION={display}");
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
