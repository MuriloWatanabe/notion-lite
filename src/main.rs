//! notion-lite — cliente leve e não oficial do Notion para Linux.
//!
//! Roda o app web do Notion (todas as funções) no WebKitGTK do sistema, em vez de
//! empacotar um Chromium inteiro como os clientes Electron, e adiciona a integração
//! nativa que falta ao navegador: sessão persistente, links externos no navegador
//! padrão, popups de login, downloads, `notion://`, instância única e notificações.

mod instance;
mod popup;
mod state;
mod tabs;
mod urls;

use std::{cell::RefCell, process::Command, rc::Rc};

use gtk::prelude::GtkWindowExt;

use tao::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::unix::{EventLoopBuilderExtUnix, WindowExtUnix},
    window::{Fullscreen, WindowBuilder},
};
use wry::WebContext;

use crate::{state::WindowState, tabs::Tabs};

const APP_ID: &str = "io.github.murilowatanabe.NotionLite";
pub const ICON_NAME: &str = "notion-lite";
/// O app (e o cookie de sessão) mora em app.notion.com; www.notion.so é só a página de
/// marketing e não recebe o cookie, então abrir por ele parecia "deslogado".
const HOME_URL: &str = "https://app.notion.com/";
/// Na primeira execução vai direto para o login em vez da página de marketing.
const LOGIN_URL: &str = "https://www.notion.so/login";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
enum UserEvent {
    Title(u64, String),
    NewTab(Option<String>),
    CloseTab(u64),
    /// A aba ativa mudou.
    Switched,
    /// Pedido vindo de outra instância (ou de um link notion://).
    Remote(String),
    Ipc(String),
}

/// Atalhos que um navegador teria e a página do Notion não oferece.
pub(crate) const INIT_SCRIPT: &str = r#"
(() => {
  const send = (m) => window.ipc && window.ipc.postMessage(m);
  window.addEventListener('keydown', (e) => {
    const mod = e.ctrlKey || e.metaKey;
    let handled = true;
    if (mod && !e.altKey && (e.key === '=' || e.key === '+')) send('zoom:in');
    else if (mod && !e.altKey && e.key === '-') send('zoom:out');
    else if (mod && !e.altKey && e.key === '0') send('zoom:reset');
    else if ((mod && !e.shiftKey && e.key.toLowerCase() === 'r') || e.key === 'F5') location.reload();
    else if (e.key === 'F11') send('fullscreen');
    else if (mod && !e.shiftKey && e.key.toLowerCase() === 'q') send('quit');
    else if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 't') send('tab:new');
    else if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'w') send('tab:close');
    else if (mod && !e.altKey && (e.key === 'Tab' || e.key === 'PageDown')) send(e.shiftKey && e.key === 'Tab' ? 'tab:prev' : 'tab:next');
    else if (mod && !e.altKey && e.key === 'PageUp') send('tab:prev');
    else if (e.altKey && e.key === 'ArrowLeft') history.back();
    else if (e.altKey && e.key === 'ArrowRight') history.forward();
    else handled = false;
    if (handled) { e.preventDefault(); e.stopPropagation(); }
  }, true);
})();
"#;

fn print_help() {
    println!(
        "notion-lite {VERSION} — cliente leve e não oficial do Notion\n\n\
         Uso: notion-lite [opções] [URL]\n\n\
         URL pode ser um link https do Notion ou notion://...\n\n\
         Opções:\n  \
           --safe-mode      renderiza sem GPU e lembra a escolha (tela branca, erro de EGL)\n  \
           --gpu            volta a usar a GPU depois de um --safe-mode\n  \
           --new-instance   não reaproveita a janela já aberta\n  \
           --devtools       habilita o inspetor web (clique direito → Inspecionar)\n  \
           --reset          apaga sessão, cache e preferências\n  \
           -V, --version    mostra a versão\n  \
           -h, --help       mostra esta ajuda\n\n\
         Atalhos: Ctrl +/-/0 zoom · Ctrl R recarregar · F11 tela cheia · Ctrl T/W nova/fechar aba · Ctrl Tab alternar abas · Alt ←/→ voltar/avançar · Ctrl Q sair"
    );
}

