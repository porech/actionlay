//! Stable-release updates. Network/staging work never runs on the UI thread.
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use eframe::egui;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const API: &str = "https://api.github.com/repos/porech/actionlay/releases/latest";

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub disabled: bool,
    pub skipped_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Availability {
    Development,
    #[cfg(target_os = "linux")]
    System,
    Enabled,
    Unsupported,
}

pub fn availability() -> Availability {
    static AVAILABILITY: std::sync::OnceLock<Availability> = std::sync::OnceLock::new();
    *AVAILABILITY.get_or_init(probe_availability)
}

fn probe_availability() -> Availability {
    if env!("ACTIONLAY_BUILD_VERSION").contains("-dev") {
        return Availability::Development;
    }
    #[cfg(target_os = "linux")]
    if std::env::current_exe().is_ok_and(|exe| system_managed(&exe)) {
        return Availability::System;
    }
    if cfg!(target_os = "macos")
        || cfg!(all(
            target_arch = "x86_64",
            any(target_os = "windows", target_os = "linux")
        ))
    {
        Availability::Enabled
    } else {
        Availability::Unsupported
    }
}

#[cfg(target_os = "linux")]
fn system_managed(exe: &Path) -> bool {
    // Inspect ownership of THIS executable: a portable copy alongside an APT
    // installation must still update itself. Includes local DEBs and RPMs.
    [("dpkg-query", "-S"), ("rpm", "-qf")]
        .iter()
        .any(|(program, flag)| {
            std::process::Command::new(program)
                .arg(flag)
                .arg(exe)
                .output()
                .is_ok_and(|out| out.status.success())
        })
}

