//! Page scaffolding from material-3-ui, with `require` wired to this app's
//! install action.

pub use material_3_ui::common::*;

use super::{Action, Actions};
use crate::sys::{arch, cmd};
use egui::Ui;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Check that a command-line tool is available. When it isn't, show a
/// banner offering to install its package (through [`Action::Install`])
/// and return false.
pub fn require(ui: &mut Ui, actions: &mut Actions, program: &str, package: &str, purpose: &str) -> bool {
    if cmd::exists(program) {
        return true;
    }
    match material_3_ui::common::require(ui, &installed_packages(), program, package, purpose) {
        Requirement::Present => true,
        Requirement::Missing => false,
        Requirement::InstallRequested => {
            actions.push(Action::Install { packages: vec![package.to_string()] });
            false
        }
    }
}

/// The installed pacman packages, re-read at most every few seconds (only
/// while a tool is missing).
fn installed_packages() -> Arc<HashSet<String>> {
    static CACHE: Mutex<Option<(Instant, Arc<HashSet<String>>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    match &*cache {
        Some((at, set)) if at.elapsed() < Duration::from_secs(5) => set.clone(),
        _ => {
            let set = Arc::new(arch::installed_packages());
            *cache = Some((Instant::now(), set.clone()));
            set
        }
    }
}
