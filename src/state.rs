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

fn safe_mode_file() -> PathBuf {
    config_dir().join("safe-mode")
}

/// Modo seguro lembrado (ativado manualmente ou depois de um crash do motor de renderização).
pub fn safe_mode_saved() -> bool {
    safe_mode_file().exists()
}

pub fn set_safe_mode(on: bool) {
    if on {
        let _ = std::fs::create_dir_all(config_dir());
        let _ = std::fs::write(safe_mode_file(), "1\n");
    } else {
        let _ = std::fs::remove_file(safe_mode_file());
    }
}

/// Abas abertas ao fechar o app, restauradas na próxima execução.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub urls: Vec<String>,
    pub active: usize,
}

const MAX_RESTORED_TABS: usize = 12;

fn session_file() -> PathBuf {
    config_dir().join("session.json")
}

/// Só páginas do Notion fora das telas de login/saída valem a pena restaurar.
pub fn restorable(url: &str) -> bool {
    crate::urls::is_notion(url)
        && url.starts_with("https://")
        && !["/login", "/logout", "/signup", "/onboarding"]
            .iter()
            .any(|p| url.contains(p))
}

pub fn load_session() -> Option<Session> {
    let mut s: Session = std::fs::read_to_string(session_file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())?;
    s.urls.retain(|u| restorable(u));
    s.urls.truncate(MAX_RESTORED_TABS);
    s.active = s.active.min(s.urls.len().saturating_sub(1));
    (!s.urls.is_empty()).then_some(s)
}

pub fn save_session(session: &Session) {
    if session.urls.is_empty() {
        let _ = std::fs::remove_file(session_file());
        return;
    }
    if std::fs::create_dir_all(config_dir()).is_ok() {
        if let Ok(json) = serde_json::to_string(session) {
            let _ = std::fs::write(session_file(), json);
        }
    }
}
