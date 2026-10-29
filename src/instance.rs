//! Instância única via socket Unix: abrir o app de novo (ou um link notion://)
//! reaproveita a janela existente em vez de subir outro WebKit.

use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    thread,
    time::Duration,
};

pub fn socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    // Sem XDG_RUNTIME_DIR, o nome inclui o usuário para não colidir em /tmp.
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    base.join(format!("notion-lite-{user}.sock"))
}

/// Tenta entregar a mensagem a uma instância já aberta. `true` = entregue (esta instância deve sair).
pub fn forward(message: &str) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket_path()) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    stream.write_all(message.as_bytes()).is_ok()
}

/// Passa a escutar pedidos de outras instâncias. Cada mensagem é repassada a `on_message`.
pub fn listen(on_message: impl Fn(String) + Send + 'static) {
    let path = socket_path();
    let _ = std::fs::remove_file(&path); // socket órfão de uma execução anterior
    let Ok(listener) = UnixListener::bind(&path) else {
        eprintln!("notion-lite: não foi possível criar {}", path.display());
        return;
    };
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = String::new();
            if stream.take(8192).read_to_string(&mut buf).is_ok() {
                on_message(buf);
            }
        }
    });
}

pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}
