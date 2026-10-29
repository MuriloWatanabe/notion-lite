<div align="center">

<img src="assets/notion-lite.svg" width="96" alt="">

# Notion Lite

**A lightweight, unofficial Notion desktop client for Linux, written in Rust.**

Every Notion feature, no Electron: a sub-1 MB binary on top of your system's WebKitGTK.

</div>

## Why

The Notion clients currently available for Linux bundle a whole Chromium through Electron. Notion Lite uses the web engine your system already ships (WebKitGTK, the same one GNOME Web uses):

| | Notion Lite | Chromium/Electron |
|---|---|---|
| Executable size | **0.8 MB** | ~150–250 MB per app |
| RAM on the login screen, no GPU | **~500 MB** | ~700 MB (plain Chromium, without Electron's Node) |
| Time to login screen | ~3 s | — |

Measured on x86-64 Linux without a GPU (Xvfb), summing the PSS of all processes. Numbers change with a GPU, but the ratio holds. Most of the RAM goes to Notion's own JavaScript, which no client can avoid.

## Features

Everything the Notion web app does works: pages, databases, AI, comments, Calendar, Mail, sharing, real-time collaboration, import/export. On top of that, Notion Lite adds:

- **Persistent session.** Log in once and stay logged in.
- **Google, Apple, Microsoft, passkey and SSO login.** It opens in a popup inside the app and returns you logged in.
- **External links** open in your default browser. `mailto:`, `zoommtg:`, `slack:` and similar links go to the right app.
- **`notion://` links** and Notion links opened by the system land in the window that's already open (single instance).
- **Downloads** go to `~/Downloads`, with a notification when they finish.
- **Notion notifications** (reminders, mentions) show up as system notifications.
- **Spell checking** in your system language and English.
- **Window size, position and zoom** are remembered between runs.
- **Automatic fallback.** With no GPU (VM or a problematic driver) or a renderer crash, it switches to software rendering by itself.

### Shortcuts

| | |
|---|---|
| `Ctrl` `+` / `-` / `0` | Zoom |
| `Ctrl` `R` or `F5` | Reload |
| `F11` | Fullscreen |
| `Alt` `←` / `→` | Back / forward |
| `Ctrl` `Q` | Quit |

Notion's own shortcuts (`Ctrl+K`, `Ctrl+P`, `/`, etc.) keep working as usual.

## Installation

### Arch Linux and derivatives

Not on the AUR yet. AUR registration is temporarily closed. Until then, build the package with the PKGBUILD from this repository:

```bash
sudo pacman -S --needed base-devel git
git clone https://github.com/MuriloWatanabe/notion-lite.git
cd notion-lite/aur/notion-lite-git
makepkg -si
```

The app then shows up as **Notion Lite** in your app menu, or you can run `notion-lite`.

To update, run the same commands again in a fresh clone, or `git pull` and then `makepkg -si` inside `aur/notion-lite-git`.

### Building from source (any distro)

Requirements: Rust 1.85+, WebKitGTK 4.1 and GTK 3.

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

## Usage

```
notion-lite [options] [URL]

  URL              an https Notion link or notion://...
  --safe-mode      render without the GPU and remember it (blank window, EGL errors)
  --gpu            go back to GPU rendering after safe mode
  --new-instance   open a separate, independent window
  --devtools       enable the web inspector
  --reset          delete session, cache and preferences
```

## Troubleshooting

- **Blank or flickering window, or `Could not create default EGL display: EGL_BAD_PARAMETER`** (NVIDIA, remote sessions, VMs, some Intel GPUs): the app detects when the renderer crashes and reopens itself in safe mode. Safe mode renders in software and is remembered for next time. To force it, run `notion-lite --safe-mode`. To switch back to the GPU later, run `notion-lite --gpu`.
- **Still blank on NVIDIA + Wayland:** try `__NV_DISABLE_EXPLICIT_SYNC=1 notion-lite` or `GDK_BACKEND=x11 notion-lite --safe-mode`.
- **Hybrid graphics (Intel + NVIDIA on `nouveau`):** try `DRI_PRIME=0 notion-lite` to force the integrated GPU, or install the proprietary NVIDIA driver.
- **`libenchant ... Error loading plugin` warnings:** these are harmless. They only mean an optional spell-checking backend isn't installed.
- **No spell checking in your language:** install the matching hunspell dictionary (for example `hunspell-de`, `hunspell-es_es`, `hunspell-pt-br`).
- **Log out and wipe everything:** `notion-lite --reset`.

## Privacy

Notion Lite collects nothing. It only talks to Notion's servers and to whichever login provider you choose. Session and cache live in `~/.local/share/notion-lite`, preferences in `~/.config/notion-lite`.

## How it works

```
src/
├── main.rs      # window (tao), WebView (wry/WebKitGTK), shortcuts, permissions, crash recovery
├── popup.rs     # popups tied to the main window (OAuth with window.opener)
├── instance.rs  # single instance through a Unix socket in $XDG_RUNTIME_DIR
├── state.rs     # window size, position, zoom and safe mode
└── urls.rs      # what stays in the app vs. what goes to the browser (with tests)
aur/             # PKGBUILDs (notion-lite and notion-lite-git)
assets/          # icon and .desktop file
```

## Disclaimer

Independent project, not affiliated with Notion Labs, Inc. "Notion" is a trademark of Notion Labs, Inc. This app only displays the official notion.so service, and your use of it remains subject to Notion's terms.

## License

[MIT](LICENSE) © Murilo Watanabe