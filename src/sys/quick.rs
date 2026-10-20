//! Quick settings for the Game Mode overlay: volume and output device
//! (pactl), brightness (brightnessctl), media (playerctl), controller
//! batteries (sysfs) and power. All blocking; call from background tasks.

use super::cmd;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Audio {
    pub volume: u32,
    pub muted: bool,
    pub sinks: Vec<(String, String)>,
    pub default_sink: String,
}

/// "Volume: front-left: 32768 /  50% / -18.06 dB,   front-right: …"
pub fn parse_volume(text: &str) -> Option<u32> {
    let i = text.find('%')?;
    let digits: String = text[..i].chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    digits.chars().rev().collect::<String>().parse().ok()
}

/// `pactl list short sinks`: index, name, driver, format, state.
pub fn parse_sinks(text: &str) -> Vec<String> {
    text.lines().filter_map(|l| l.split('\t').nth(1).map(str::to_string)).collect()
}

/// Friendly names from `pactl list sinks` ("Name:" / "Description:" pairs).
pub fn parse_sink_descriptions(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut name = None;
    for l in text.lines().map(str::trim) {
        if let Some(n) = l.strip_prefix("Name: ") {
            name = Some(n.to_string());
        } else if let Some(d) = l.strip_prefix("Description: ") {
            if let Some(n) = name.take() {
                out.push((n, d.to_string()));
            }
        }
    }
    out
}

pub fn audio() -> Option<Audio> {
    if !cmd::exists("pactl") {
        return None;
    }
    let vol = cmd::run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"]).ok()?;
    let mute = cmd::run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"]).unwrap_or_default();
    let mut sinks = parse_sink_descriptions(&cmd::run("pactl", &["list", "sinks"]).unwrap_or_default());
    if sinks.is_empty() {
        sinks = parse_sinks(&cmd::run("pactl", &["list", "short", "sinks"]).unwrap_or_default()).into_iter().map(|n| (n.clone(), n)).collect();
    }
    Some(Audio {
        volume: parse_volume(&vol).unwrap_or(0),
        muted: mute.contains("yes"),
        sinks,
        default_sink: cmd::run("pactl", &["get-default-sink"]).unwrap_or_default().trim().to_string(),
    })
}

pub fn set_volume(percent: u32) -> Result<(), String> {
    cmd::run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", percent.min(150))]).map(|_| ())
}

pub fn set_mute(muted: bool) -> Result<(), String> {
    cmd::run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", if muted { "1" } else { "0" }]).map(|_| ())
}

pub fn set_default_sink(name: &str) -> Result<(), String> {
    cmd::run("pactl", &["set-default-sink", name]).map(|_| ())
}

/// `brightnessctl -m`: "intel_backlight,backlight,400,42%,960".
pub fn parse_brightness(text: &str) -> Option<u32> {
    let pct = text.lines().next()?.split(',').nth(3)?;
    pct.trim_end_matches('%').parse().ok()
}

pub fn brightness() -> Option<u32> {
    if !cmd::exists("brightnessctl") {
        return None;
    }
    parse_brightness(&cmd::run("brightnessctl", &["-m", "-c", "backlight"]).ok()?)
}

pub fn set_brightness(percent: u32) -> Result<(), String> {
    cmd::run("brightnessctl", &["-q", "-c", "backlight", "set", &format!("{}%", percent.clamp(1, 100))]).map(|_| ())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Media {
    pub player: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

/// `playerctl metadata --format '{{playerName}}\t{{status}}\t{{title}}\t{{artist}}'`
pub fn parse_media(text: &str) -> Option<Media> {
    let mut p = text.trim().split('\t');
    let player = p.next()?.to_string();
    let status = p.next().unwrap_or("");
    Some(Media {
        player,
        playing: status == "Playing",
        title: p.next().unwrap_or("").to_string(),
        artist: p.next().unwrap_or("").to_string(),
    })
    .filter(|m| !m.player.is_empty())
}

pub fn media() -> Option<Media> {
    if !cmd::exists("playerctl") {
        return None;
    }
    parse_media(&cmd::run("playerctl", &["metadata", "--format", "{{playerName}}\t{{status}}\t{{title}}\t{{artist}}"]).ok()?)
}

pub fn media_command(action: &str) -> Result<(), String> {
    cmd::run("playerctl", &[action]).map(|_| ())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Battery {
    pub name: String,
    /// Percent, when the device reports one.
    pub percent: Option<u32>,
    /// "Full", "High", "Normal", "Low", "Critical" when it only reports a level.
    pub level: String,
    pub charging: bool,
}

/// Peripheral batteries (controllers, headsets): power_supply entries with
/// `scope` = Device.
pub fn device_batteries(sys: &Path) -> Vec<Battery> {
    let Ok(rd) = std::fs::read_dir(sys) else {
        return Vec::new();
    };
    let read = |p: &Path, f: &str| std::fs::read_to_string(p.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut out: Vec<Battery> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| read(p, "scope") == "Device")
        .map(|p| {
            let model = read(&p, "model_name");
            Battery {
                name: if model.is_empty() { p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { model },
                percent: read(&p, "capacity").parse().ok(),
                level: read(&p, "capacity_level"),
                charging: read(&p, "status") == "Charging",
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn power(action: &str) -> Result<(), String> {
    match action {
        "suspend" | "poweroff" | "reboot" => cmd::run("systemctl", &[action]).map(|_| ()),
        _ => Err(format!("unknown power action {action}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers() {
        assert_eq!(parse_volume("Volume: front-left: 32768 /  50% / -18.06 dB,   front-right: 32768 /  50% / -18.06 dB"), Some(50));
        assert_eq!(parse_volume("nothing"), None);
        assert_eq!(parse_sinks("47\talsa_output.pci.analog-stereo\tPipeWire\ts32le 2ch 48000Hz\tRUNNING\n"), ["alsa_output.pci.analog-stereo"]);
        let d = parse_sink_descriptions("Sink #47\n\tState: RUNNING\n\tName: alsa_output.x\n\tDescription: Built-in Audio\nSink #48\n\tName: bluez.y\n\tDescription: Headphones\n");
        assert_eq!(d, [("alsa_output.x".to_string(), "Built-in Audio".to_string()), ("bluez.y".to_string(), "Headphones".to_string())]);
        assert_eq!(parse_brightness("intel_backlight,backlight,400,42%,960\n"), Some(42));
        let m = parse_media("m3music\tPlaying\tNorthern Lights\tAurora Vale\n").unwrap();
        assert!(m.playing && m.title == "Northern Lights" && m.player == "m3music");
        assert_eq!(parse_media(""), None);
    }

    #[test]
    fn controller_batteries() {
        let sys = tempfile::tempdir().unwrap();
        let pad = sys.path().join("ps-controller-battery-aa:bb");
        std::fs::create_dir_all(&pad).unwrap();
        std::fs::write(pad.join("scope"), "Device\n").unwrap();
        std::fs::write(pad.join("capacity"), "70\n").unwrap();
        std::fs::write(pad.join("status"), "Discharging\n").unwrap();
        std::fs::write(pad.join("model_name"), "Wireless Controller\n").unwrap();
        let bat0 = sys.path().join("BAT0");
        std::fs::create_dir_all(&bat0).unwrap();
        std::fs::write(bat0.join("capacity"), "90\n").unwrap();
        let b = device_batteries(sys.path());
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].name, "Wireless Controller");
        assert_eq!(b[0].percent, Some(70));
        assert!(!b[0].charging);
    }
}
