//! Tamanho/posição da janela e zoom, lembrados entre execuções.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
    pub zoom: f64,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1200.0,
            height: 800.0,
            x: None,
            y: None,
            maximized: false,
            zoom: 1.0,
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("notion-lite")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("notion-lite")
}

fn state_file() -> PathBuf {
    config_dir().join("window.json")
}

impl WindowState {
    pub fn load() -> Self {
        let mut s: Self = std::fs::read_to_string(state_file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.width = s.width.clamp(400.0, 10000.0);
        s.height = s.height.clamp(300.0, 10000.0);
        s.zoom = s.zoom.clamp(0.5, 3.0);
        s
    }

    pub fn save(&self) {
        let dir = config_dir();
        if std::fs::create_dir_all(&dir).is_ok() {
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(state_file(), json);
            }
        }
    }
}

/// Ainda não existe estado salvo: o app nunca foi fechado normalmente nesta conta.
pub fn is_first_run() -> bool {
    !state_file().exists()
}
