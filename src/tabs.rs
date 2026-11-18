//! Abas: um `gtk::Notebook` com um WebView do Notion por aba. Todas compartilham o mesmo
//! `WebContext` (cookies/sessão), então o login vale para todas.
//!
//! Cada aba carregada roda num processo `WebKitWebProcess` próprio, com uma cópia inteira
//! do app do Notion (~1 GB). Para várias abas não somarem gigas de RAM:
//! - ao abrir o app, só a aba visível carrega; as outras restauradas esperam o clique;
//! - abas em segundo plano "dormem" depois de um tempo sem uso, ou quando passam de
//!   [`MAX_AWAKE`] carregadas: o WebView é destruído (o processo fecha) e a aba guarda só
//!   endereço e título. Ao voltar para ela, a página recarrega (o cache em disco ajuda).

use std::{
    process::Command,
    time::{Duration, Instant},
};

use gtk::prelude::*;
use tao::event_loop::EventLoopProxy;
use webkit2gtk::{
    CacheModel, HardwareAccelerationPolicy, MediaCaptureState, NotificationPermissionRequest,
    PermissionRequestExt, SettingsExt, UserMediaPermissionRequest, UserMediaPermissionRequestExt,
    WebContextExt, WebProcessTerminationReason, WebViewExt as WebKitViewExt,
};
use wry::{NewWindowResponse, WebContext, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};

use crate::{instance, open_external, popup, state, urls, UserEvent};

const MAX_TITLE_CHARS: i32 = 22;
/// Abas carregadas ao mesmo tempo: a visível e as usadas por último.
const MAX_AWAKE: usize = 3;

pub struct Options {
    pub devtools: bool,
    pub safe_mode: bool,
    /// `--new-instance`: não mexe no socket da instância única ao reiniciar.
    pub new_instance: bool,
    /// URL usada ao reabrir o app depois de uma queda do motor de renderização.
    pub restart_url: String,
    /// Tempo em segundo plano até uma aba dormir. `None`: abas nunca dormem.
    pub sleep_after: Option<Duration>,
}

struct Tab {
    id: u64,
    /// Página do notebook. O WebView entra nela quando a aba acorda e sai quando dorme.
    page: gtk::Box,
    /// `None`: aba dormindo, sem processo do WebKit.
    view: Option<wry::WebView>,
    /// Endereço para recarregar a aba quando ela acordar.
    url: String,
    label: gtk::Label,
    title: String,
    /// Desde quando a aba está em segundo plano. `None`: é a aba visível.
    hidden_since: Option<Instant>,
}

impl Tab {
    /// Endereço atual: o do WebView, se a aba está carregada.
    fn current_url(&self) -> String {
        self.view
            .as_ref()
            .and_then(|v| v.url().ok())
            .filter(|u| !u.is_empty() && u != "about:blank")
            .unwrap_or_else(|| self.url.clone())
    }

    /// Carregada, sem áudio tocando nem microfone/câmera/tela em uso (ex.: Notion AI
    /// Meeting Notes gravando) e com o app do Notion (anexos são leves e têm link que expira).
    fn can_sleep(&self) -> bool {
        let Some(view) = &self.view else {
            return false;
        };
        let wk = view.webview();
        let capturing = [
            wk.microphone_capture_state(),
            wk.camera_capture_state(),
            wk.display_capture_state(),
        ]
        .into_iter()
        .any(|s| !matches!(s, MediaCaptureState::None));
        !wk.is_playing_audio() && !capturing && !urls::is_file(&self.current_url())
    }
}