pub fn settings(ui: &mut egui::Ui, prefs: &mut Preferences) {
    ui.separator();
    ui.heading(crate::i18n::text("Automatic updates"));
    match availability() {
        Availability::Development => {
            ui.label(crate::i18n::ui_text(
                ui,
                "Automatic updates are disabled in development builds.",
            ));
        }
        #[cfg(target_os = "linux")]
        Availability::System => {
            ui.label(crate::i18n::ui_text(
                ui,
                "ActionLay updates will be downloaded automatically with system updates.",
            ));
        }
        Availability::Unsupported => {
            ui.label(crate::i18n::ui_text(
                ui,
                "Automatic updates are unavailable on this platform.",
            ));
        }
        Availability::Enabled => {
            let mut enabled = !prefs.disabled;
            if ui
                .checkbox(
                    &mut enabled,
                    crate::i18n::text("Check for updates at startup"),
                )
                .changed()
            {
                prefs.disabled = !enabled;
            }
            if let Some(version) = &prefs.skipped_version {
                ui.label(version);
                if ui
                    .button(crate::i18n::text("Ask again for the skipped version"))
                    .clicked()
                {
                    prefs.skipped_version = None;
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Debug, Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}
#[derive(Debug, Clone)]
struct Update {
    version: String,
    asset: Asset,
}
fn asset_name(version: &str) -> String {
    let suffix = if cfg!(target_os = "windows") {
        if std::env::current_exe().is_ok_and(|exe| installation_mode(&exe).is_some()) {
            "windows-x64-setup.exe"
        } else {
            "windows-x64.zip"
        }
    } else if cfg!(target_os = "macos") {
        "macos-universal.dmg"
    } else {
        "linux-x64.tar.gz"
    };
    format!("actionlay-{version}-{suffix}")
}
fn select(release: Release, current: &str, prefs: &Preferences) -> Result<Option<Update>> {
    if prefs.disabled || release.draft || release.prerelease {
        return Ok(None);
    }
    let version = release
        .tag_name
        .strip_prefix('v')
        .context("Invalid release tag")?;
    let remote = semver::Version::parse(version)?;
    if !remote.pre.is_empty()
        || !remote.build.is_empty()
        || remote <= semver::Version::parse(current)?
        || prefs.skipped_version.as_deref() == Some(version)
    {
        return Ok(None);
    }
    let Some(asset) = release
        .assets
        .into_iter()
        .find(|a| a.name == asset_name(version))
    else {
        return Ok(None);
    };
    ensure!(asset.size > 0, "Empty release asset");
    ensure!(
        asset.browser_download_url
            == format!(
                "https://github.com/porech/actionlay/releases/download/{}/{}",
                release.tag_name, asset.name
            ),
        "Unexpected download URL"
    );
    let digest = asset
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .context("Missing release checksum")?;
    ensure!(
        digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid release checksum"
    );
    Ok(Some(Update {
        version: version.to_owned(),
        asset,
    }))
}
fn client(timeout: u64) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("ActionLay/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(timeout))
        .https_only(true)
        .build()?)
}

#[derive(Default)]
struct Progress {
    bytes: u64,
    total: u64,
}
enum Event {
    Checked(Result<Option<Update>>),
    Prepared(Result<Prepared>),
}

pub struct Updater {
    rx: Option<mpsc::Receiver<Event>>,
    offer: Option<Update>,
    progress: Arc<Mutex<Progress>>,
    downloading: bool,
    ready: Option<Prepared>,
    failure: bool,
    checked: bool,
}
impl Updater {
    pub fn startup(prefs: &Preferences, ctx: &egui::Context) -> Self {
        let failure = error_path().is_some_and(|p| {
            if let Ok(message) = std::fs::read_to_string(&p) {
                log::error!("Previous update failed: {message}");
                std::fs::remove_file(p).ok();
                true
            } else {
                false
            }
        });
        let mut updater = Self {
            rx: None,
            offer: None,
            progress: Default::default(),
            downloading: false,
            ready: None,
            failure,
            checked: false,
        };
        updater.check(prefs, ctx);
        updater
    }
    fn check(&mut self, prefs: &Preferences, ctx: &egui::Context) {
        if self.rx.is_some()
            || self.checked
            || prefs.disabled
            || availability() != Availability::Enabled
        {
            return;
        }
        self.checked = true;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let prefs = prefs.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut response = client(30)?
                    .get(API)
                    .header("Accept", "application/vnd.github+json")
                    .send()?
                    .error_for_status()?;
                let mut json = String::new();
                response
                    .by_ref()
                    .take(2 * 1024 * 1024)
                    .read_to_string(&mut json)?;
                select(
                    serde_json::from_str(&json)?,
                    env!("CARGO_PKG_VERSION"),
                    &prefs,
                )
            })();
            tx.send(Event::Checked(result)).ok();
            ctx.request_repaint();
        });
    }
    /// Returns true when preferences changed. Restoring either opt-out also
    /// checks immediately, rather than requiring another application restart.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        prefs: &mut Preferences,
        can_restart: bool,
    ) -> bool {
        self.check(prefs, ctx);
        if let Some(event) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.rx = None;
            match event {
                Event::Checked(Ok(update)) => self.offer = update,
                Event::Checked(Err(e)) => log::debug!("Update check: {e:#}"),
                Event::Prepared(result) => {
                    self.downloading = false;
                    match result {
                        Ok(ready) => self.ready = Some(ready),
                        Err(e) => {
                            log::error!("Update: {e:#}");
                            self.failure = true;
                        }
                    }
                }
            }
        }
        if prefs.disabled
            || self
                .offer
                .as_ref()
                .is_some_and(|u| prefs.skipped_version.as_deref() == Some(&u.version))
        {
            self.offer = None;
        }
        let mut choice = None;
        if let Some(update) = &self.offer {
            egui::Window::new(crate::i18n::text("Automatic updates"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(crate::i18n::text(
                        "A new version of ActionLay is available. Update now?",
                    ));
                    ui.label(&update.version);
                    ui.label(crate::i18n::ui_text(
                        ui,
                        "ActionLay will restart after the update.",
                    ));
                    ui.horizontal_wrapped(|ui| {
                        for (index, label) in
                            ["Yes", "Not now", "Skip this version", "Don't ask again"]
                                .iter()
                                .enumerate()
                        {
                            if ui
                                .add_enabled(
                                    index != 0 || can_restart,
                                    egui::Button::new(crate::i18n::text(label)),
                                )
                                .clicked()
                            {
                                choice = Some(index);
                            }
                        }
                    });
                    if !can_restart {
                        ui.small(crate::i18n::ui_text(
                            ui,
                            "Save your layout and finish exporting before updating.",
                        ));
                    }
                });
        }
        let mut changed = false;
        if let Some(choice) = choice {
            let update = self.offer.take().unwrap();
            match choice {
                0 => {
                    self.downloading = true;
                    *self.progress.lock().unwrap() = Progress {
                        bytes: 0,
                        total: update.asset.size,
                    };
                    let progress = self.progress.clone();
                    let (tx, rx) = mpsc::channel();
                    self.rx = Some(rx);
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        tx.send(Event::Prepared(prepare(&update.asset, &progress, &ctx)))
                            .ok();
                        ctx.request_repaint();
                    });
                }
                2 => {
                    prefs.skipped_version = Some(update.version);
                    changed = true;
                }
                3 => {
                    prefs.disabled = true;
                    changed = true;
                }
                _ => {}
            }
        }
        if self.downloading || self.ready.is_some() {
            egui::Window::new(crate::i18n::text("Automatic updates"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let p = self.progress.lock().unwrap();
                    ui.label(crate::i18n::text("Downloading update…"));
                    ui.add(
                        egui::ProgressBar::new(p.bytes as f32 / p.total.max(1) as f32)
                            .show_percentage(),
                    );
                    ui.label(format!(
                        "{:.1} / {:.1} MiB",
                        p.bytes as f64 / 1048576.0,
                        p.total as f64 / 1048576.0
                    ));
                    if p.bytes == p.total {
                        ui.label(crate::i18n::text("Preparing update…"));
                    }
                    if self.ready.is_some() && !can_restart {
                        ui.label(crate::i18n::ui_text(
                            ui,
                            "Save your layout and finish exporting before updating.",
                        ));
                    }
                });
        }
        if self.ready.is_some() && can_restart {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.failure {
            egui::Window::new(crate::i18n::text("Automatic updates"))
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(crate::i18n::ui_text(
                        ui,
                        "The update failed. Please try again later.",
                    ));
                    if ui.button(crate::i18n::text("Close")).clicked() {
                        self.failure = false;
                    }
                });
        }
        changed
    }
    pub fn preferences_changed(&mut self) {
        if !self.downloading && self.ready.is_none() {
            self.checked = false;
        }
    }
    pub fn on_exit(&mut self) {
        if let Some(ready) = self.ready.take()
            && let Err(e) = ready.launch()
        {
            log::error!("Update helper: {e:#}");
            if let Some(path) = error_path() {
                std::fs::write(path, e.to_string()).ok();
            }
        }
    }
}

