//! Abas: um `gtk::Notebook` com um WebView do Notion por aba. Todas compartilham o mesmo
//! `WebContext` (cookies/sessão), então o login vale para todas.

use std::process::Command;

use gtk::prelude::*;
use tao::event_loop::EventLoopProxy;
use webkit2gtk::{
    CacheModel, HardwareAccelerationPolicy, NotificationPermissionRequest, PermissionRequestExt,
    SettingsExt, UserMediaPermissionRequest, UserMediaPermissionRequestExt, WebContextExt,
    WebProcessTerminationReason, WebViewExt as WebKitViewExt,
};
use wry::{NewWindowResponse, WebContext, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};

use crate::{instance, open_external, popup, state, urls, UserEvent};

const MAX_TITLE_CHARS: i32 = 22;

pub struct Options {
    pub devtools: bool,
    pub safe_mode: bool,
    /// `--new-instance`: não mexe no socket da instância única ao reiniciar.
    pub new_instance: bool,
    /// URL usada ao reabrir o app depois de uma queda do motor de renderização.
    pub restart_url: String,
}

struct Tab {
    id: u64,
    view: wry::WebView,
    label: gtk::Label,
    title: String,
}

pub struct Tabs {
    notebook: gtk::Notebook,
    tabs: Vec<Tab>,
    next_id: u64,
    context: WebContext,
    proxy: EventLoopProxy<UserEvent>,
    opts: Options,
    zoom: f64,
}

impl Tabs {
    pub fn new(
        container: &gtk::Box,
        context: WebContext,
        proxy: EventLoopProxy<UserEvent>,
        opts: Options,
        zoom: f64,
    ) -> Self {
        let notebook = gtk::Notebook::new();
        notebook.set_scrollable(true);
        notebook.set_show_border(false);

        let plus = gtk::Button::from_icon_name(Some("list-add-symbolic"), gtk::IconSize::Menu);
        plus.set_relief(gtk::ReliefStyle::None);
        plus.set_tooltip_text(Some("Nova aba (Ctrl+T)"));
        let p = proxy.clone();
        plus.connect_clicked(move |_| {
            let _ = p.send_event(UserEvent::NewTab(None));
        });
        notebook.set_action_widget(&plus, gtk::PackType::End);
        plus.show();

        let p = proxy.clone();
        notebook.connect_switch_page(move |_, _, _| {
            let _ = p.send_event(UserEvent::Switched);
        });

        container.pack_start(&notebook, true, true, 0);
        notebook.show();

        Self {
            notebook,
            tabs: Vec::new(),
            next_id: 1,
            context,
            proxy,
            opts,
            zoom,
        }
    }

    /// Abre uma aba nova no fim e passa a mostrá-la.
    pub fn open(&mut self, url: &str) -> wry::Result<()> {
        let id = self.next_id;
        self.next_id += 1;

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let (tab_label, label) = self.build_tab_label(id);
        let index = self.notebook.append_page(&page, Some(&tab_label));
        self.notebook.set_tab_reorderable(&page, true);
        page.show();

        let view = self.build_view(id, url, &page)?;
        let _ = view.zoom(self.zoom);
        self.tabs.push(Tab {
            id,
            view,
            label,
            title: String::new(),
        });
        self.notebook.set_current_page(Some(index));
        Ok(())
    }

    fn build_tab_label(&self, id: u64) -> (gtk::Box, gtk::Label) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let label = gtk::Label::new(Some("Notion"));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(MAX_TITLE_CHARS);
        label.set_width_chars(8);
        row.pack_start(&label, true, true, 0);

