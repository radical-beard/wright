//! Editor state that survives restarts (recent project, export target).
//! Lives at `~/.local/share/wright/state.toml` per the workspace XDG
//! convention — hard-coded, never configured via environment variables.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Default, Serialize, Deserialize)]
pub struct AppState {
    pub last_project: Option<PathBuf>,
    pub last_export_dir: Option<PathBuf>,
    /// The game's `assets/dungeons` directory dungeons export into.
    #[serde(default)]
    pub last_dungeon_dir: Option<PathBuf>,
}

fn state_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".local/share/wright/state.toml"))
}

impl AppState {
    pub fn load() -> Self {
        state_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = state_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = toml::to_string_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }
}