fn error_path() -> Option<PathBuf> {
    crate::prefs::default_path().map(|p| p.with_file_name("update-error.txt"))
}

struct Prepared {
    directory: tempfile::TempDir,
    platform: PlatformPrepared,
}
struct PlatformPrepared {
    script: PathBuf,
    staged: Option<tempfile::TempPath>,
}
impl Prepared {
    fn launch(self) -> Result<()> {
        #[cfg(unix)]
        let mut command = std::process::Command::new("/bin/sh");
        #[cfg(target_os = "windows")]
        let mut command = {
            use std::os::windows::process::CommandExt;
            let mut cmd = std::process::Command::new("powershell.exe");
            cmd.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ]);
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW; helper stays unelevated.
            cmd
        };
        command
            .arg(&self.platform.script)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        // The helper owns cleanup after the application exits.
        let _ = self.directory.keep();
        if let Some(staged) = self.platform.staged {
            staged.keep()?;
        }
        Ok(())
    }
}

fn prepare(asset: &Asset, progress: &Mutex<Progress>, ctx: &egui::Context) -> Result<Prepared> {
    let directory = tempfile::Builder::new()
        .prefix("actionlay-update-")
        .tempdir()?;
    let download = directory.path().join(&asset.name);
    let mut source = client(3600)?
        .get(&asset.browser_download_url)
        .send()?
        .error_for_status()?;
    let mut file = std::fs::File::create(&download)?;
    download_verified(&mut source, &mut file, asset, progress, || {
        ctx.request_repaint()
    })?;
    let exe = std::env::current_exe()?;
    let error = error_path().context("Missing preferences directory")?;
    std::fs::create_dir_all(error.parent().unwrap())?;
    let platform = prepare_platform(directory.path(), &download, &exe, &error)?;
    Ok(Prepared {
        directory,
        platform,
    })
}

