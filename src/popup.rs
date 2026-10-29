//! Janelas filhas (popups) ligadas à janela principal: login com Google/Apple/Microsoft/SSO
//! e "abrir em nova janela" do Notion. Compartilham sessão e `window.opener` com a principal,
//! então o fluxo de OAuth volta para o Notion normalmente.

use gtk::prelude::*;
use webkit2gtk::{WebView, WebViewExt, WindowPropertiesExt};

pub fn open(opener: &WebView, url: &str) -> WebView {
    let webview = WebView::builder().related_view(opener).build();

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

    webview
}
