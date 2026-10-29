//! Classificação de URLs: o que fica dentro do app e o que vai para o navegador do sistema.

/// Extrai o host (sem porta, sem usuário) de uma URL http(s). Retorna `None` para outros esquemas.
pub fn host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if authority.starts_with('[') {
        authority.split(']').next()?.trim_start_matches('[')
    } else {
        authority.split(':').next()?
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn is_or_sub(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// Domínios do próprio Notion (app, sites publicados, arquivos, calendário, e-mail).
pub fn is_notion(url: &str) -> bool {
    host(url).is_some_and(|h| {
        [
            "notion.so",
            "notion.site",
            "notion.com",
            "notion.new",
            "notion-static.com",
            "notionusercontent.com",
        ]
        .iter()
        .any(|d| is_or_sub(&h, d))
    })
}

/// Provedores de login usados por "Continuar com Google/Apple/Microsoft" e SSO (SAML).
pub fn is_auth(url: &str) -> bool {
    host(url).is_some_and(|h| {
        [
            "accounts.google.com",
            "appleid.apple.com",
            "idmsa.apple.com",
            "login.microsoftonline.com",
            "login.live.com",
            "okta.com",
            "oktapreview.com",
            "onelogin.com",
            "auth0.com",
            "jumpcloud.com",
            "duosecurity.com",
        ]
        .iter()
        .any(|d| is_or_sub(&h, d))
    })
}

/// Navegações no frame principal/iframes: http(s) e esquemas internos ficam no app;
/// `mailto:`, `tel:`, `slack:`, `zoommtg:` etc. vão para o sistema.
pub fn stays_in_app(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["https:", "http:", "about:", "blob:", "data:"]
        .iter()
        .any(|s| lower.starts_with(s))
}

/// Converte um argumento da linha de comando em URL do Notion.
/// Aceita `notion://www.notion.so/...`, `notion://...` e links https do Notion.
/// Qualquer outra coisa é ignorada (o app não abre sites arbitrários).
pub fn from_arg(arg: &str) -> Option<String> {
    let arg = arg.trim();
    let url = if let Some(rest) = arg.strip_prefix("notion://") {
        let rest = rest.trim_start_matches('/');
        if rest.starts_with("www.notion.so") || rest.starts_with("notion.so") {
            format!("https://{rest}")
        } else {
            format!("https://www.notion.so/{rest}")
        }
    } else {
        arg.to_string()
    };
    (url.starts_with("https://") && is_notion(&url)).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrai_host() {
        assert_eq!(
            host("https://www.notion.so/abc?x=1").as_deref(),
            Some("www.notion.so")
        );
        assert_eq!(
            host("http://user:pw@Example.com:8080/x").as_deref(),
            Some("example.com")
        );
        assert_eq!(host("https://[::1]:3000/").as_deref(), Some("::1"));
        assert_eq!(host("mailto:a@b.c"), None);
    }

    #[test]
    fn dominios_do_notion() {
        assert!(is_notion("https://www.notion.so/workspace/Page-123"));
        assert!(is_notion("https://acme.notion.site/Doc"));
        assert!(is_notion("https://file.notion.so/f/x.pdf"));
        assert!(is_notion("https://www.notion.com/pricing"));
        assert!(!is_notion("https://notion.so.evil.com/"));
        assert!(!is_notion("https://evilnotion.so/"));
        assert!(!is_notion("https://github.com/"));
    }

    #[test]
    fn provedores_de_login() {
        assert!(is_auth("https://accounts.google.com/o/oauth2/auth?x"));
        assert!(is_auth("https://acme.okta.com/app/sso"));
        assert!(!is_auth("https://google.com/"));
    }

    #[test]
    fn esquemas() {
        assert!(stays_in_app("https://youtube.com/embed/x"));
        assert!(stays_in_app("about:blank"));
        assert!(!stays_in_app("mailto:x@y.z"));
        assert!(!stays_in_app("zoommtg://zoom.us/join"));
    }

    #[test]
    fn argumentos() {
        assert_eq!(
            from_arg("notion://www.notion.so/acme/Page-1").as_deref(),
            Some("https://www.notion.so/acme/Page-1")
        );
        assert_eq!(
            from_arg("notion://acme/Page-1").as_deref(),
            Some("https://www.notion.so/acme/Page-1")
        );
        assert_eq!(
            from_arg("https://www.notion.so/x").as_deref(),
            Some("https://www.notion.so/x")
        );
        assert_eq!(from_arg("https://evil.com/"), None);
        assert_eq!(from_arg("http://www.notion.so/"), None);
    }
}