fn download_verified(
    source: &mut impl Read,
    file: &mut std::fs::File,
    asset: &Asset,
    progress: &Mutex<Progress>,
    notify: impl Fn(),
) -> Result<()> {
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0; 128 * 1024];
    loop {
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        ensure!(bytes <= asset.size, "Download exceeds release size");
        file.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
        progress.lock().unwrap().bytes = bytes;
        notify();
    }
    file.sync_all()?;
    ensure!(bytes == asset.size, "Incomplete download");
    ensure!(
        Some(format!("sha256:{:x}", hash.finalize())).as_deref() == asset.digest.as_deref(),
        "Release checksum mismatch"
    );
    Ok(())
}

#[cfg(unix)]
fn sh(value: &Path) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "'\\''"))
}
#[cfg(target_os = "macos")]
fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<()> {
    let output = std::process::Command::new(program).args(args).output()?;
    ensure!(
        output.status.success(),
        "{program}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn prepare_platform(
    dir: &Path,
    download: &Path,
    exe: &Path,
    error: &Path,
) -> Result<PlatformPrepared> {
    ensure!(!system_managed(exe), "System-managed installation");
    // Extract ONLY the executable; no archive-controlled paths are written.
    let output = std::process::Command::new("tar")
        .args(["-xOf"])
        .arg(download)
        .arg("ActionLay/actionlay")
        .output()?;
    ensure!(
        output.status.success() && output.stdout.starts_with(b"\x7fELF"),
        "Invalid Linux package"
    );
    stage_executable(dir, &output.stdout, exe, error)
}

#[cfg(unix)]
fn stage_executable(
    dir: &Path,
    bytes: &[u8],
    exe: &Path,
    error: &Path,
) -> Result<PlatformPrepared> {
    use std::os::unix::fs::PermissionsExt;
    // Stage in the same filesystem, guaranteeing an atomic rename.
    let mut stage = tempfile::NamedTempFile::new_in(exe.parent().context("Executable directory")?)?;
    stage.write_all(bytes)?;
    stage
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o755))?;
    stage.as_file().sync_all()?;
    let stage = stage.into_temp_path();
    let script = dir.join("install.sh");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
while kill -0 {pid} 2>/dev/null; do sleep 1; done
if mv -f {stage} {exe}; then
    {exe} >/dev/null 2>&1 &
else
    printf '%s\n' 'Executable replacement failed' > {error}
    rm -f {stage}
    {exe} >/dev/null 2>&1 &
fi
rm -rf {dir}
"#,
            pid = std::process::id(),
            stage = sh(&stage),
            exe = sh(exe),
            error = sh(error),
            dir = sh(dir)
        ),
    )?;
    Ok(PlatformPrepared {
        script,
        staged: Some(stage),
    })
}

#[cfg(target_os = "macos")]
fn prepare_platform(
    dir: &Path,
    download: &Path,
    exe: &Path,
    error: &Path,
) -> Result<PlatformPrepared> {
    let app = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"));
    let mount = dir.join("mount");
    std::fs::create_dir(&mount)?;
    run(
        "/usr/bin/hdiutil",
        &[
            "attach".as_ref(),
            "-readonly".as_ref(),
            "-nobrowse".as_ref(),
            "-mountpoint".as_ref(),
            mount.as_os_str(),
            download.as_os_str(),
        ],
    )?;
    let staged = dir.join("ActionLay.app");
    let copied = run(
        "/usr/bin/ditto",
        &[mount.join("ActionLay.app").as_os_str(), staged.as_os_str()],
    );
    let detached = run("/usr/bin/hdiutil", &["detach".as_ref(), mount.as_os_str()]);
    copied?;
    detached?;
    ensure!(
        staged.join("Contents/MacOS/actionlay").is_file(),
        "Invalid application bundle"
    );
    let Some(app) = app else {
        let bytes = std::fs::read(staged.join("Contents/MacOS/actionlay"))?;
        return stage_executable(dir, &bytes, exe, error);
    };
    // Copy to a sibling then swap, keeping the previous bundle for rollback.
    let incoming = app.with_file_name(format!(
        ".{}-incoming.app",
        dir.file_name().unwrap().to_string_lossy()
    ));
    let backup = app.with_file_name(format!(
        ".{}-backup.app",
        dir.file_name().unwrap().to_string_lossy()
    ));
    let install = dir.join("replace.sh");
    std::fs::write(
        &install,
        bundle_replacement(&staged, app, &incoming, &backup),
    )?;
    let apple = dir.join("elevate.applescript");
    let shell = format!("/bin/sh {}", sh(&install));
    let apple_literal = shell.replace('\\', "\\\\").replace('"', "\\\"");
    std::fs::write(
        &apple,
        format!("do shell script \"{apple_literal}\" with administrator privileges\n"),
    )?;
    let script = dir.join("install.sh");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
while kill -0 {pid} 2>/dev/null; do sleep 1; done
if [ -w {app} ] && [ -w {parent} ]; then
    /bin/sh {install}
else
    /usr/bin/osascript {apple}
fi
result=$?
if [ "$result" -ne 0 ]; then printf '%s\n' 'Application replacement failed or authorization cancelled' > {error}; fi
/usr/bin/open {app}
rm -rf {dir}
"#,
            pid = std::process::id(),
            app = sh(app),
            parent = sh(app.parent().unwrap()),
            install = sh(&install),
            apple = sh(&apple),
            error = sh(error),
            dir = sh(dir)
        ),
    )?;
    Ok(PlatformPrepared {
        script,
        staged: None,
    })
}