pub struct Tabs {
    notebook: gtk::Notebook,
    tabs: Vec<Tab>,
    next_id: u64,
    context: WebContext,
    proxy: EventLoopProxy<UserEvent>,
    opts: Options,
    zoom: f64,
    /// O primeiro WebView já configurou o contexto compartilhado (cache, ortografia, downloads).
    context_ready: bool,
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
            context_ready: false,
        }
    }

    /// Abre uma aba já carregada no fim e passa a mostrá-la.
    pub fn open(&mut self, url: &str) -> wry::Result<()> {
        let i = self.add(url, "");
        self.wake(i)?;
        let page = self.notebook.page_num(&self.tabs[i].page);
        self.notebook.set_current_page(page);
        Ok(())
    }

    /// Acrescenta uma aba dormindo (sessão restaurada): só carrega quando for mostrada.
    pub fn add_sleeping(&mut self, url: &str, title: &str) {
        self.add(url, title);
    }

    fn add(&mut self, url: &str, title: &str) -> usize {
        let id = self.next_id;
        self.next_id += 1;

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let (tab_label, label) = self.build_tab_label(id);
        label.set_text(display_title(title));
        self.notebook.append_page(&page, Some(&tab_label));
        self.notebook.set_tab_reorderable(&page, true);
        page.show();

        self.tabs.push(Tab {
            id,
            page,
            view: None,
            url: url.to_string(),
            label,
            title: title.trim().to_string(),
            hidden_since: Some(Instant::now()),
        });
        self.tabs.len() - 1
    }

    /// Carrega a aba, se ela estiver dormindo.
    fn wake(&mut self, i: usize) -> wry::Result<()> {
        if self.tabs[i].view.is_some() {
            return Ok(());
        }
        let (id, url, page) = {
            let t = &self.tabs[i];
            (t.id, t.url.clone(), t.page.clone())
        };
        let view = self.build_view(id, &url, &page)?;
        let _ = view.zoom(self.zoom);
        self.tabs[i].view = Some(view);
        Ok(())
    }

    /// Destrói o WebView da aba; sem outra página nele, o processo do WebKit fecha.
    fn sleep(&mut self, i: usize) {
        let t = &mut self.tabs[i];
        t.url = t.current_url();
        t.view = None;
    }

    /// A aba visível mudou: acorda a nova, marca as outras como em segundo plano e, se
    /// houver abas carregadas demais, faz dormir a que foi usada há mais tempo.
    pub fn on_switched(&mut self) -> wry::Result<()> {
        let Some(cur) = self.current_index() else {
            return Ok(());
        };
        let now = Instant::now();
        for (i, t) in self.tabs.iter_mut().enumerate() {
            if i == cur {
                t.hidden_since = None;
            } else if t.hidden_since.is_none() {
                t.hidden_since = Some(now);
            }
        }
        self.wake(cur)?;
        if self.opts.sleep_after.is_some() {
            while self.tabs.iter().filter(|t| t.view.is_some()).count() > MAX_AWAKE {
                let oldest = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(i, t)| *i != cur && t.can_sleep())
                    .min_by_key(|(_, t)| t.hidden_since)
                    .map(|(i, _)| i);
                match oldest {
                    Some(i) => self.sleep(i),
                    None => break,
                }
            }
        }
        Ok(())
    }

    /// Chamado periodicamente: faz dormir as abas em segundo plano há mais de `sleep_after`.
    pub fn sleep_idle(&mut self) {
        let Some(limit) = self.opts.sleep_after else {
            return;
        };
        let cur = self.current_index();
        for i in 0..self.tabs.len() {
            let t = &self.tabs[i];
            let idle = t.hidden_since.is_some_and(|since| since.elapsed() >= limit);
            if Some(i) != cur && idle && t.can_sleep() {
                self.sleep(i);
            }
        }
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
        let first = !self.context_ready;
        let title_proxy = self.proxy.clone();
        let ipc_proxy = self.proxy.clone();
        let window_proxy = self.proxy.clone();

        let mut builder = WebViewBuilder::new_with_web_context(&mut self.context)
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
            });
        // O wry liga os handlers de download no WebContext compartilhado, e não no WebView:
        // registrando em toda aba, cada download avisaria uma vez por aba já criada.
        if first {
            builder = builder.with_download_completed_handler(|_url, path, ok| {
                let name = path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                crate::notify(
                    if ok {
                        "Download concluído"
                    } else {
                        "Falha no download"
                    },
                    &name,
                );
            });
        }
        let webview = builder.build_gtk(page)?;

        let wk = webview.webview();
        if first {
            if let Some(ctx) = wk.context() {
                // DocumentBrowser mantém o cache em disco (abas que acordam recarregam
                // rápido); DocumentViewer o desligaria.
                ctx.set_cache_model(CacheModel::DocumentBrowser);
                ctx.set_spell_checking_enabled(true);
                let langs = crate::spellcheck_languages();
                let refs: Vec<&str> = langs.iter().map(String::as_str).collect();
                ctx.set_spell_checking_languages(&refs);
            }
            self.context_ready = true;
        }
        if let Some(settings) = WebKitViewExt::settings(&wk) {
            settings.set_enable_smooth_scrolling(true);
            settings.set_enable_back_forward_navigation_gestures(true);
            settings.set_javascript_can_access_clipboard(true);
            settings.set_enable_developer_extras(self.opts.devtools);
            // O wry liga o cache de voltar/avançar, que guarda a página anterior inteira na
            // memória (às vezes num processo à parte). O Notion é uma página só: não ajuda.
            settings.set_enable_page_cache(false);
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

    /// Índice em `tabs` da página do notebook. As abas podem ser arrastadas, então a ordem
    /// do notebook não é a do vetor.
    fn index_of_page(&self, page: &gtk::Widget) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.page.upcast_ref::<gtk::Widget>() == page)
    }

    fn current_index(&self) -> Option<usize> {
        let page = self
            .notebook
            .nth_page(Some(self.notebook.current_page()?))?;
        self.index_of_page(&page)
    }

    fn current(&self) -> Option<&Tab> {
        self.current_index().map(|i| &self.tabs[i])
    }

    /// Abas na ordem em que aparecem na barra.
    fn in_display_order(&self) -> impl Iterator<Item = &Tab> {
        (0..self.notebook.n_pages()).filter_map(move |n| {
            let page = self.notebook.nth_page(Some(n))?;
            self.index_of_page(&page).map(|i| &self.tabs[i])
        })
    }

    /// Fecha a aba; devolve `true` se era a última (o app deve sair).
    pub fn close(&mut self, id: u64) -> bool {
        if let Some(i) = self.index_of(id) {
            let tab = self.tabs.remove(i);
            if let Some(n) = self.notebook.page_num(&tab.page) {
                self.notebook.remove_page(Some(n));
            }
        }
        self.tabs.is_empty()
    }

    pub fn close_current(&mut self) -> bool {
        match self.current().map(|t| t.id) {
            Some(id) => self.close(id),
            None => true,
        }
    }

    /// Vai para a aba seguinte (`1`) ou anterior (`-1`) na barra, circulando.
    pub fn cycle(&mut self, delta: i32) {
        let n = self.notebook.n_pages() as i32;
        if let (Some(cur), true) = (self.notebook.current_page(), n > 1) {
            let next = (cur as i32 + delta).rem_euclid(n) as u32;
            self.notebook.set_current_page(Some(next));
        }
    }

    /// Mostra a aba na posição `index` da barra.
    pub fn select(&self, index: usize) {
        self.notebook.set_current_page(Some(index as u32));
    }

    pub fn set_title(&mut self, id: u64, title: &str) {
        if let Some(i) = self.index_of(id) {
            let title = title.trim();
            self.tabs[i].title = title.to_string();
            self.tabs[i].label.set_text(display_title(title));
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
        if let Some(view) = self.current().and_then(|t| t.view.as_ref()) {
            let _ = view.focus();
        }
    }

    pub fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom;
        for view in self.tabs.iter().filter_map(|t| t.view.as_ref()) {
            let _ = view.zoom(zoom);
        }
    }

    /// Abas abertas (na ordem da barra), títulos e aba ativa, para a próxima execução.
    pub fn session(&self) -> state::Session {
        let current = self.current().map(|t| t.id);
        let mut session = state::Session::default();
        for t in self.in_display_order() {
            let url = t.current_url();
            if state::restorable(&url) {
                if Some(t.id) == current {
                    session.active = session.urls.len();
                }
                session.urls.push(url);
                session.titles.push(t.title.clone());
            }
        }
        session
    }
}

fn display_title(title: &str) -> &str {
    let title = title.trim();
    if title.is_empty() {
        "Notion"
    } else {
        title
    }
}
