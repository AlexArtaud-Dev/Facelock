<div align="center">

<img src="assets/logo.png" width="112" alt="FaceLock">

# FaceLock

### Your PC knows your face. And it trusts no one else.

*Face-recognition lock and privacy for Windows and Linux — 100% local, 0% CPU at idle.*

[![CI](https://github.com/AlexArtaud-Dev/Facelock/actions/workflows/ci.yml/badge.svg)](https://github.com/AlexArtaud-Dev/Facelock/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/AlexArtaud-Dev/Facelock?label=release)](https://github.com/AlexArtaud-Dev/Facelock/releases/latest)
![Windows](https://img.shields.io/badge/Windows-10%2F11-0078D6?logo=windows&logoColor=white)
![Linux](https://img.shields.io/badge/Linux-X11%20%7C%20Wayland-FCC624?logo=linux&logoColor=black)
![Rust](https://img.shields.io/badge/Rust-1.95+-B7410E?logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-yellow)

</div>

---

You get up, you forget `Win+L`, and your screen tells your life story to the whole open space. **FaceLock** quietly watches through your webcam: the moment *your* face is gone, or a stranger sits down or reads over your shoulder, it runs **your workflows**.

## ✨ Features

- 🎯 **Recognition, not just detection**: only enrolled faces count as "you".
- ⚡ **Visual workflows**: a trigger (*I step away*, *unknown face*, *I'm back*, *keyboard shortcut*) followed by a chain of actions. You build them block by block, enable each one on its own, or start from 9 ready-made templates.
- 🛡️ **Privacy screen**: an "Access denied" message or your own image, full screen, on top of everything else. It closes with a **mandatory emergency shortcut** (which still works if the screen freezes), or automatically when you come back.
- 🧰 **13 actions**: lock, privacy screen, switch virtual desktop, show desktop, minimize / close / move an app (by process and window title), play/pause, mute, turn the screen off, sleep, notification, wait.
- 🪶 **Featherweight**: a single executable, 0% CPU at idle (system events, no polling), camera released as soon as the session is locked.
- 🔒 **Privacy by design**: no photos are stored, only 128-D embeddings; nothing ever leaves your machine.

## 🚀 Installation

Grab the [latest release](https://github.com/AlexArtaud-Dev/Facelock/releases/latest).

| System | File | |
|---|---|---|
| Windows 10/11 | `FaceLock-Setup-x.y.z.exe` | per-user install, no admin rights needed |
| Windows (portable) | `FaceLock-x.y.z-portable-x64.exe` | a single file, nothing to install |
| Debian / Ubuntu | `facelock_x.y.z_amd64.deb` | `sudo apt install ./facelock_x.y.z_amd64.deb` |
| Other Linux | `facelock-x.y.z-linux-x86_64.tar.gz` | binary + `.desktop` file + icon |

Checksums are in `SHA256SUMS-*.txt`. On first launch, go to **Faces → Add a face** (12 captures, about a minute).

### Linux notes

- Tray icon: native on KDE, XFCE, Cinnamon…; GNOME needs the *AppIndicator* extension.
- System actions use the standard tools: `loginctl` / `systemctl` (systemd), `xdotool` and `wmctrl` (windows, desktops), `playerctl`, `pactl`, `xset`. The `.deb` recommends them, and a missing tool is reported in the log.
- Global shortcuts: native on X11. On **Wayland**, bind a desktop shortcut to `facelock --run "<workflow name>"`.

## ⌨️ Command line

```text
facelock                    start the service (tray icon)
facelock --ui [page]        open the interface (live, workflows, faces, settings)
facelock --toggle           pause / resume monitoring
facelock --reset            reset the check
facelock --run "<name>"     run a workflow
facelock --dismiss          close the privacy screen
facelock --quit             stop the service
```

## 🏗️ Architecture

```
facelock               resident service, no graphics library loaded
├─ system              tray, global shortcuts, lock events (Win32/WTS or SNI/X11/logind)
├─ monitor             webcam → YuNet → SFace → triggers → workflows
└─ ipc                 named pipe (Windows) / Unix socket 0600 (Linux)

facelock --ui          egui interface, started on demand, memory freed when closed
facelock --overlay     privacy screen (separate process, killed by the service)
```

The monitor sleeps between checks and wakes up instantly on any command. A face that has already been identified is only re-identified if it moves, or every 3 s, so a check costs about 10 ms. Inference runs on [tract](https://github.com/sonos/tract) (pure Rust, no DLL) and gives **the same results as OpenCV** (embedding cosine ≥ 0.999), so the recommended 0.363 threshold still applies.

| Folder | Role |
|---|---|
| `src/daemon/` | service: lifecycle, IPC, shortcuts, privacy screen (+ `windows.rs` / `linux.rs`) |
| `src/monitor.rs` | monitoring loop, identity cache, workflow engine, enrollment |
| `src/engine.rs` | YuNet + SFace (pre/post-processing identical to OpenCV) |
| `src/camera/` | Media Foundation (Windows), V4L2 (Linux) |
| `src/actions/` | action catalog + per-system implementations |
| `src/ui/` | interface: glass theme, workflow editor, privacy screen |

## 🛠️ Development

```bash
cargo run                     # service
cargo run -- --ui workflows   # interface only
cargo test                    # unit tests
cargo run --release --example selftest   # model self-test
scripts/package.sh 0.0.0-dev dist        # installers (Inno Setup / dpkg-deb)
```

Linux: `sudo apt install libclang-dev` (V4L2 headers). The compiler version is pinned by `rust-toolchain.toml`.

Commits follow [Conventional Commits](https://www.conventionalcommits.org/): see [CONTRIBUTING.md](CONTRIBUTING.md). Releases are automated. *release-please* keeps a release PR up to date (changelog + version), and merging it publishes the GitHub release with the Windows and Linux installers.

## 🔄 Coming from the Python version (1.x)

- `config.json` is migrated to workflows automatically (the old file is kept as `config.v1.json`).
- `.npz` face profiles are not imported: just enroll again (one minute).
- Shortcuts take modifiers plus **one** key (`ctrl+alt+p`).

## 🤔 FAQ

**Can it be fooled with a photo?** A good photo held steady, probably. FaceLock protects against forgotten locks and opportunistic snoopers, not against a determined spy. For hardware anti-spoofing, use Windows Hello with an IR camera.

**What if the privacy screen freezes?** The emergency shortcut is caught by the service, not by the window, so it closes the screen even if the window stops responding. As a last resort, there is `facelock --dismiss`.

## 📄 License

MIT, see [LICENSE](LICENSE). [YuNet](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet) (MIT) and [SFace](https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface) (Apache 2.0) models from opencv_zoo; [Phosphor](https://phosphoricons.com/) icons (MIT).