#[cfg(target_os = "macos")]
fn bundle_replacement(staged: &Path, app: &Path, incoming: &Path, backup: &Path) -> String {
    format!(
        r#"#!/bin/sh
set -e
/usr/bin/ditto {staged} {incoming}
mv {app} {backup}
if mv {incoming} {app}; then
    rm -rf {backup}
else
    mv {backup} {app}
    exit 1
fi
"#,
        staged = sh(staged),
        incoming = sh(incoming),
        app = sh(app),
        backup = sh(backup)
    )
}

#[cfg(target_os = "windows")]
fn ps(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}
#[cfg(not(target_os = "windows"))]
fn installation_mode(_exe: &Path) -> Option<bool> {
    None
}
#[cfg(target_os = "windows")]
fn installation_mode(exe: &Path) -> Option<bool> {
    use winreg::{RegKey, enums::*};
    let key = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{B7B4CFBE-D778-4FB8-A976-84E90F770ACF}_is1";
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .find_map(|hive| {
            let location: String = RegKey::predef(hive)
                .open_subkey_with_flags(key, KEY_READ | KEY_WOW64_64KEY)
                .ok()?
                .get_value("InstallLocation")
                .ok()?;
            let installed =
                std::fs::canonicalize(Path::new(&location).join("actionlay.exe")).ok()?;
            (std::fs::canonicalize(exe).ok().as_ref() == Some(&installed))
                .then_some(hive == HKEY_LOCAL_MACHINE)
        })
}
#[cfg(target_os = "windows")]
fn prepare_platform(
    dir: &Path,
    download: &Path,
    exe: &Path,
    error: &Path,
) -> Result<PlatformPrepared> {
    let Some(machine) = installation_mode(exe) else {
        return prepare_windows_portable(dir, download, exe, error);
    };
    let mode = if machine { "/ALLUSERS" } else { "/CURRENTUSER" };
    let script = dir.join("install.ps1");
    let target = exe.parent().context("Installation directory")?;
    let parameters = format!(
        "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /NOCLOSEAPPLICATIONS /NORESTARTAPPLICATIONS {mode} /DIR=\"{}\"",
        target.display()
    );
    std::fs::write(
        &script,
        format!(
            concat!(
                "\u{feff}",
                r#"$ErrorActionPreference = 'Stop'
try {{
    Wait-Process -Id {pid} -ErrorAction SilentlyContinue
    $installer = Start-Process -FilePath {download} -ArgumentList {parameters} {verb} -PassThru -Wait
    if ($installer.ExitCode -ne 0) {{ throw "Installer failed: $($installer.ExitCode)" }}
}} catch {{
    [IO.File]::WriteAllText({error}, $_.ToString())
}} finally {{
    Start-Process -FilePath {exe}
    Remove-Item -LiteralPath {dir} -Recurse -Force -ErrorAction SilentlyContinue
}}
"#
            ),
            pid = std::process::id(),
            download = ps(download),
            parameters = ps(Path::new(&parameters)),
            verb = if machine { "-Verb RunAs" } else { "" },
            error = ps(error),
            exe = ps(exe),
            dir = ps(dir)
        ),
    )?;
    Ok(PlatformPrepared {
        script,
        staged: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(version: &str) -> Release {
        Release {
            tag_name: format!("v{version}"),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: asset_name(version),
                browser_download_url: format!(
                    "https://github.com/porech/actionlay/releases/download/v{version}/{}",
                    asset_name(version)
                ),
                size: 100,
                digest: Some(format!("sha256:{}", "a".repeat(64))),
            }],
        }
    }
    #[test]
    fn only_new_stable_versions_are_offered() {
        let prefs = Preferences::default();
        assert!(
            select(release("1.10.0"), "1.9.0", &prefs)
                .unwrap()
                .is_some()
        );
        for version in ["1.8.0", "1.9.0", "1.10.0-beta.1", "1.10.0+dev"] {
            assert!(select(release(version), "1.9.0", &prefs).unwrap().is_none());
        }
        let mut nightly = release("1.10.0");
        nightly.prerelease = true;
        assert!(select(nightly, "1.9.0", &prefs).unwrap().is_none());
        let mut draft = release("1.10.0");
        draft.draft = true;
        assert!(select(draft, "1.9.0", &prefs).unwrap().is_none());
    }
    #[test]
    fn skip_is_scoped_to_one_version_and_opt_out_is_persistent() {
        let prefs = Preferences {
            disabled: false,
            skipped_version: Some("1.10.0".into()),
        };
        let prefs: Preferences =
            serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
        assert!(
            select(release("1.10.0"), "1.9.0", &prefs)
                .unwrap()
                .is_none()
        );
        assert!(
            select(release("1.11.0"), "1.9.0", &prefs)
                .unwrap()
                .is_some()
        );
        assert!(
            select(
                release("1.11.0"),
                "1.9.0",
                &Preferences {
                    disabled: true,
                    ..prefs
                }
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(
            serde_json::from_str::<Preferences>("{}").unwrap(),
            Preferences::default()
        );
    }
    #[test]
    fn unexpected_assets_are_never_executed() {
        let mut r = release("1.10.0");
        r.assets[0].browser_download_url = "https://example.com/malware".into();
        assert!(select(r, "1.9.0", &Preferences::default()).is_err());
        let mut r = release("1.10.0");
        r.assets[0].digest = None;
        assert!(select(r, "1.9.0", &Preferences::default()).is_err());
        let mut r = release("1.10.0");
        r.assets.clear();
        assert!(
            select(r, "1.9.0", &Preferences::default())
                .unwrap()
                .is_none()
        );
    }
    #[cfg(unix)]
    #[test]
    fn helper_paths_are_shell_quoted() {
        assert_eq!(
            sh(Path::new("/tmp/a'b $(touch nope)")),
            "'/tmp/a'\\''b $(touch nope)'"
        );
    }
}

#[cfg(target_os = "windows")]
fn prepare_windows_portable(
    dir: &Path,
    download: &Path,
    exe: &Path,
    error: &Path,
) -> Result<PlatformPrepared> {
    let staged = dir.join("actionlay.exe");
    // Extract exactly one known entry; never expand archive-controlled paths.
    let extract = format!(
        r#"$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.IO.Compression.FileSystem; $zip=[IO.Compression.ZipFile]::OpenRead({download}); try {{ $entry=$zip.GetEntry('ActionLay/actionlay.exe'); if (!$entry) {{ throw 'Missing executable' }}; [IO.Compression.ZipFileExtensions]::ExtractToFile($entry,{staged}) }} finally {{ $zip.Dispose() }}"#,
        download = ps(download),
        staged = ps(&staged)
    );
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &extract])
        .creation_flags(0x08000000)
        .output()?;
    ensure!(
        out.status.success(),
        "ZIP extraction failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut magic = [0; 2];
    std::fs::File::open(&staged)?.read_exact(&mut magic)?;
    ensure!(&magic == b"MZ", "Invalid Windows executable");
    let script = dir.join("install.ps1");
    let next = exe.with_file_name(format!(
        ".{}-incoming.exe",
        dir.file_name().unwrap().to_string_lossy()
    ));
    let backup = exe.with_file_name(format!(
        ".{}-backup.exe",
        dir.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(
        &script,
        format!(
            concat!(
                "\u{feff}",
                r#"$ErrorActionPreference = 'Stop'
try {{
    Wait-Process -Id {pid} -ErrorAction SilentlyContinue
    Copy-Item -LiteralPath {staged} -Destination {next}
    Move-Item -LiteralPath {exe} -Destination {backup}
    try {{ Move-Item -LiteralPath {next} -Destination {exe} }}
    catch {{ Move-Item -LiteralPath {backup} -Destination {exe}; throw }}
    Remove-Item -LiteralPath {backup} -Force
}} catch {{
    [IO.File]::WriteAllText({error}, $_.ToString())
}} finally {{
    Start-Process -FilePath {exe}
    Remove-Item -LiteralPath {next} -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath {dir} -Recurse -Force -ErrorAction SilentlyContinue
}}
"#
            ),
            pid = std::process::id(),
            staged = ps(&staged),
            next = ps(&next),
            backup = ps(&backup),
            exe = ps(exe),
            error = ps(error),
            dir = ps(dir)
        ),
    )?;
    Ok(PlatformPrepared {
        script,
        staged: None,
    })
}

#[cfg(test)]
mod installation_tests {
    use super::*;
    #[test]
    fn downloads_report_progress_and_reject_corruption_or_truncation() {
        let data = b"a trusted release executable";
        let asset = Asset {
            name: "test".into(),
            browser_download_url: String::new(),
            size: data.len() as u64,
            digest: Some(format!("sha256:{:x}", Sha256::digest(data))),
        };
        let progress = Mutex::new(Progress {
            bytes: 0,
            total: asset.size,
        });
        let mut file = tempfile::tempfile().unwrap();
        download_verified(&mut &data[..], &mut file, &asset, &progress, || {}).unwrap();
        assert_eq!(progress.lock().unwrap().bytes, asset.size);
        for invalid in [
            &data[..data.len() - 1],
            &b"a tampered release executable"[..],
            &b"a trusted release executable plus extra bytes"[..],
        ] {
            let mut file = tempfile::tempfile().unwrap();
            assert!(
                download_verified(&mut &invalid[..], &mut file, &asset, &progress, || {}).is_err()
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn standalone_replacement_is_atomic_and_restarts_from_original_path() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("a user's standalone directory");
        std::fs::create_dir(&parent).unwrap();
        let exe = parent.join("actionlay");
        std::fs::write(&exe, "old executable").unwrap();
        let helper = root.path().join("helper");
        std::fs::create_dir(&helper).unwrap();
        let sentinel = root.path().join("restarted");
        let bytes = format!("#!/bin/sh\nprintf updated > {}\n", sh(&sentinel));
        let platform =
            stage_executable(&helper, bytes.as_bytes(), &exe, &root.path().join("error")).unwrap();
        let script = std::fs::read_to_string(&platform.script).unwrap().replace(
            &format!("kill -0 {}", std::process::id()),
            "kill -0 2147483647",
        );
        assert!(
            std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), bytes);
        for _ in 0..100 {
            if sentinel.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "updated");
        assert!(!root.path().join("error").exists());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn bundle_is_replaced_in_its_actual_location_with_spaces_and_quotes() {
        let root = tempfile::tempdir().unwrap();
        let app = root
            .path()
            .join("custom user's location/renamed ActionLay.app");
        let staged = root.path().join("downloaded.app");
        for path in [&app, &staged] {
            std::fs::create_dir_all(path.join("Contents/MacOS")).unwrap();
        }
        std::fs::write(app.join("Contents/MacOS/actionlay"), "old").unwrap();
        std::fs::write(staged.join("Contents/MacOS/actionlay"), "new").unwrap();
        let incoming = app.with_file_name("incoming.app");
        let backup = app.with_file_name("backup.app");
        let script = bundle_replacement(&staged, &app, &incoming, &backup);
        assert!(
            std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            std::fs::read_to_string(app.join("Contents/MacOS/actionlay")).unwrap(),
            "new"
        );
        assert!(!backup.exists());
        assert!(!incoming.exists());
    }
}