/// Abre no navegador/app padrão do sistema, sem bloquear a interface.
pub(crate) fn open_external(url: &str) {
    let opener = std::env::var("NOTION_LITE_OPENER").unwrap_or_else(|_| "xdg-open".into());
    match Command::new(&opener).arg(url).spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => eprintln!("notion-lite: não foi possível executar {opener}: {e}"),
    }
}

pub(crate) fn notify(summary: &str, body: &str) {
    if let Ok(mut c) = Command::new("notify-send")
        .args(["--app-name=Notion Lite", "--icon", ICON_NAME, summary, body])
        .spawn()
    {
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    }
}

fn has_gpu_render_node() -> bool {
    std::fs::read_dir("/dev/dri")
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("renderD"))
        })
        .unwrap_or(false)
}

pub(crate) fn spellcheck_languages() -> Vec<String> {
    let mut langs = Vec::new();
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            let code = v.split(['.', '@']).next().unwrap_or("").to_string();
            if code.len() >= 2 && code != "C" && code != "POSIX" {
                langs.push(code);
                break;
            }
        }
    }
    if !langs.iter().any(|l| l == "en_US") {
        langs.push("en_US".into());
    }
    langs
}

fn main() -> wry::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let has = |args: &mut Vec<String>, flag: &str| {
        let found = args.iter().any(|a| a == flag);
        args.retain(|a| a != flag);
        found
    };
    if has(&mut args, "-h") || has(&mut args, "--help") {
        print_help();
        return Ok(());
    }
    if has(&mut args, "-V") || has(&mut args, "--version") {
        println!("notion-lite {VERSION}");
        return Ok(());
    }
    if has(&mut args, "--gpu") {
        state::set_safe_mode(false);
    }
    let safe_mode = has(&mut args, "--safe-mode")
        || std::env::var_os("NOTION_LITE_SAFE_MODE").is_some()
        || state::safe_mode_saved();
    let new_instance = has(&mut args, "--new-instance");
    let devtools =
        has(&mut args, "--devtools") || std::env::var_os("NOTION_LITE_DEVTOOLS").is_some();
    if has(&mut args, "--reset") {
        let _ = std::fs::remove_dir_all(state::data_dir());
        let _ = std::fs::remove_dir_all(state::config_dir());
        println!("Sessão, cache e preferências apagados.");
        return Ok(());
    }

    let requested = args.iter().find_map(|a| urls::from_arg(a));
    // Sem link explícito, reabre as abas da última sessão (já logado, onde parou).
    let session = if requested.is_none() && std::env::var_os("NOTION_LITE_URL").is_none() {
        state::load_session()
    } else {
        None
    };
    // NOTION_LITE_URL: só para testes/desenvolvimento, aponta para outra URL inicial.
    let start_url = requested
        .clone()
        .or_else(|| std::env::var("NOTION_LITE_URL").ok())
        .or_else(|| session.as_ref().map(|s| s.urls[s.active].clone()))
        .unwrap_or_else(|| {
            if state::is_first_run() {
                LOGIN_URL
            } else {
                HOME_URL
            }
            .to_string()
        });

    // Já existe uma janela aberta? Entrega o link (ou só pede foco) e sai.
    if !new_instance && instance::forward(requested.as_deref().unwrap_or("focus")) {
        return Ok(());
    }

    // Sem GPU (VM, servidor, driver ausente) o caminho acelerado só usa renderização por
    // software mais cara; o modo seguro gasta ~150 MB a menos nesse caso.
    let safe_mode = safe_mode || !has_gpu_render_node();
    if safe_mode {
        eprintln!("notion-lite: modo seguro (renderização por software)");
        // Renderização 100% por software: contorna tela em branco e o crash
        // "Could not create default EGL display" (NVIDIA, acesso remoto, VMs, alguns Intel).
        for (k, v) in [
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
            ("WEBKIT_DISABLE_COMPOSITING_MODE", "1"),
            ("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1"),
            ("LIBGL_ALWAYS_SOFTWARE", "1"),
        ] {
            std::env::set_var(k, v);
        }
    }

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
        .with_app_id(APP_ID)
        .build();
    let proxy = event_loop.create_proxy();

    if !new_instance {
        let p = proxy.clone();
        instance::listen(move |msg| {
            let _ = p.send_event(UserEvent::Remote(msg));
        });
    }

    let mut win_state = WindowState::load();
    let mut builder = WindowBuilder::new()
        .with_title("Notion")
        .with_inner_size(LogicalSize::new(win_state.width, win_state.height))
        .with_min_inner_size(LogicalSize::new(400.0, 300.0))
        .with_maximized(win_state.maximized);
    if let (Some(x), Some(y)) = (win_state.x, win_state.y) {
        builder = builder.with_position(PhysicalPosition::new(x, y));
    }
    let window = builder.build(&event_loop).expect("falha ao criar a janela");
    window.gtk_window().set_icon_name(Some(ICON_NAME));

    let data_dir = state::data_dir();
    let _ = std::fs::create_dir_all(&data_dir);
    let context = WebContext::new(Some(data_dir));

    let opts = tabs::Options {
        devtools,
        safe_mode,
        new_instance,
        restart_url: start_url.clone(),
    };
    let container = window.default_vbox().expect("janela sem vbox GTK");
    let mut tabs = Tabs::new(container, context, proxy.clone(), opts, win_state.zoom);
    match &session {
        Some(sess) => {
            for url in &sess.urls {
                tabs.open(url)?;
            }
            tabs.select(sess.active);
        }
        None => tabs.open(&start_url)?,
    }
    let fullscreen = Rc::new(RefCell::new(false));

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Title(id, title)) => {
                tabs.set_title(id, &title);
                window.set_title(tabs.current_title());
            }
            Event::UserEvent(UserEvent::Switched) => {
                window.set_title(tabs.current_title());
                tabs.focus_current();
            }
            Event::UserEvent(UserEvent::NewTab(url)) => {
                let url = url.unwrap_or_else(|| HOME_URL.to_string());
                if let Err(e) = tabs.open(&url) {
                    eprintln!("notion-lite: não foi possível abrir a aba: {e}");
                }
            }
            Event::UserEvent(UserEvent::CloseTab(id)) => {
                if tabs.close(id) {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::Remote(msg)) => {
                // Link de fora (notion://, xdg-open): abre numa aba nova em vez de
                // sobrescrever o que a pessoa está fazendo.
                if let Some(url) = urls::from_arg(&msg) {
                    if let Err(e) = tabs.open(&url) {
                        eprintln!("notion-lite: não foi possível abrir a aba: {e}");
                    }
                }
                window.set_minimized(false);
                window.set_visible(true);
                window.set_focus();
            }
            Event::UserEvent(UserEvent::Ipc(msg)) => match msg.as_str() {
                "zoom:in" | "zoom:out" | "zoom:reset" => {
                    win_state.zoom = match msg.as_str() {
                        "zoom:in" => (win_state.zoom + 0.1).min(3.0),
                        "zoom:out" => (win_state.zoom - 0.1).max(0.5),
                        _ => 1.0,
                    };
                    win_state.zoom = (win_state.zoom * 10.0).round() / 10.0;
                    tabs.set_zoom(win_state.zoom);
                }
                "tab:new" => {
                    let _ = tabs.open(HOME_URL);
                }
                "tab:close" => {
                    if tabs.close_current() {
                        *control_flow = ControlFlow::Exit;
                    }
                }
                "tab:next" => tabs.cycle(1),
                "tab:prev" => tabs.cycle(-1),
                "fullscreen" => {
                    let mut fs = fullscreen.borrow_mut();
                    *fs = !*fs;
                    window.set_fullscreen(fs.then_some(Fullscreen::Borderless(None)));
                }
                "quit" => *control_flow = ControlFlow::Exit,
                _ => {}
            },
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::LoopDestroyed => {
                state::save_session(&tabs.session());
                win_state.maximized = window.is_maximized();
                if !win_state.maximized && !*fullscreen.borrow() {
                    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
                    win_state.width = size.width;
                    win_state.height = size.height;
                    if let Ok(pos) = window.outer_position() {
                        win_state.x = Some(pos.x);
                        win_state.y = Some(pos.y);
                    }
                }
                win_state.save();
                if !new_instance {
                    instance::cleanup();
                }
            }
            _ => {}
        }
    });
}
