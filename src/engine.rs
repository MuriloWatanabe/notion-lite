//! Configuração do WebKitGTK: contexto compartilhado pelas abas (sessão, cache, downloads)
//! e os ajustes que deixam o Notion gastar menos memória.
//!
//! Quase toda a RAM do app é o próprio Notion (o app web inteiro roda numa página). Os
//! padrões do WebKit são generosos: um processo só começa a liberar memória de verdade
//! perto de 1,5 GB, e o coletor de lixo do JavaScript deixa a memória dobrar antes de
//! limpar quando o computador tem muita RAM. Aqui os dois ficam mais econômicos.

use std::path::{Path, PathBuf};

use webkit2gtk::{
    CacheModel, CookieManagerExt, CookiePersistentStorage, DownloadExt, MemoryPressureSettings,
    URIRequestExt, WebContext, WebContextExt, WebsiteDataManager, WebsiteDataManagerExt,
};

/// Limite padrão (MB) de cada processo do WebKit. Com os limiares padrão do WebKit
/// (0,33 e 0,5), a limpeza leve começa em ~680 MB e a forte em ~1 GB, em vez de ~1,5 GB.
const DEFAULT_MEMORY_LIMIT_MB: u32 = 2048;

/// Ajustes do JavaScriptCore, lidos do ambiente pelo processo de cada aba. Só valem se a
/// pessoa não definiu a variável por conta própria.
const JSC_TUNING: &[(&str, &str)] = &[
    // O compilador de topo (FTL) gasta bastante memória para acelerar código que roda
    // muito; o Notion é um editor de documentos e quase não sente a diferença.
    ("JSC_useFTLJIT", "0"),
    // O GC dimensiona a heap pela RAM da máquina: com 32 GB ela pode dobrar antes de
    // cada coleta. Fingir 1 GB faz heaps grandes crescerem bem menos entre coletas.
    ("JSC_forceRAMSize", "1073741824"),
];

/// Precisa rodar no começo do `main`, antes de criar threads e processos do WebKit.
pub fn tune_javascript() {
    for (key, value) in JSC_TUNING {
        if std::env::var_os(key).is_none() {
            std::env::set_var(key, value);
        }
    }
}

/// `NOTION_LITE_MEMORY_LIMIT` em MB; `0` volta aos padrões do WebKit (~1,5 GB por processo).
fn memory_limit_mb() -> Option<u32> {
    let mb = std::env::var("NOTION_LITE_MEMORY_LIMIT")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(DEFAULT_MEMORY_LIMIT_MB);
    (mb > 0).then_some(mb)
}

/// Contexto compartilhado por todas as abas. Sessão, cookies e cache ficam em `data_dir`,
/// nos mesmos caminhos que o app usava antes, para ninguém precisar logar de novo.
pub fn context(data_dir: &Path) -> WebContext {
    let dir = data_dir.to_string_lossy().into_owned();
    let data_manager = WebsiteDataManager::builder()
        .base_cache_directory(dir.as_str())
        .base_data_directory(dir.as_str())
        .build();
    if let Some(cookies) = data_manager.cookie_manager() {
        cookies.set_persistent_storage(
            &data_dir.join("cookies").to_string_lossy(),
            CookiePersistentStorage::Text,
        );
    }

    let mut builder = WebContext::builder().website_data_manager(&data_manager);
    if let Some(limit) = memory_limit_mb() {
        let mut settings = MemoryPressureSettings::new();
        settings.set_memory_limit(limit);
        // Processos das abas.
        builder = builder.memory_pressure_settings(&settings);
        // Processo de rede.
        WebsiteDataManager::set_memory_pressure_settings(&mut settings);
    }
    let context = builder.build();

    // DocumentBrowser mantém o cache em disco: abas que acordam recarregam rápido.
    context.set_cache_model(CacheModel::DocumentBrowser);
    context.set_use_system_appearance_for_scrollbars(false);
    context.set_spell_checking_enabled(true);
    let langs = crate::spellcheck_languages();
    let refs: Vec<&str> = langs.iter().map(String::as_str).collect();
    context.set_spell_checking_languages(&refs);

    context.connect_download_started(|_, download| watch_download(download));
    context
}

/// Downloads vão para ~/Downloads sem sobrescrever nada, com notificação ao terminar.
fn watch_download(download: &webkit2gtk::Download) {
    download.connect_decide_destination(|download, suggested| {
        download.set_destination(&unique_path(&download_dir(), suggested).to_string_lossy());
        true
    });
    let failed = std::rc::Rc::new(std::cell::Cell::new(false));
    let f = failed.clone();
    download.connect_failed(move |_, _| f.set(true));
    download.connect_finished(move |download| {
        let name = download
            .destination()
            .map(|d| d.to_string())
            .or_else(|| {
                download
                    .request()
                    .and_then(|r| r.uri())
                    .map(|u| u.to_string())
            })
            .and_then(|p| {
                Path::new(p.trim_start_matches("file://"))
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        if failed.get() {
            crate::notify("Falha no download", &name);
        } else {
            crate::notify("Download concluído", &name);
        }
    });
}

fn download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
}

/// `dir/nome.ext`, ou `dir/nome (1).ext`, `dir/nome (2).ext`... se já existir.
fn unique_path(dir: &Path, suggested: &str) -> PathBuf {
    let file = Path::new(suggested)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "download".into());
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (file.clone(), String::new()),
    };
    let mut path = dir.join(&file);
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nome_de_download_sem_sobrescrever() {
        let dir = std::env::temp_dir().join(format!("nl-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "a.pdf"), dir.join("a.pdf"));
        std::fs::write(dir.join("a.pdf"), "").unwrap();
        assert_eq!(unique_path(&dir, "a.pdf"), dir.join("a (1).pdf"));
        assert_eq!(unique_path(&dir, "../../etc/passwd"), dir.join("passwd"));
        assert_eq!(unique_path(&dir, ".bashrc"), dir.join(".bashrc"));
        assert_eq!(unique_path(&dir, ""), dir.join("download"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
