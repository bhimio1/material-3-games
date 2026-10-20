//! Steam integration beyond the library scan: non-Steam shortcuts for
//! Steam Input, achievements, community guides, news, per-game Proton and
//! launch options, and the Steam Web API.

pub mod account;
pub mod achievements;
pub mod appconfig;
pub mod bvdf;
pub mod guides;
pub mod news;
pub mod owned;
pub mod shortcuts;
pub mod textedit;
pub mod web;

/// steam:// actions for a Steam app.
pub fn url(action: &str, appid: &str) -> String {
    match action {
        "install" => format!("steam://install/{appid}"),
        "uninstall" => format!("steam://uninstall/{appid}"),
        "validate" => format!("steam://validate/{appid}"),
        "store" => format!("steam://store/{appid}"),
        "hub" => format!("steam://url/GameHub/{appid}"),
        "workshop" => format!("steam://url/SteamWorkshopPage/{appid}"),
        "properties" => format!("steam://gameproperties/{appid}"),
        "controller" => format!("steam://controllerconfig/{appid}"),
        _ => format!("steam://nav/games/details/{appid}"),
    }
}

/// Hand a steam:// URL to Steam.
pub fn open(url: &str) -> Result<(), String> {
    crate::sys::cmd::spawn_detached("steam", &[url])
}
