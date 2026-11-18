//! Janelas filhas (popups) ligadas à janela principal: login com Google/Apple/Microsoft/SSO
//! e "abrir em nova janela" do Notion. Compartilham sessão e `window.opener` com a principal,
//! então o fluxo de OAuth volta para o Notion normalmente.
//!
//! O Google recusa login em WebViews embutidos ("Este navegador ou app pode não ser
//! seguro") olhando o user agent do WebKitGTK. Os popups se apresentam como o Firefox
//! para Linux, que o Google aceita — mesma solução de Ferdium/Rambox.

use std::time::{SystemTime, UNIX_EPOCH};

use gtk::prelude::*;
use webkit2gtk::{SettingsExt, URIRequestExt, WebView, WebViewExt, WindowPropertiesExt};

/// Firefox 140 saiu em 24/06/2025; desde então sai uma versão a cada 4 semanas.
const FIREFOX_ANCHOR: (u64, u64) = (140, 1_750_723_200);
const FIREFOX_CYCLE_SECS: u64 = 28 * 24 * 60 * 60;

/// Versão estável do Firefox na data dada (segundos Unix). Calculada em vez de fixa
/// para o user agent não envelhecer: o Google também barra navegadores muito antigos.
fn firefox_version(now_secs: u64) -> u64 {
    let (version, released) = FIREFOX_ANCHOR;
    version + now_secs.saturating_sub(released) / FIREFOX_CYCLE_SECS
}

/// User agent do Firefox para Linux, usado nos popups de login.
pub fn firefox_user_agent() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let v = firefox_version(now);
    format!("Mozilla/5.0 (X11; Linux x86_64; rv:{v}.0) Gecko/20100101 Firefox/{v}.0")
}

pub fn open(opener: &WebView, url: &str) -> WebView {
    // Configurações próprias: mexer no user agent aqui não afeta as abas do Notion.
    let settings = webkit2gtk::Settings::new();
    settings.set_user_agent(Some(&firefox_user_agent()));
    settings.set_javascript_can_open_windows_automatically(true);
    settings.set_enable_developer_extras(
        WebViewExt::settings(opener).is_some_and(|s| s.enables_developer_extras()),
    );
    let webview = WebView::builder()
        .related_view(opener)
        .settings(&settings)
        .build();

    let (w, h) = if crate::urls::is_notion(url) {
        (1100, 760)
    } else {
        (520, 700)
    };
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(w, h);
    window.set_icon_name(Some(crate::ICON_NAME));
    window.set_title("Notion");
    if let Some(parent) = opener
        .toplevel()
        .and_then(|t| t.downcast::<gtk::Window>().ok())
    {
        window.set_transient_for(Some(&parent));
        window.set_position(gtk::WindowPosition::CenterOnParent);
    }
    window.add(&webview);

    // Respeita o tamanho pedido em window.open(..., "width=...,height=...").
    let win = window.clone();
    webview.connect_ready_to_show(move |wv| {
        if let Some(props) = wv.window_properties() {
            let g = props.geometry();
            if g.width() > 100 && g.height() > 100 {
                win.resize(g.width(), g.height());
            }
        }
        win.show_all();
    });

    let win = window.clone();
    webview.connect_title_notify(move |wv| {
        if let Some(t) = wv.title().filter(|t| !t.is_empty()) {
            win.set_title(&t);
        }
    });

    // window.close() na página (fim do login) fecha a janela.
    let win = window.clone();
    webview.connect_close(move |_| unsafe { win.destroy() });

    // O próprio popup pode abrir outro (seletor de conta, 2FA, consentimento).
    webview.connect_create(|wv, action| {
        let url = action
            .request()
            .and_then(|r| r.uri())
            .map(|u| u.to_string())
            .unwrap_or_default();
        if url.is_empty()
            || url == "about:blank"
            || crate::urls::is_auth(&url)
            || crate::urls::is_notion(&url)
        {
            Some(open(wv, &url).upcast())
        } else {
            crate::open_external(&url);
            None
        }
    });

    webview
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versao_do_firefox() {
        assert_eq!(firefox_version(0), 140);
        assert_eq!(firefox_version(FIREFOX_ANCHOR.1), 140);
        assert_eq!(firefox_version(FIREFOX_ANCHOR.1 + FIREFOX_CYCLE_SECS), 141);
        // 17/11/2026
        assert_eq!(firefox_version(1_795_392_000), 158);
    }

    #[test]
    fn user_agent_parece_firefox() {
        let ua = firefox_user_agent();
        assert!(ua.starts_with("Mozilla/5.0 (X11; Linux x86_64; rv:"));
        assert!(ua.contains("Gecko/20100101 Firefox/"));
        assert!(!ua.contains("AppleWebKit"));
    }
}
