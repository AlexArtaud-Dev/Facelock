//! Partie Linux du service :
//! - icône de notification StatusNotifierItem (KDE, GNOME avec AppIndicator, XFCE…) ;
//! - raccourcis globaux X11 (sous Wayland : `facelock --run "<workflow>"`
//!   à associer à un raccourci du bureau) ;
//! - verrouillage suivi par événement (propriété logind `LockedHint`) ;
//! - notifications via le service de notifications freedesktop.

use super::{apply_hotkeys, on_hotkey, open_ui, sh};
use crate::state::{MonCmd, Shared};
use ksni::blocking::TrayMethods;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};
use x11rb::protocol::Event;

static TRAY: OnceLock<ksni::blocking::Handle<Tray>> = OnceLock::new();
static HOTKEYS_TX: OnceLock<Mutex<mpsc::Sender<()>>> = OnceLock::new();
static QUIT_TX: OnceLock<Mutex<mpsc::Sender<()>>> = OnceLock::new();

pub fn quit() {
    if let Some(tx) = QUIT_TX.get() {
        let _ = tx.lock().unwrap().send(());
    }
}

pub fn already_running() -> bool {
    crate::ipc::connect().is_some()
}

pub fn allow_foreground(_pid: Option<u32>) {}

fn login_session() -> Option<zbus::blocking::Proxy<'static>> {
    let conn = zbus::blocking::Connection::system().ok()?;
    zbus::blocking::Proxy::new_owned(
        conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
    )
    .ok()
}

pub fn session_locked() -> bool {
    login_session()
        .and_then(|p| p.get_property::<bool>("LockedHint").ok())
        .unwrap_or(false)
}

pub fn notify(message: &str) {
    let message = message.to_string();
    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new()
            .appname("FaceLock")
            .summary("FaceLock")
            .body(&message)
            .icon("security-high")
            .show();
    });
}

pub fn post_refresh() {
    if let Some(h) = TRAY.get() {
        let s = sh();
        let (suspended, tip) = (s.suspended.load(Ordering::SeqCst), crate::state::tray_tip(s));
        h.update(|t| {
            t.suspended = suspended;
            t.tip = tip;
        });
    }
}

pub fn post_hotkeys() {
    if let Some(tx) = HOTKEYS_TX.get() {
        let _ = tx.lock().unwrap().send(());
    }
}

pub fn run_loop(shared: &Arc<Shared>) {
    let (quit_tx, quit_rx) = mpsc::channel::<()>();
    let _ = QUIT_TX.set(Mutex::new(quit_tx));

    // Icône de notification
    let tray = Tray {
        suspended: false,
        tip: crate::state::tray_tip(shared),
    };
    match tray.spawn() {
        Ok(handle) => {
            let _ = TRAY.set(handle);
        }
        Err(e) => shared.log(format!(
            "Zone de notification indisponible ({e}) — interface : facelock --ui ; arrêt : facelock --quit"
        )),
    }

    // Verrouillage de session (événementiel)
    std::thread::Builder::new()
        .name("logind".into())
        .spawn(|| {
            let Some(proxy) = login_session() else { return };
            for change in proxy.receive_property_changed::<bool>("LockedHint") {
                if let Ok(locked) = change.get() {
                    sh().set_locked(locked);
                }
            }
        })
        .ok();

    // Raccourcis globaux X11
    let (hk_tx, hk_rx) = mpsc::channel();
    let _ = HOTKEYS_TX.set(Mutex::new(hk_tx));
    std::thread::Builder::new()
        .name("hotkeys".into())
        .spawn(move || hotkeys_thread(hk_rx))
        .ok();

    // Bloque jusqu'à « Quitter » (menu, ou `facelock --quit`).
    let _ = quit_rx.recv();
}

// ------------------------------------------------------------------- tray

pub struct Tray {
    suspended: bool,
    tip: String,
}

fn pixmaps(v: crate::icon::Variant) -> Vec<ksni::Icon> {
    [22usize, 32, 48, 64]
        .into_iter()
        .map(|s| {
            let rgba = crate::icon::rgba(s, v);
            // ARGB32, ordre réseau
            let data = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[3], p[0], p[1], p[2]])
                .collect();
            ksni::Icon {
                width: s as i32,
                height: s as i32,
                data,
            }
        })
        .collect()
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "facelock".into()
    }

    fn title(&self) -> String {
        "FaceLock".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        pixmaps(if self.suspended {
            crate::icon::Variant::Suspended
        } else {
            crate::icon::Variant::Active
        })
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.tip.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        open_ui("live");
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let item = |label: &str, f: Box<dyn Fn(&mut Self) + Send>| -> ksni::MenuItem<Self> {
            StandardItem {
                label: label.into(),
                activate: f,
                ..Default::default()
            }
            .into()
        };
        vec![
            item("Ouvrir FaceLock", Box::new(|_| open_ui("live"))),
            item("Workflows", Box::new(|_| open_ui("workflows"))),
            ksni::MenuItem::Separator,
            item(
                if self.suspended { "Reprendre" } else { "Mettre en pause" },
                Box::new(|_| {
                    let s = sh();
                    s.set_suspended(!s.suspended.load(Ordering::SeqCst));
                }),
            ),
            item("Réinitialiser la vérification", Box::new(|_| sh().send(MonCmd::Reset))),
            ksni::MenuItem::Separator,
            item("Quitter", Box::new(|_| quit())),
        ]
    }
}

