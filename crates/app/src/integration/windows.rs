use std::io;
use std::path::Path;
use winreg::{RegKey, enums::*};

const PROG_ID: &str = "ActionLay.Video";
const INSTALLED_PROG_ID: &str = "ActionLay.InstalledVideo";
const EXTENSIONS: &[&str] = &[".mp4", ".mov", ".lrv", ".insv"];

fn classes() -> io::Result<RegKey> {
    RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(r"Software\Classes")
        .map(|(key, _)| key)
}

fn install(root: &RegKey, executable: &Path) -> io::Result<()> {
    let path = executable
        .to_str()
        .ok_or_else(|| io::Error::other("executable path is not Unicode"))?;
    if path.contains(['"', '\0']) {
        return Err(io::Error::other("invalid executable path"));
    }
    let (prog, _) = root.create_subkey(PROG_ID)?;
    prog.set_value("", &"ActionLay video")?;
    prog.create_subkey("Application")?
        .0
        .set_value("ApplicationName", &"ActionLay")?;
    prog.create_subkey("DefaultIcon")?
        .0
        .set_value("", &format!("\"{path}\",0"))?;
    prog.create_subkey(r"shell\open\command")?
        .0
        .set_value("", &format!("\"{path}\" \"%1\""))?;
    for extension in EXTENSIONS {
        root.create_subkey(format!(r"{extension}\OpenWithProgids"))?
            .0
            .set_value(PROG_ID, &"")?;
    }
    Ok(())
}

fn uninstall(root: &RegKey) -> io::Result<()> {
    for extension in EXTENSIONS {
        match root.open_subkey_with_flags(format!(r"{extension}\OpenWithProgids"), KEY_WRITE) {
            Ok(key) => match key.delete_value(PROG_ID) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            },
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    match root.delete_subkey_all(PROG_ID) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn notify_shell() {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHChangeNotify(
            event: i32,
            flags: u32,
            item1: *const std::ffi::c_void,
            item2: *const std::ffi::c_void,
        );
    }
    // SAFETY: SHCNE_ASSOCCHANGED with SHCNF_IDLIST takes two null item pointers.
    unsafe {
        SHChangeNotify(0x0800_0000, 0, std::ptr::null(), std::ptr::null());
    }
}

pub fn registered() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(format!(r"Software\Classes\{PROG_ID}\shell\open\command"))
        .and_then(|key| key.get_value::<String, _>(""))
        .is_ok()
}

pub fn managed() -> bool {
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .any(|hive| {
            RegKey::predef(hive)
                .open_subkey_with_flags(
                    format!(r"Software\Classes\{INSTALLED_PROG_ID}\shell\open\command"),
                    KEY_READ | KEY_WOW64_64KEY,
                )
                .and_then(|key| key.get_value::<String, _>(""))
                .is_ok()
        })
}

pub fn register() -> io::Result<()> {
    install(&classes()?, &std::env::current_exe()?)?;
    notify_shell();
    Ok(())
}

pub fn remove() -> io::Result<()> {
    uninstall(&classes()?)?;
    notify_shell();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_preserves_defaults_and_unrelated_handlers() {
        // Isolated registry tree: never touch the user's actual file associations.
        let name = format!(r"Software\ActionLayTest\{}", std::process::id());
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (root, _) = hkcu.create_subkey(&name).unwrap();
        let extension = root.create_subkey(".mp4").unwrap().0;
        extension.set_value("", &"Other.Video").unwrap();
        extension
            .create_subkey("OpenWithProgids")
            .unwrap()
            .0
            .set_value("Other.Video", &"")
            .unwrap();
        install(&root, Path::new(r"C:\Action Lay\actionlay.exe")).unwrap();
        let command = root
            .open_subkey(format!(r"{PROG_ID}\shell\open\command"))
            .unwrap();
        assert_eq!(
            command.get_value::<String, _>("").unwrap(),
            r#""C:\Action Lay\actionlay.exe" "%1""#
        );
        install(&root, Path::new(r"D:\moved\actionlay.exe")).unwrap();
        assert_eq!(
            command.get_value::<String, _>("").unwrap(),
            r#""D:\moved\actionlay.exe" "%1""#
        );
        drop(command);
        uninstall(&root).unwrap();
        uninstall(&root).unwrap();
        assert_eq!(extension.get_value::<String, _>("").unwrap(), "Other.Video");
        assert!(
            extension
                .open_subkey("OpenWithProgids")
                .unwrap()
                .get_value::<String, _>("Other.Video")
                .is_ok()
        );
        assert!(root.open_subkey(PROG_ID).is_err());
        drop(extension);
        drop(root);
        hkcu.delete_subkey_all(&name).unwrap();
    }
}
