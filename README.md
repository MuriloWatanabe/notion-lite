<div align="center">

<img src="assets/notion-lite.svg" width="96" alt="">

# Notion Lite

**Cliente leve e não oficial do Notion para Linux, feito em Rust.**

Todas as funções do Notion, sem Electron: um binário de menos de 1 MB que usa o WebKitGTK do sistema.

`yay -S notion-lite`

</div>

> *English: Lightweight, unofficial Notion desktop client for Linux. A <1 MB Rust binary on top of the system WebKitGTK instead of a bundled Chromium. Every Notion feature works, because it runs the real Notion web app, plus native integration (persistent session, external links, OAuth popups, downloads, `notion://` links, single instance).*

## Por que

Os clientes Linux do Notion disponíveis hoje empacotam um Chromium inteiro com Electron. O Notion Lite usa o motor web que já vem no sistema (WebKitGTK, o mesmo do GNOME Web), e o resultado é:

| | Notion Lite | Chromium/Electron |
|---|---|---|
| Tamanho do executável | **0,8 MB** | ~150–250 MB por app |
| RAM na tela de login, sem GPU | **~500 MB** | ~700 MB (Chromium puro, sem o Node do Electron) |
| Tempo até a tela de login | ~3 s | — |

Medido em Linux x86-64 sem GPU (Xvfb), somando o PSS de todos os processos. Com GPU os números mudam, mas a proporção se mantém. A maior parte da RAM é o JavaScript do próprio Notion, que nenhum cliente consegue evitar.

## O que funciona

Tudo o que o Notion web faz: páginas, databases, IA, comentários, calendário, e-mail, compartilhamento, tempo real e importação/exportação. Além disso:

- **Sessão salva:** você entra uma vez e continua logado.
- **Login com Google, Apple, Microsoft, passkey e SSO:** abre em popup dentro do app e volta logado.
- **Links externos** abrem no navegador padrão. `mailto:`, `zoommtg:`, `slack:` etc. vão para o app certo.
- **Links `notion://`** e links do Notion abertos pelo sistema caem na janela que já está aberta (instância única).
- **Downloads** vão para `~/Downloads`, com notificação ao concluir.
- **Notificações** do Notion (lembretes e menções) aparecem como notificações do sistema.
- **Correção ortográfica** no idioma do sistema e em inglês.
- **Janela e zoom** são lembrados entre execuções.
- **Sem GPU** (VM ou driver problemático), o modo de renderização mais leve é ativado sozinho.

### Atalhos

| | |
|---|---|
| `Ctrl` `+` / `-` / `0` | Zoom |
| `Ctrl` `R` ou `F5` | Recarregar |
| `F11` | Tela cheia |
| `Alt` `←` / `→` | Voltar / avançar |
| `Ctrl` `Q` | Sair |

Os atalhos do Notion (`Ctrl+K`, `Ctrl+P`, `/` etc.) continuam funcionando normalmente.

## Instalação

### Arch Linux e derivados (AUR)

```bash
yay -S notion-lite        # versão estável
yay -S notion-lite-git    # direto do branch main
```

### Compilando

Dependências: Rust 1.85+, WebKitGTK 4.1 e GTK 3.

```bash
# Arch
sudo pacman -S --needed rust webkit2gtk-4.1 gtk3
# Debian/Ubuntu
sudo apt install cargo libwebkit2gtk-4.1-dev libgtk-3-dev
# Fedora
sudo dnf install cargo webkit2gtk4.1-devel gtk3-devel

git clone https://github.com/MuriloWatanabe/notion-lite.git
cd notion-lite
cargo build --release
./target/release/notion-lite
```

## Uso

```
notion-lite [opções] [URL]

  URL              link https do Notion ou notion://...
  --safe-mode      desativa a aceleração de GPU (use se a janela ficar em branco)
  --new-instance   abre outra janela independente
  --devtools       habilita o inspetor web
  --reset          apaga sessão, cache e preferências
```

## Problemas comuns

- **Janela em branco ou piscando (NVIDIA, Wayland):** rode `notion-lite --safe-mode`, ou use a ação "Modo seguro" no menu do ícone.
- **Sem correção ortográfica em português:** instale um dicionário hunspell de pt-BR (`hunspell-pt-br` ou `hunspell-pt_br`, conforme a distro).
- **Quero sair da conta e limpar tudo:** `notion-lite --reset`.

## Privacidade

O Notion Lite não coleta nada. Ele só conversa com os servidores do Notion (e com os provedores de login que você escolher). Sessão e cache ficam em `~/.local/share/notion-lite` e as preferências em `~/.config/notion-lite`.

## Como funciona

```
src/
├── main.rs      # janela (tao), WebView (wry/WebKitGTK), atalhos, permissões, estado
├── popup.rs     # popups ligados à janela principal (OAuth com window.opener)
├── instance.rs  # instância única via socket Unix em $XDG_RUNTIME_DIR
├── state.rs     # tamanho, posição e zoom da janela
└── urls.rs      # o que fica no app × o que vai para o navegador (com testes)
aur/             # PKGBUILDs (notion-lite e notion-lite-git)
assets/          # ícone e arquivo .desktop
```

## Aviso

Projeto independente, sem afiliação com a Notion Labs, Inc. "Notion" é marca registrada da Notion Labs, Inc. Este app apenas exibe o serviço oficial notion.so, e o uso continua sujeito aos termos do Notion.

## Licença

[MIT](LICENSE) © Murilo Watanabe
