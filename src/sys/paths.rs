//! Path helpers: `~` expansion and display.

use std::path::{Path, PathBuf};

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

pub fn config_home() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| home().join(".config"))
}

/// Show a path with the home directory as `~`.
pub fn tilde(path: &Path) -> String {
    let home = home();
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_paths() {
        assert_eq!(tilde(&home().join(".config/x")), "~/.config/x");
        assert_eq!(tilde(Path::new("/etc/x")), "/etc/x");
    }
}

/// Expand a leading `~`.
pub fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return home().join(rest);
    }
    if path == "~" {
        return home();
    }
    PathBuf::from(path)
}
