# Material 3 Games

A game manager for Arch Linux and Hyprland, built with [egui](https://github.com/emilk/egui) and themed with Material You to match the [Material 3 File Manager](https://github.com/bhimio1/material-3-file-manager-rust), [Settings](https://github.com/bhimio1/material-3-settings) and [Music](https://github.com/bhimio1/material-3-music) apps. On an end4-pC desktop it takes its colors from your wallpaper, like the shell does.

It has two jobs:
- **Desktop mode:** launch games so they open fullscreen and fast on Hyprland.
- **Game Mode:** leave Hyprland entirely and run games under gamescope, like SteamOS.

![Library](docs/screenshot-library.png)
![Game](docs/screenshot-detail.png)
![Game Mode](docs/screenshot-gamemode.png)
![Big picture](docs/screenshot-big.png)

## Features

**One library**
- **Steam:** every library folder, cover and hero art from Steam's own cache, and last-played and play time from `localconfig.vdf`. Proton and runtimes are skipped.
- **Lutris:** reads `pga.db`, covers and banners, native or Flatpak. It's the "Lutris-based" part: install and configure games in Lutris, and play them from here.
- **Heroic:** installed Epic, GOG and Amazon games.
- **Apps:** native games from your application menu (`Categories=Game`).
- **Added by hand:** native binaries, or Windows `.exe` files run with Proton through umu-launcher (or Wine). Each game gets its own prefix in `~/Games/m3-games/prefixes`.
- **Cover art:** missing covers come from Heroic's store art and, with a free API key, SteamGridDB.
- **Library view:** search, sort by recent, name or play time, filters per source, and live updates when you install or remove games.

**Desktop mode on Hyprland**
- **Fullscreen fix:** when a game launches, a small helper watches Hyprland's event socket for its window, focuses it and makes it fullscreen. It recognises Steam games by their `steam_app_<id>` class and others by their process, and ignores launcher and splash windows.
- **Performance mode:** while the game runs, animations, blur and shadows are turned off (`hyprctl eval` on Lua configs, `hyprctl keyword` on hyprland.conf). A reload when it closes turns them back on.
- **Game window rules** (Settings → Hyprland): one click adds a marked block to your config. On end-4 it goes in `custom/rules.lua`, which survives dots updates. The block contains:
  - `allow_tearing`;
  - for Steam games, gamescope, `.exe` windows and games launched here before: `immediate` (tearing for lowest latency), `idle_inhibit = "fullscreen"`, and no animations, blur or shadows.
- **Per-game options:** GameMode, MangoHud, running inside gamescope on the desktop, force fullscreen, and environment variables.
- **Play time:** tracked for games launched here.

**Game Mode**
- **Full Game Mode:** pick the **Hyprland (with Game Mode)** session at login once. It's Hyprland as usual. When you enter Game Mode, it closes your apps (after listing them), exits Hyprland, and runs **gamescope directly on the GPU** with the big-picture UI, so no desktop compositor sits between your game and the screen. **Exit to desktop** brings Hyprland back.
- **Nested Game Mode:** outside that session, gamescope runs fullscreen inside Hyprland, with performance mode on.
- **Big picture:**
  - "Continue playing" hero cards and a cover grid;
  - controller navigation (D-pad or stick to move, A play, Y options, B back, Start menu), plus keyboard and mouse;
  - clock, battery and network in the top bar;
  - a system menu with volume, Steam's own Big Picture (`steam -gamepadui`), exit to desktop, sleep and power off.

**end4-pC integration.** The shell can drive everything through the CLI:

```
m3-games                  # library window
m3-games --big-picture    # big picture
m3-games list --json      # [{"id","name","source","cover","hero","last_played","playtime_minutes","installed"}]
m3-games launch <id>      # e.g. steam:570, lutris:12, heroic:gog:1207658924, manual:my-game
m3-games game-mode        # enter Game Mode
m3-games exit-game-mode
m3-games status --json    # {"game_mode", "session_ready", "running_game": {"id","name","pid"}|null}
```

The library is also kept at `~/.cache/m3-games/library.json` (same format as `list --json`), so the shell can read it without starting anything.

## Install

From the AUR, prebuilt or built from source:

```sh
paru -S material-3-games-bin   # or material-3-games-git
```

Prebuilt x86_64 binaries are attached to [releases](https://github.com/bhimio1/material-3-games/releases). The end4-pC fork installs the latest one automatically.

With the included PKGBUILD:

```sh
cd packaging
makepkg -si
```

Or build directly (needs `libudev`, part of systemd):

```sh
cargo build --release
./target/release/m3-games
```

The packages install the `Hyprland (with Game Mode)` session entry and the `m3-session` script. With a `~/.local` install, use Game Mode → **Install** (one password prompt) instead.

Optional tools, each offered for install where it's needed:
- `gamescope`: Game Mode.
- `umu-launcher`: Windows games with Proton.
- `gamemode`, `mangohud`: per-game tuning and overlay.
- `steam`, `lutris`: their own game libraries.

## Notes

- **Hyprland config syntax:** Hyprland 0.55+ uses Lua configs, where `hyprctl keyword` doesn't apply; the app uses `hyprctl eval` there. For `hyprland.conf`, rules are written in the 0.53+ `windowrule = match:class …, immediate on, …` form; older versions need `windowrulev2` syntax instead.
- **Store-launched games:** Steam, Lutris and Heroic start their own games, so set GameMode, MangoHud and gamescope in those launchers (for Steam: `gamemoderun mangohud %command%` in launch options). Fullscreen handling and performance mode still apply.
- **Steam window classes:** games running under Steam use the `steam_app_<id>` class, which is how their windows are matched.

## Configuration

- **Settings:** `~/.config/m3-games/config.toml`.
- **Games added by hand:** `~/.config/m3-games/games.toml`.
- **Play statistics:** `~/.local/share/m3-games/stats.json`.

## Project layout

```
src/
  app.rs          state, theme, scanning, launching, actions (desktop + big picture)
  library/        Steam (vdf.rs, steam.rs), Lutris, Heroic, desktop entries, manual games
  launch.rs       commands per source with gamemode/MangoHud/gamescope wrappers
  watch.rs        background helper: catch the game window, fullscreen it, track play time
  hypr.rs         Hyprland: rules block (Lua/hyprlang), event socket, clients, perf mode
  session.rs      Game Mode: m3-session handshake, closing apps, nested gamescope
  gamepad.rs      controller input (gilrs)
  covers.rs       cover cache, SteamGridDB
  ui/             sidebar, pages, big picture, M3 widgets (shared with the other apps)
packaging/        PKGBUILD, AUR -bin template, desktop files, m3-session
```

Run the tests with `cargo test`.

## License

MIT, see [LICENSE](LICENSE).

Fonts: Inter (SIL OFL 1.1, see `assets/fonts/LICENSE-Inter.txt`). Icons: Material Symbols, via `egui_material_icons`.
