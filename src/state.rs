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
    /// Títulos das abas (mesma ordem de `urls`), mostrados antes de a aba carregar.
    pub titles: Vec<String>,
    pub active: usize,
}

/// Abas restauradas dormem até serem clicadas, então restaurar muitas não custa RAM.
const MAX_RESTORED_TABS: usize = 20;

fn session_file() -> PathBuf {
    config_dir().join("session.json")
}

/// Só páginas do Notion fora das telas de login/saída valem a pena restaurar. Anexos
/// (PDF, imagem) ficam de fora: o link é assinado e expira.
pub fn restorable(url: &str) -> bool {
    crate::urls::is_notion(url)
        && url.starts_with("https://")
        && !crate::urls::is_file(url)
        && !["/login", "/logout", "/signup", "/onboarding"]
            .iter()
            .any(|p| url.contains(p))
}

pub fn load_session() -> Option<Session> {
    let saved: Session =
        serde_json::from_str(&std::fs::read_to_string(session_file()).ok()?).ok()?;
    let session = clean_session(saved);
    (!session.urls.is_empty()).then_some(session)
}

/// Descarta abas que não valem restaurar, mantendo títulos e aba ativa alinhados.
fn clean_session(saved: Session) -> Session {
    let mut session = Session::default();
    for (i, url) in saved.urls.into_iter().enumerate() {
        if session.urls.len() == MAX_RESTORED_TABS || !restorable(&url) {
            continue;
        }
        if i == saved.active {
            session.active = session.urls.len();
        }
        session
            .titles
            .push(saved.titles.get(i).cloned().unwrap_or_default());
        session.urls.push(url);
    }
    session
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

#[cfg(test)]
mod tests {
    use super::*;

    fn session(urls: &[&str], titles: &[&str], active: usize) -> Session {
        Session {
            urls: urls.iter().map(|s| s.to_string()).collect(),
            titles: titles.iter().map(|s| s.to_string()).collect(),
            active,
        }
    }

    #[test]
    fn sessao_descarta_e_mantem_alinhado() {
        let saved = session(
            &[
                "https://app.notion.com/login",
                "https://app.notion.com/p/A",
                "https://file.notion.so/f/x.pdf",
                "https://app.notion.com/p/B",
            ],
            &["Login", "A", "x.pdf", "B"],
            3,
        );
        let s = clean_session(saved);
        assert_eq!(
            s.urls,
            ["https://app.notion.com/p/A", "https://app.notion.com/p/B"]
        );
        assert_eq!(s.titles, ["A", "B"]);
        assert_eq!(s.active, 1);
    }

    #[test]
    fn sessao_antiga_sem_titulos() {
        let saved: Session =
            serde_json::from_str(r#"{"urls":["https://app.notion.com/p/A"],"active":0}"#).unwrap();
        let s = clean_session(saved);
        assert_eq!(s.titles, [""]);
        assert_eq!(s.active, 0);
    }

    #[test]
    fn aba_ativa_descartada_cai_na_primeira() {
        let saved = session(
            &["https://app.notion.com/p/A", "https://www.notion.so/logout"],
            &[],
            1,
        );
        assert_eq!(clean_session(saved).active, 0);
    }
}
