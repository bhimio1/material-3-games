//! The distribution we run on (`/etc/os-release`), installed pacman
//! packages, and the command line that installs missing ones.

use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OsRelease {
    pub id: String,
    pub id_like: String,
    pub pretty_name: String,
}

impl OsRelease {
    pub fn load() -> Self {
        let text = std::fs::read_to_string("/etc/os-release")
            .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
            .unwrap_or_default();
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Self {
        let mut os = OsRelease::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            match k.trim() {
                "ID" => os.id = v,
                "ID_LIKE" => os.id_like = v,
                "PRETTY_NAME" => os.pretty_name = v,
                _ => {}
            }
        }
        os
    }

    pub fn is_cachyos(&self) -> bool {
        self.id == "cachyos"
    }
}

/// Is `package` installed? Reads pacman's local database
/// (`<name>-<pkgver>-<pkgrel>` folders) instead of running pacman.
pub fn installed(package: &str) -> bool {
    installed_in(Path::new("/var/lib/pacman/local"), package)
}

pub fn installed_in(db: &Path, package: &str) -> bool {
    let Ok(rd) = std::fs::read_dir(db) else {
        return false;
    };
    rd.flatten().any(|e| {
        let folder = e.file_name().to_string_lossy().into_owned();
        // Strip the last two dash-separated fields (version and release).
        let mut parts = folder.rsplitn(3, '-');
        let (_rel, _ver, name) = (parts.next(), parts.next(), parts.next());
        name == Some(package)
    })
}

/// A POSIX `sh` command line that installs `packages`: those in the
/// configured repositories (CachyOS's included) with pacman, the rest from
/// the AUR with paru, else yay. The split is decided when it runs.
pub fn install_script(packages: &[String]) -> String {
    let list = packages.iter().map(|p| super::term::shell_quote(p)).collect::<Vec<_>>().join(" ");
    format!(
        "repo=; aur=; for p in {list}; do \
if pacman -Si \"$p\" >/dev/null 2>&1; then repo=\"$repo $p\"; else aur=\"$aur $p\"; fi; done; \
( if [ -n \"$repo\" ]; then sudo pacman -S --needed $repo || exit; fi; \
if [ -n \"$aur\" ]; then \
if command -v paru >/dev/null 2>&1; then paru -S --needed $aur; \
elif command -v yay >/dev/null 2>&1; then yay -S --needed $aur; \
else echo \"Not in the repositories and no AUR helper (paru or yay) found:$aur\"; exit 1; fi; fi )"
    )
}

/// CachyOS's gaming meta packages, offered in Settings when missing.
pub const CACHYOS_GAMING: [&str; 2] = ["cachyos-gaming-meta", "cachyos-gaming-applications"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release() {
        let cachy = OsRelease::parse("NAME=\"CachyOS Linux\"\nPRETTY_NAME=\"CachyOS\"\nID=cachyos\nID_LIKE=arch\nBUILD_ID=rolling\n");
        assert!(cachy.is_cachyos());
        assert_eq!(cachy.id_like, "arch");
        assert_eq!(cachy.pretty_name, "CachyOS");
        let arch = OsRelease::parse("NAME=\"Arch Linux\"\nID=arch\n");
        assert!(!arch.is_cachyos());
        let endeavour = OsRelease::parse("ID='endeavouros'\nID_LIKE='arch'\n");
        assert_eq!((endeavour.id.as_str(), endeavour.id_like.as_str()), ("endeavouros", "arch"));
        let deb = OsRelease::parse("ID=debian\nID_LIKE=\"ubuntu debian\"\n# comment\n");
        assert!(!deb.is_cachyos());
        assert_eq!(OsRelease::parse(""), OsRelease::default());
    }

    #[test]
    fn pacman_db() {
        let db = tempfile::tempdir().unwrap();
        for f in ["cachyos-gaming-meta-1-7", "gamescope-3.16.4-1", "lib32-mesa-git-25.0.0_devel.1-1"] {
            std::fs::create_dir(db.path().join(f)).unwrap();
        }
        assert!(installed_in(db.path(), "cachyos-gaming-meta"));
        assert!(installed_in(db.path(), "gamescope"));
        assert!(installed_in(db.path(), "lib32-mesa-git"));
        assert!(!installed_in(db.path(), "cachyos-gaming-applications"));
        assert!(!installed_in(db.path(), "lib32-mesa"));
        assert!(!installed_in(Path::new("/nonexistent"), "gamescope"));
    }

    #[test]
    fn install_command() {
        let s = install_script(&["gamescope".into(), "proton-cachyos".into()]);
        assert!(s.starts_with("repo=; aur=; for p in gamescope proton-cachyos; do"));
        assert!(s.contains("pacman -Si \"$p\" >/dev/null 2>&1"));
        assert!(s.contains("sudo pacman -S --needed $repo"));
        // paru is tried before yay.
        let paru = s.find("command -v paru").unwrap();
        let yay = s.find("command -v yay").unwrap();
        assert!(paru < yay);
        assert!(s.contains("paru -S --needed $aur"));
        // Odd names are quoted.
        assert!(install_script(&["a b".into()]).contains("for p in 'a b'; do"));
    }

    /// Run the script with stub pacman/paru that record their arguments.
    #[test]
    fn install_command_runs() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let log = dir.path().join("log");
        let stub = |name: &str, body: &str| {
            use std::os::unix::fs::PermissionsExt;
            let p = bin.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        // Only "inrepo" is in the repositories.
        stub("pacman", &format!("if [ \"$1\" = -Si ]; then [ \"$2\" = inrepo ]; exit; fi; echo pacman \"$@\" >> {}", log.display()));
        stub("sudo", "exec \"$@\"");
        stub("paru", &format!("echo paru \"$@\" >> {}", log.display()));
        let script = install_script(&["inrepo".into(), "fromaur".into()]);
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let st = std::process::Command::new("sh").arg("-c").arg(&script).env("PATH", path).status().unwrap();
        assert!(st.success());
        let got = std::fs::read_to_string(&log).unwrap();
        assert_eq!(got, "pacman -S --needed inrepo\nparu -S --needed fromaur\n");
    }
}