        let close = gtk::Button::from_icon_name(Some("window-close-symbolic"), gtk::IconSize::Menu);
        close.set_relief(gtk::ReliefStyle::None);
        close.set_focus_on_click(false);
        close.set_tooltip_text(Some("Fechar aba (Ctrl+W)"));
        let p = self.proxy.clone();
        close.connect_clicked(move |_| {
            let _ = p.send_event(UserEvent::CloseTab(id));
        });
        row.pack_start(&close, false, false, 0);
        row.show_all();
        (row, label)
    }

    fn build_view(&mut self, id: u64, url: &str, page: &gtk::Box) -> wry::Result<wry::WebView> {
        let first = self.tabs.is_empty();
        let title_proxy = self.proxy.clone();
        let ipc_proxy = self.proxy.clone();
        let window_proxy = self.proxy.clone();

        let webview = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_url(url)
            .with_clipboard(true)
            .with_devtools(self.opts.devtools)
            .with_autoplay(true)
            .with_initialization_script_for_main_only(crate::INIT_SCRIPT, true)
            .with_ipc_handler(move |req| {
                let _ = ipc_proxy.send_event(UserEvent::Ipc(req.body().clone()));
            })
            .with_document_title_changed_handler(move |title| {
                let _ = title_proxy.send_event(UserEvent::Title(id, title));
            })
            // mailto:, tel:, zoommtg:, slack: ... vão para o app do sistema.
            .with_navigation_handler(|url| {
                if urls::stays_in_app(&url) {
                    true
                } else {
                    open_external(&url);
                    false
                }
            })
            // target=_blank / window.open(): páginas do Notion viram aba; login (Google,
            // Apple, SSO) e janelas em branco abrem como popup ligado à janela de origem
            // (precisam do `window.opener`); o resto vai para o navegador padrão.
            .with_new_window_req_handler(move |url, features| {
                if urls::is_notion(&url) {
                    let _ = window_proxy.send_event(UserEvent::NewTab(Some(url)));
                    NewWindowResponse::Deny
                } else if url.is_empty() || url == "about:blank" || urls::is_auth(&url) {
                    NewWindowResponse::Create {
                        webview: popup::open(&features.opener.webview, &url),
                    }
                } else {
                    open_external(&url);
                    NewWindowResponse::Deny
                }
            })
            .with_download_completed_handler(|_url, path, ok| {
                let name = path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                crate::notify(
                    if ok { "Download concluído" } else { "Falha no download" },
                    &name,
                );
            })
            .build_gtk(page)?;

        let wk = webview.webview();
        if first {
            if let Some(ctx) = wk.context() {
                ctx.set_cache_model(CacheModel::DocumentBrowser);
                ctx.set_spell_checking_enabled(true);
                let langs = crate::spellcheck_languages();
                let refs: Vec<&str> = langs.iter().map(String::as_str).collect();
                ctx.set_spell_checking_languages(&refs);
            }
        }
        if let Some(settings) = WebKitViewExt::settings(&wk) {
            settings.set_enable_smooth_scrolling(true);
            settings.set_enable_back_forward_navigation_gestures(true);
            settings.set_javascript_can_access_clipboard(true);
            settings.set_enable_developer_extras(self.opts.devtools);
            if self.opts.safe_mode {
                settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
            }
        }
        self.watch_crashes(&wk);

        // Notificações (lembretes, menções) e microfone (Notion AI Meeting Notes).
        wk.connect_permission_request(|_, request| {
            if request.is::<NotificationPermissionRequest>() {
                request.allow();
                return true;
            }
            if let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() {
                if media.is_for_audio_device() && !media.is_for_video_device() {
                    request.allow();
                    return true;
                }
            }
            false
        });
        Ok(webview)
    }

    /// O processo de renderização caiu (ex.: EGL_BAD_PARAMETER com alguns drivers)?
    /// Sem modo seguro, reabre o app nele e lembra a escolha; com ele, só recarrega.
    fn watch_crashes(&self, wk: &webkit2gtk::WebView) {
        if self.opts.safe_mode {
            wk.connect_web_process_terminated(|wv, reason| {
                if reason == WebProcessTerminationReason::Crashed {
                    wv.reload();
                }
            });
            return;
        }
        let restart_url = self.opts.restart_url.clone();
        let keep_socket = self.opts.new_instance;
        wk.connect_web_process_terminated(move |_, reason| {
            if reason != WebProcessTerminationReason::Crashed {
                return;
            }
            eprintln!(
                "notion-lite: o motor de renderização caiu; reabrindo em modo seguro (--safe-mode)"
            );
            state::set_safe_mode(true);
            if !keep_socket {
                instance::cleanup();
            }
            if let Ok(exe) = std::env::current_exe() {
                let mut cmd = Command::new(exe);
                cmd.arg("--safe-mode");
                if keep_socket {
                    cmd.arg("--new-instance");
                }
                if urls::from_arg(&restart_url).is_some() {
                    cmd.arg(&restart_url);
                }
                let _ = cmd.spawn();
            }
            std::process::exit(0);
        });
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn current_index(&self) -> Option<usize> {
        self.notebook
            .current_page()
            .map(|p| p as usize)
            .filter(|&i| i < self.tabs.len())
    }

    fn current(&self) -> Option<&Tab> {
        self.current_index().map(|i| &self.tabs[i])
    }

    /// Fecha a aba; devolve `true` se era a última (o app deve sair).
    pub fn close(&mut self, id: u64) -> bool {
        if let Some(i) = self.index_of(id) {
            self.notebook.remove_page(Some(i as u32));
            self.tabs.remove(i);
        }
        self.tabs.is_empty()
    }

    pub fn close_current(&mut self) -> bool {
        match self.current().map(|t| t.id) {
            Some(id) => self.close(id),
            None => true,
        }
    }

    /// Vai para a aba seguinte (`1`) ou anterior (`-1`), circulando.
    pub fn cycle(&mut self, delta: i32) {
        let n = self.tabs.len() as i32;
        if let (Some(cur), true) = (self.current_index(), n > 1) {
            let next = (cur as i32 + delta).rem_euclid(n) as u32;
            self.notebook.set_current_page(Some(next));
        }
    }

    pub fn select(&self, index: usize) {
        self.notebook.set_current_page(Some(index as u32));
    }

    pub fn set_title(&mut self, id: u64, title: &str) {
        if let Some(i) = self.index_of(id) {
            let title = title.trim();
            self.tabs[i].title = title.to_string();
            self.tabs[i]
                .label
                .set_text(if title.is_empty() { "Notion" } else { title });
        }
    }

    /// Título da aba ativa, para a barra de título da janela.
    pub fn current_title(&self) -> &str {
        self.current()
            .map(|t| t.title.as_str())
            .filter(|t| !t.is_empty())
            .unwrap_or("Notion")
    }

    pub fn focus_current(&self) {
        if let Some(t) = self.current() {
            let _ = t.view.focus();
        }
    }

    pub fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom;
        for t in &self.tabs {
            let _ = t.view.zoom(zoom);
        }
    }

    /// URLs abertas e índice da aba ativa, para restaurar na próxima execução.
    pub fn session(&self) -> state::Session {
        let mut urls = Vec::new();
        let mut active = 0;
        let current = self.current_index();
        for (i, t) in self.tabs.iter().enumerate() {
            if let Ok(url) = t.view.url() {
                if state::restorable(&url) {
                    if Some(i) == current {
                        active = urls.len();
                    }
                    urls.push(url);
                }
            }
        }
        state::Session { urls, active }
    }
}
