//! notion-lite — cliente leve e não oficial do Notion para Linux.
//!
//! Roda o app web do Notion (todas as funções) no WebKitGTK do sistema, em vez de
//! empacotar um Chromium inteiro como os clientes Electron, e adiciona a integração
//! nativa que falta ao navegador: sessão persistente, links externos no navegador
//! padrão, popups de login, downloads, `notion://`, instância única e notificações.

mod instance;
mod popup;
mod state;
mod urls;

use std::{cell::RefCell, process::Command, rc::Rc};

use gtk::{
    glib::{Cast, ObjectExt},
    prelude::GtkWindowExt,
};

use tao::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::unix::{EventLoopBuilderExtUnix, WindowExtUnix},
    window::{Fullscreen, WindowBuilder},
};
use webkit2gtk::{
    CacheModel, HardwareAccelerationPolicy, NotificationPermissionRequest, PermissionRequestExt,
    SettingsExt, UserMediaPermissionRequest, UserMediaPermissionRequestExt, WebContextExt,
    WebProcessTerminationReason, WebViewExt as WebKitViewExt,
};
use wry::{NewWindowResponse, WebContext, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};

use crate::state::WindowState;

const APP_ID: &str = "io.github.murilowatanabe.NotionLite";
pub const ICON_NAME: &str = "notion-lite";
const HOME_URL: &str = "https://www.notion.so/";
/// Na primeira execução vai direto para o login em vez da página de marketing.
const LOGIN_URL: &str = "https://www.notion.so/login";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
enum UserEvent {
    Title(String),
    /// Pedido vindo de outra instância (ou de um link notion://).
    Remote(String),
    Ipc(String),
}

/// Atalhos que um navegador teria e a página do Notion não oferece.
const INIT_SCRIPT: &str = r#"
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
         Atalhos: Ctrl +/-/0 zoom · Ctrl R recarregar · F11 tela cheia · Alt ←/→ voltar/avançar · Ctrl Q sair"
    );
}

/// Abre no navegador/app padrão do sistema, sem bloquear a interface.
fn open_external(url: &str) {
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

fn notify(summary: &str, body: &str) {
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

fn spellcheck_languages() -> Vec<String> {
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
    // Só para testes/desenvolvimento: permite apontar para outra URL inicial.
    let start_url = requested
        .clone()
        .or_else(|| std::env::var("NOTION_LITE_URL").ok())
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
    let mut context = WebContext::new(Some(data_dir));

    let title_proxy = proxy.clone();
    let ipc_proxy = proxy.clone();
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(&start_url)
        .with_clipboard(true)
        .with_devtools(devtools)
        .with_autoplay(true)
        .with_initialization_script_for_main_only(INIT_SCRIPT, true)
        .with_ipc_handler(move |req| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(req.body().clone()));
        })
        .with_document_title_changed_handler(move |title| {
            let _ = title_proxy.send_event(UserEvent::Title(title));
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
        // Links com target=_blank e window.open(): Notion e login (Google, Apple,
        // Microsoft, SSO) abrem como popup ligado à janela principal; o resto vai
        // para o navegador padrão.
        .with_new_window_req_handler(|url, features| {
            if url.is_empty()
                || url == "about:blank"
                || urls::is_notion(&url)
                || urls::is_auth(&url)
            {
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
            if ok {
                notify("Download concluído", &name);
            } else {
                notify("Falha no download", &name);
            }
        })
        .build_gtk(window.default_vbox().expect("janela sem vbox GTK"))?;

    // Ajustes direto no WebKitGTK.
    let wk = webview.webview();
    if let Some(ctx) = wk.context() {
        ctx.set_cache_model(CacheModel::DocumentBrowser);
        ctx.set_spell_checking_enabled(true);
        let langs = spellcheck_languages();
        let refs: Vec<&str> = langs.iter().map(String::as_str).collect();
        ctx.set_spell_checking_languages(&refs);
    }
    if let Some(settings) = WebKitViewExt::settings(&wk) {
        settings.set_enable_smooth_scrolling(true);
        settings.set_enable_back_forward_navigation_gestures(true);
        settings.set_javascript_can_access_clipboard(true);
        settings.set_enable_developer_extras(devtools);
        if safe_mode {
            settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
        }
    }
    // O processo de renderização caiu (ex.: EGL_BAD_PARAMETER com alguns drivers)?
    // Sem modo seguro, reabre o app nele e lembra a escolha; com ele, só recarrega.
    if !safe_mode {
        let restart_url = start_url.clone();
        let keep_socket = new_instance;
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
    } else {
        wk.connect_web_process_terminated(|wv, reason| {
            if reason == WebProcessTerminationReason::Crashed {
                wv.reload();
            }
        });
    }

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
    let _ = webview.zoom(win_state.zoom);

    let webview = Rc::new(webview);
    let fullscreen = Rc::new(RefCell::new(false));

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Title(title)) => {
                let title = title.trim();
                window.set_title(if title.is_empty() { "Notion" } else { title });
            }
            Event::UserEvent(UserEvent::Remote(msg)) => {
                if let Some(url) = urls::from_arg(&msg) {
                    let _ = webview.load_url(&url);
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
                    let _ = webview.zoom(win_state.zoom);
                }
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
