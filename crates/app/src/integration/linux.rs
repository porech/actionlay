use std::io;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::process::Command;

const DESKTOP: &str = "org.ActionLay.ActionLay.desktop";
const MIME_FILE: &str = "actionlay-camera-video.xml";
const MIME_TYPES: &str = "video/mp4;video/quicktime;video/x-actionlay-lrv;video/x-actionlay-insv;";
const MIME_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="video/x-actionlay-lrv">
    <comment>Action camera low-resolution video</comment>
    <sub-class-of type="video/mp4"/>
    <glob pattern="*.lrv"/><glob pattern="*.LRV"/>
  </mime-type>
  <mime-type type="video/x-actionlay-insv">
    <comment>Insta360 video</comment>
    <sub-class-of type="video/mp4"/>
    <glob pattern="*.insv"/><glob pattern="*.INSV"/>
  </mime-type>
</mime-info>
"#;

#[cfg(target_os = "linux")]
fn data_dir() -> io::Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.data_dir().to_path_buf())
        .ok_or_else(|| io::Error::other("cannot locate the user data directory"))
}

// Desktop Exec parsing is not shell parsing. Escape both the quoted argument
// grammar and the desktop-entry string grammar, and protect percent field codes.
fn desktop_quote(path: &Path) -> io::Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::other("executable path is not UTF-8"))?;
    if path.contains(['\n', '\r', '\0']) {
        return Err(io::Error::other(
            "unsupported control character in executable path",
        ));
    }
    let mut quoted = String::from("\"");
    for ch in path.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\\\\\"),
            '"' => quoted.push_str("\\\\\""),
            '$' => quoted.push_str("\\\\$"),
            '`' => quoted.push_str("\\\\`"),
            '%' => quoted.push_str("%%"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

fn desktop_entry(executable: &Path, icon: &Path) -> io::Result<String> {
    let icon = icon
        .to_str()
        .ok_or_else(|| io::Error::other("icon path is not UTF-8"))?
        .replace('\\', "\\\\");
    if icon.contains(['\n', '\r', '\0']) {
        return Err(io::Error::other(
            "unsupported control character in icon path",
        ));
    }
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=ActionLay\nComment=Action-camera telemetry dashboards\nExec={} %f\nIcon={icon}\nTerminal=false\nCategories=AudioVideo;Video;\nMimeType={MIME_TYPES}\n",
        desktop_quote(executable)?
    ))
}

fn install(data: &Path, executable: &Path) -> io::Result<()> {
    let applications = data.join("applications");
    let packages = data.join("mime/packages");
    let icon = data.join("icons/hicolor/256x256/apps/actionlay.png");
    let entry = desktop_entry(executable, &icon)?;
    std::fs::create_dir_all(&applications)?;
    std::fs::create_dir_all(&packages)?;
    std::fs::create_dir_all(icon.parent().unwrap())?;
    std::fs::write(
        &icon,
        include_bytes!("../../../../assets/icons/actionlay-256.png"),
    )?;
    std::fs::write(packages.join(MIME_FILE), MIME_XML)?;
    std::fs::write(applications.join(DESKTOP), entry)?;
    Ok(())
}

fn uninstall(data: &Path) -> io::Result<()> {
    for relative in [
        format!("applications/{DESKTOP}"),
        format!("mime/packages/{MIME_FILE}"),
        "icons/hicolor/256x256/apps/actionlay.png".into(),
    ] {
        match std::fs::remove_file(data.join(relative)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn refresh(data: &Path) {
    for (tool, directory) in [
        ("update-mime-database", "mime"),
        ("update-desktop-database", "applications"),
    ] {
        if let Err(error) = Command::new(tool).arg(data.join(directory)).status() {
            log::warn!("{tool}: {error}; the desktop may need a new login to refresh Open With");
        }
    }
}

#[cfg(target_os = "linux")]
pub fn registered() -> bool {
    data_dir().is_ok_and(|d| d.join("applications").join(DESKTOP).is_file())
}

#[cfg(target_os = "linux")]
pub fn managed() -> bool {
    let mut roots: Vec<PathBuf> = std::env::var_os("XDG_DATA_DIRS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
    if let Ok(user) = data_dir() {
        roots.push(user);
    }
    roots.into_iter().any(|root| {
        std::fs::read_to_string(root.join("applications").join(DESKTOP))
            .is_ok_and(|entry| entry.lines().any(|line| line == "X-ActionLay-Managed=true"))
    })
}

#[cfg(target_os = "linux")]
pub fn register() -> io::Result<()> {
    let data = data_dir()?;
    install(&data, &std::env::current_exe()?)?;
    refresh(&data);
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn remove() -> io::Result<()> {
    let data = data_dir()?;
    uninstall(&data)?;
    refresh(&data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_preserves_defaults_and_other_applications_and_updates_path() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path();
        std::fs::create_dir_all(data.join("applications")).unwrap();
        let defaults = "[Default Applications]\nvideo/mp4=other.desktop;\n";
        std::fs::write(data.join("applications/mimeapps.list"), defaults).unwrap();
        std::fs::write(data.join("applications/other.desktop"), "other application").unwrap();
        install(data, Path::new("/opt/Action Lay/actionlay")).unwrap();
        let desktop = data.join("applications").join(DESKTOP);
        assert!(
            std::fs::read_to_string(&desktop)
                .unwrap()
                .contains("Exec=\"/opt/Action Lay/actionlay\" %f")
        );
        install(data, Path::new("/new/actionlay")).unwrap();
        assert!(
            std::fs::read_to_string(&desktop)
                .unwrap()
                .contains("Exec=\"/new/actionlay\" %f")
        );
        uninstall(data).unwrap();
        uninstall(data).unwrap();
        assert!(!desktop.exists());
        assert_eq!(
            std::fs::read_to_string(data.join("applications/mimeapps.list")).unwrap(),
            defaults
        );
        assert_eq!(
            std::fs::read_to_string(data.join("applications/other.desktop")).unwrap(),
            "other application"
        );
    }

    #[test]
    fn exec_paths_escape_reserved_characters_and_field_codes() {
        assert_eq!(
            desktop_quote(Path::new("/opt/a%f/$b`c\"d\\e")).unwrap(),
            "\"/opt/a%%f/\\\\$b\\\\`c\\\\\"d\\\\\\\\e\""
        );
        assert!(desktop_quote(Path::new("/bad\npath")).is_err());
    }
}