// --------------------------------------------------------------- hotkeys

fn hotkeys_thread(rx: mpsc::Receiver<()>) {
    let Ok((conn, screen)) = x11rb::connect(None) else {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            sh().log("Raccourcis globaux : session Wayland sans X11 — associe plutôt un raccourci du bureau à « facelock --run <workflow> »");
        }
        // On consomme les demandes pour ne pas les accumuler.
        for _ in rx {}
        return;
    };
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        sh().log("Session Wayland : les raccourcis X11 ne fonctionnent que dans les applis XWayland ; préfère « facelock --run <workflow> » dans les raccourcis du bureau");
    }
    let conn = Arc::new(conn);
    let root = conn.setup().roots[screen].root;
    // (keycode, modificateurs) -> id
    let grabs: Arc<Mutex<HashMap<(u8, u16), i32>>> = Arc::default();

    // Lecture des événements
    {
        let (conn, grabs) = (conn.clone(), grabs.clone());
        std::thread::Builder::new()
            .name("x11-events".into())
            .spawn(move || {
                let relevant = u16::from(ModMask::CONTROL | ModMask::SHIFT | ModMask::M1 | ModMask::M4);
                let mut last: Option<(i32, Instant)> = None;
                while let Ok(ev) = conn.wait_for_event() {
                    if let Event::KeyPress(k) = ev {
                        let mods = u16::from(k.state) & relevant;
                        let id = grabs.lock().unwrap().get(&(k.detail, mods)).copied();
                        if let Some(id) = id {
                            // Répétition automatique : une seule exécution par appui.
                            if last.is_some_and(|(l, t)| l == id && t.elapsed() < Duration::from_millis(400)) {
                                last = Some((id, Instant::now()));
                                continue;
                            }
                            last = Some((id, Instant::now()));
                            on_hotkey(id);
                        }
                    }
                }
            })
            .ok();
    }

    let keycode_of = |keysym: u32| -> Option<u8> {
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
        let per = map.keysyms_per_keycode as usize;
        map.keysyms
            .chunks(per.max(1))
            .position(|syms| syms.iter().take(2).any(|&s| s == keysym))
            .map(|i| min + i as u8)
    };
    // Verr. Maj et Verr. Num ne doivent pas empêcher le raccourci.
    let extra = [
        0u16,
        u16::from(ModMask::LOCK),
        u16::from(ModMask::M2),
        u16::from(ModMask::LOCK | ModMask::M2),
    ];

    let apply = |force: bool| {
        apply_hotkeys(
            force,
            &mut |id| {
                let mut g = grabs.lock().unwrap();
                let keys: Vec<(u8, u16)> = g.iter().filter(|(_, v)| **v == id).map(|(k, _)| *k).collect();
                for (code, mods) in keys {
                    for e in extra {
                        let _ = conn.ungrab_key(code, root, ModMask::from(mods | e));
                    }
                    g.remove(&(code, mods));
                }
                let _ = conn.flush();
            },
            &mut |mods, vk, id| {
                let Some(code) = crate::hotkey::x11_keysym(vk).and_then(keycode_of) else {
                    return false;
                };
                let mut x = 0u16;
                if mods & crate::hotkey::MOD_CONTROL != 0 {
                    x |= u16::from(ModMask::CONTROL);
                }
                if mods & crate::hotkey::MOD_SHIFT != 0 {
                    x |= u16::from(ModMask::SHIFT);
                }
                if mods & crate::hotkey::MOD_ALT != 0 {
                    x |= u16::from(ModMask::M1);
                }
                if mods & crate::hotkey::MOD_WIN != 0 {
                    x |= u16::from(ModMask::M4);
                }
                let ok = extra.iter().all(|e| {
                    conn.grab_key(
                        false,
                        root,
                        ModMask::from(x | e),
                        code,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                    )
                    .map(|c| c.check().is_ok())
                    .unwrap_or(false)
                });
                if ok {
                    grabs.lock().unwrap().insert((code, x), id);
                }
                ok
            },
        );
    };
    apply(true);
    for () in rx {
        apply(false);
    }
}
