//! Native games installed as desktop entries (`Categories=…Game…`), minus
//! launchers and shortcuts for games we already list from Steam, Lutris or
//! Heroic.

use super::{Game, Launch, Source};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub exec: String,
    pub icon: Option<String>,
    pub categories: Vec<String>,
    pub path: Option<String>,
    pub hidden: bool,
}

/// Parse the `[Desktop Entry]` group (other groups and localized keys are
/// ignored).
pub fn parse(id: &str, text: &str) -> Entry {
    let mut e = Entry {
        id: id.to_string(),
        ..Default::default()
    };
    let mut in_main = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "Name" => e.name = v.to_string(),
            "Exec" => e.exec = v.to_string(),
            "Icon" => e.icon = Some(v.to_string()),
            "Path" => e.path = Some(v.to_string()).filter(|s| !s.is_empty()),
            "Categories" => e.categories = v.split(';').filter(|c| !c.is_empty()).map(str::to_string).collect(),
            "NoDisplay" | "Hidden" if v == "true" => e.hidden = true,
            "Type" if v != "Application" => e.hidden = true,
            _ => {}
        }
    }
    e
}

/// Desktop ids of game launchers themselves (they declare `Game` too).
const LAUNCHERS: [&str; 9] = [
    "steam",
    "net.lutris.lutris",
    "lutris",
    "com.heroicgameslauncher.hgl",
    "heroic",
    "com.valvesoftware.steam",
    "material_3_games",
    "itch",
    "io.itch.itch",
];

/// Is this entry a game we should list on its own?
pub fn is_game(e: &Entry) -> bool {
    if e.hidden || e.name.is_empty() || e.exec.is_empty() {
        return false;
    }
    if !e.categories.iter().any(|c| c == "Game") {
        return false;
    }
    if LAUNCHERS.contains(&e.id.to_lowercase().as_str()) {
        return false;
    }
    let exec = e.exec.to_lowercase();
    // Shortcuts into stores we already read.
    !(exec.contains("steam://rungameid")
        || exec.contains("lutris:rungameid")
        || exec.contains("lutris:rungame")
        || exec.contains("heroic://"))
}

/// Split an Exec line into argv, honoring double quotes and dropping field
/// codes (`%U`, `%f`, …).
pub fn split_exec(exec: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut has = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ' ' | '\t' if !quoted => {
                if has || !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            _ => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        args.push(cur);
    }
    args.into_iter()
        .filter(|a| !(a.len() == 2 && a.starts_with('%')))
        .map(|a| a.replace("%%", "%"))
        .collect()
}

fn app_dirs(home: &Path) -> Vec<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let mut dirs = vec![data_home.join("applications")];
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').filter(|s| !s.is_empty()).map(|d| Path::new(d).join("applications")));
    dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs
}

fn collect(dir: &Path, prefix: &str, out: &mut HashMap<String, PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            collect(&p, &format!("{prefix}{name}-"), out);
        } else if let Some(stem) = name.strip_suffix(".desktop") {
            // Earlier directories take precedence.
            out.entry(format!("{prefix}{stem}")).or_insert(p);
        }
    }
}

/// Resolve a themed icon name to a file, best effort (no full icon theme
/// lookup: hicolor and pixmaps only).
fn icon_path(icon: &str) -> Option<PathBuf> {
    if icon.starts_with('/') {
        return Some(PathBuf::from(icon)).filter(|p| p.is_file());
    }
    let sizes = ["512x512", "256x256", "128x128"];
    for base in ["/usr/share/icons/hicolor", "/usr/local/share/icons/hicolor"] {
        for s in sizes {
            let p = Path::new(base).join(s).join("apps").join(format!("{icon}.png"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let p = Path::new("/usr/share/pixmaps").join(format!("{icon}.png"));
    p.is_file().then_some(p)
}

pub fn scan(home: &Path) -> Vec<Game> {
    let mut files = HashMap::new();
    for dir in app_dirs(home) {
        collect(&dir, "", &mut files);
    }
    let mut games: Vec<Game> = files
        .into_iter()
        .filter_map(|(id, path)| {
            let e = parse(&id, &std::fs::read_to_string(&path).ok()?);
            is_game(&e).then_some(e)
        })
        .map(|e| Game {
            id: format!("desktop:{}", e.id),
            cover: e.icon.as_deref().and_then(icon_path),
            hero: None,
            cover_url: None,
            last_played: None,
            playtime_minutes: None,
            installed: true,
            install_dir: e.path.as_ref().map(PathBuf::from),
            launch: Launch::Command {
                argv: split_exec(&e.exec),
                dir: e.path.as_ref().map(PathBuf::from),
            },
            name: e.name,
            source: Source::Desktop,
        })
        .collect();
    games.sort_by(|a, b| a.id.cmp(&b.id));
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtering() {
        let game = parse(
            "supertuxkart",
            "[Desktop Entry]\nType=Application\nName=SuperTuxKart\nName[de]=X\nExec=supertuxkart %U\nCategories=Game;ArcadeGame;\n[Desktop Action new]\nName=Other\n",
        );
        assert_eq!(game.name, "SuperTuxKart");
        assert!(is_game(&game));
        let steam_shortcut = parse("Portal", "[Desktop Entry]\nName=Portal\nExec=steam steam://rungameid/400\nCategories=Game;\n");
        assert!(!is_game(&steam_shortcut));
        let launcher = parse("steam", "[Desktop Entry]\nName=Steam\nExec=steam %U\nCategories=Network;Game;\n");
        assert!(!is_game(&launcher));
        let hidden = parse("x", "[Desktop Entry]\nName=X\nExec=x\nCategories=Game;\nNoDisplay=true\n");
        assert!(!is_game(&hidden));
        let not_game = parse("gimp", "[Desktop Entry]\nName=GIMP\nExec=gimp\nCategories=Graphics;\n");
        assert!(!is_game(&not_game));
    }

    #[test]
    fn exec_splitting() {
        assert_eq!(split_exec("supertuxkart %U"), ["supertuxkart"]);
        assert_eq!(
            split_exec(r#"env WINEPREFIX="/home/u/My Games" wine "C:\\game.exe" --x 100%%"#),
            ["env", "WINEPREFIX=/home/u/My Games", "wine", "C:\\game.exe", "--x", "100%"]
        );
        assert_eq!(split_exec(r#"run """#), ["run", ""]);
    }
}
