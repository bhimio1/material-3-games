//! Talking to the system. Commands, background tasks, paths, terminals and
//! pacman helpers come from material-3-ui; the rest is Game Mode's own.

pub use material_3_ui::sys::{arch, cmd, paths, task, term};

pub mod quick;
pub mod scx;

/// CachyOS's gaming meta packages, offered in Settings when missing.
pub const CACHYOS_GAMING: [&str; 2] = ["cachyos-gaming-meta", "cachyos-gaming-applications"];
