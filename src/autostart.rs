/// Starts bawkbit in the tray instead of opening its window.
pub const HIDDEN: &str = "--hidden";

pub fn supported() -> bool {
    cfg!(any(windows, target_os = "linux"))
}

#[cfg(windows)]
mod imp {
    use windows_registry::CURRENT_USER;

    use super::HIDDEN;

    pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "bawkbit";

    fn command() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        Some(format!("\"{}\" {HIDDEN}", exe.display()))
    }

    fn read(key: &str) -> Option<String> {
        CURRENT_USER.open(key).ok()?.get_string(VALUE).ok()
    }

    /// True only when the entry starts this copy; one left by a copy that has since moved counts as off.
    pub fn enabled_at(key: &str) -> bool {
        read(key)
            .zip(command())
            .is_some_and(|(entry, command)| entry.eq_ignore_ascii_case(&command))
    }

    pub fn set_at(key: &str, on: bool) -> Result<(), String> {
        let run = CURRENT_USER
            .create(key)
            .map_err(|err| format!("cannot open the startup list: {err}"))?;
        if on {
            let command = command().ok_or("cannot find bawkbit's own path")?;
            run.set_string(VALUE, command)
                .map_err(|err| format!("cannot add bawkbit to the startup list: {err}"))
        } else if read(key).is_some() {
            run.remove_value(VALUE)
                .map_err(|err| format!("cannot take bawkbit off the startup list: {err}"))
        } else {
            Ok(())
        }
    }

    pub fn enabled() -> bool {
        enabled_at(RUN_KEY)
    }

    pub fn set(on: bool) -> Result<(), String> {
        set_at(RUN_KEY, on)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::{Path, PathBuf};

    use super::HIDDEN;

    fn file() -> Option<PathBuf> {
        Some(
            dirs::config_dir()?
                .join("autostart")
                .join("bawkbit.desktop"),
        )
    }

    /// The AppImage itself when running from one, since its mounted copy moves every launch.
    fn program() -> Option<PathBuf> {
        std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
    }

    /// Quotes a path for an Exec line, escaping as the desktop entry spec asks.
    pub fn exec_quote(path: &Path) -> String {
        let text = path.to_string_lossy();
        let plain = text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%".contains(c));
        if plain {
            return text.into_owned();
        }
        let mut quoted = String::from("\"");
        for c in text.chars() {
            match c {
                '\\' => quoted.push_str(r"\\\\"),
                '"' | '`' | '$' => {
                    quoted.push_str(r"\\");
                    quoted.push(c);
                }
                c => quoted.push(c),
            }
        }
        quoted.push('"');
        quoted
    }

    pub fn entry(program: &Path) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName=bawkbit\nExec={} {HIDDEN}\nIcon=bawkbit\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            exec_quote(program)
        )
    }

    pub fn enabled() -> bool {
        let (Some(file), Some(program)) = (file(), program()) else {
            return false;
        };
        std::fs::read_to_string(file).is_ok_and(|text| text == entry(&program))
    }

    pub fn set(on: bool) -> Result<(), String> {
        let file = file().ok_or("cannot find the autostart folder")?;
        if !on {
            return match std::fs::remove_file(&file) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("cannot remove {}: {err}", file.display()))
                }
                _ => Ok(()),
            };
        }
        let program = program().ok_or("cannot find bawkbit's own path")?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
        }
        std::fs::write(&file, entry(&program))
            .map_err(|err| format!("cannot write {}: {err}", file.display()))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    pub fn enabled() -> bool {
        false
    }

    pub fn set(_on: bool) -> Result<(), String> {
        Err("starting at login is not available here".into())
    }
}

pub use imp::{enabled, set};

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn the_startup_entry_is_written_read_and_removed() {
        use super::imp::{enabled_at, set_at};
        let key = format!(r"Software\bawkbit-test-{}", std::process::id());
        assert!(!enabled_at(&key));
        set_at(&key, true).unwrap();
        assert!(enabled_at(&key));
        set_at(&key, false).unwrap();
        assert!(!enabled_at(&key));
        set_at(&key, false).unwrap();
        windows_registry::CURRENT_USER.remove_tree(&key).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn exec_lines_quote_paths_with_spaces() {
        use super::imp::{entry, exec_quote};
        use std::path::Path;
        assert_eq!(
            exec_quote(Path::new("/usr/bin/bawkbit")),
            "/usr/bin/bawkbit"
        );
        assert_eq!(
            exec_quote(Path::new("/home/a b/bawkbit $1.AppImage")),
            r#""/home/a b/bawkbit \\$1.AppImage""#
        );
        assert!(entry(Path::new("/opt/bawkbit")).contains("Exec=/opt/bawkbit --hidden\n"));
    }
}
