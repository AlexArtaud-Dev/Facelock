//! Processus résident : icône de notification, raccourcis globaux
//! (RegisterHotKey, aucun hook clavier), suivi du verrouillage par
//! événement (WTS), serveur IPC. Aucune bibliothèque graphique chargée.

use crate::config::Config;
use crate::hotkey;
use crate::ipc::{self, Cmd, FrameMeta, Reply, Req, Status};
use crate::state::{MonCmd, Shared};
use std::os::windows::io::FromRawHandle;
use std::process::Child;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Pipes::*;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_TRAY: u32 = WM_APP + 1;
const WM_REFRESH: u32 = WM_APP + 2;
const WM_HOTKEYS: u32 = WM_APP + 3;

const ID_LIVE: u32 = 1;
const ID_SETTINGS: u32 = 2;
const ID_SUSPEND: u32 = 3;
const ID_RESET: u32 = 4;
const ID_QUIT: u32 = 9;

const HK_TOGGLE: i32 = 1;
const HK_RESET: i32 = 2;
const HK_WORKFLOW: i32 = 100;

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static UI_CHILD: Mutex<Option<Child>> = Mutex::new(None);
static APPLIED_HOTKEYS: Mutex<Vec<HotkeySpec>> = Mutex::new(Vec::new());
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

fn sh() -> &'static Arc<Shared> {
    SHARED.get().expect("état non initialisé")
}

fn hwnd_of(sh: &Shared) -> Option<HWND> {
    let h = sh.tray_hwnd.load(Ordering::SeqCst);
    (h != 0).then_some(HWND(h as *mut _))
}

pub fn post_refresh(sh: &Shared) {
    if let Some(h) = hwnd_of(sh) {
        unsafe {
            let _ = PostMessageW(Some(h), WM_REFRESH, WPARAM(0), LPARAM(0));
        }
    }
}

fn post_hotkeys(sh: &Shared) {
    if let Some(h) = hwnd_of(sh) {
        unsafe {
            let _ = PostMessageW(Some(h), WM_HOTKEYS, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn run() {
    unsafe {
        let _ = CreateMutexW(None, true, w!("Local\\FaceLock.Daemon"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Déjà lancé : on ouvre simplement la fenêtre de l'instance existante.
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            ipc::send_once(Cmd::OpenUi {
                page: "live".into(),
            });
            return;
        }
    }

    let (tx, rx) = mpsc::channel();
    let shared = Shared::new(Config::load(), tx);
    let _ = SHARED.set(shared.clone());
    shared.log("FaceLock démarré");
    shared.locked.store(session_locked(), Ordering::SeqCst);

    let mon_sh = shared.clone();
    let monitor = std::thread::Builder::new()
        .name("monitor".into())
        .stack_size(8 << 20)
        .spawn(move || crate::monitor::run(mon_sh, rx))
        .expect("thread moniteur");
    let ipc_sh = shared.clone();
    std::thread::Builder::new()
        .name("ipc".into())
        .spawn(move || ipc_server(ipc_sh))
        .expect("thread ipc");

    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("FaceLockTray");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let _ = TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("FaceLock"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .expect("fenêtre tray");
        shared.tray_hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
        tray_add(hwnd);
        let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
        apply_hotkeys(hwnd, true);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        tray_remove(hwnd);
    }

    shared.log("Arrêt de FaceLock");
    shared.send(MonCmd::Quit);
    let _ = monitor.join();
    if let Some(mut c) = UI_CHILD.lock().unwrap().take() {
        let _ = c.kill();
    }
}

fn session_locked() -> bool {
    unsafe {
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) {
            Ok(h) => {
                let _ = CloseDesktop(h);
                false
            }
            Err(_) => true,
        }
    }
}

// ------------------------------------------------------------------ tray

unsafe fn make_hicon(variant: crate::icon::Variant) -> HICON {
    let size = GetSystemMetrics(SM_CXSMICON).max(16) as usize;
    let rgba = crate::icon::rgba(size, variant);
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let Ok(color_bmp) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
        return HICON::default();
    };
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, size * size * 4);
    for (d, s) in dst.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
        d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
    }
    let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
    let info = ICONINFO {
        fIcon: true.into(),
        hbmMask: mask,
        hbmColor: color_bmp,
        ..Default::default()
    };
    let icon = CreateIconIndirect(&info).unwrap_or_default();
    let _ = DeleteObject(color_bmp.into());
    let _ = DeleteObject(mask.into());
    icon
}

fn copy_wstr(dst: &mut [u16], s: &str) {
    let mut n = 0;
    for (d, c) in dst.iter_mut().zip(s.encode_utf16()) {
        *d = c;
        n += 1;
    }
    if n >= dst.len() {
        n = dst.len() - 1;
    }
    dst[n] = 0;
}

fn nid(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    }
}

static CURRENT_ICON: Mutex<isize> = Mutex::new(0);

unsafe fn tray_update(hwnd: HWND, add: bool) {
    let s = sh();
    let suspended = s.suspended.load(Ordering::SeqCst);
    let icon = make_hicon(if suspended {
        crate::icon::Variant::Suspended
    } else {
        crate::icon::Variant::Active
    });
    let mut d = nid(hwnd);
    d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    d.uCallbackMessage = WM_TRAY;
    d.hIcon = icon;
    let tip = if suspended {
        "FaceLock — suspendu"
    } else if s.camera_active.load(Ordering::SeqCst) {
        "FaceLock — surveillance active"
    } else {
        "FaceLock — en veille"
    };
    copy_wstr(&mut d.szTip, tip);
    let _ = Shell_NotifyIconW(if add { NIM_ADD } else { NIM_MODIFY }, &d);
    let mut cur = CURRENT_ICON.lock().unwrap();
    if *cur != 0 {
        let _ = DestroyIcon(HICON(*cur as *mut _));
    }
    *cur = icon.0 as isize;
}

unsafe fn tray_add(hwnd: HWND) {
    tray_update(hwnd, true);
}

unsafe fn tray_remove(hwnd: HWND) {
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid(hwnd));
}

/// Notification Windows (bulle / toast) via l'icône de notification.
pub fn notify(message: &str) {
    let Some(hwnd) = hwnd_of(sh()) else { return };
    let mut d = nid(hwnd);
    d.uFlags = NIF_INFO;
    copy_wstr(&mut d.szInfo, message);
    copy_wstr(&mut d.szInfoTitle, "FaceLock");
    d.dwInfoFlags = NIIF_INFO;
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let Ok(menu) = CreatePopupMenu() else { return };
    let suspended = sh().suspended.load(Ordering::SeqCst);
    let _ = AppendMenuW(menu, MF_STRING, ID_LIVE as usize, w!("Vue en direct"));
    let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS as usize, w!("Paramètres"));
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        ID_SUSPEND as usize,
        if suspended {
            w!("Reprendre")
        } else {
            w!("Suspendre")
        },
    );
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        ID_RESET as usize,
        w!("Réinitialiser la vérification"),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, ID_QUIT as usize, w!("Quitter"));
    let _ = SetMenuDefaultItem(menu, ID_LIVE, 0);
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
        pt.x,
        pt.y,
        None,
        hwnd,
        None,
    );
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
    match cmd.0 as u32 {
        ID_LIVE => open_ui("live"),
        ID_SETTINGS => open_ui("settings"),
        ID_SUSPEND => sh().set_suspended(!suspended),
        ID_RESET => sh().send(MonCmd::Reset),
        ID_QUIT => {
            let _ = DestroyWindow(hwnd);
        }
        _ => {}
    }
}

// --------------------------------------------------------------- hotkeys

/// (id RegisterHotKey, combo, libellé, id du workflow éventuel)
type HotkeySpec = (i32, String, String, Option<u64>);

fn apply_hotkeys(hwnd: HWND, force: bool) {
    let s = sh();
    let cfg = s.cfg();
    let mut wanted: Vec<HotkeySpec> = Vec::new();
    if !s.hotkeys_paused.load(Ordering::SeqCst) {
        wanted.push((
            HK_TOGGLE,
            cfg.hotkeys.toggle.clone(),
            "démarrer/suspendre".into(),
            None,
        ));
        wanted.push((
            HK_RESET,
            cfg.hotkeys.reset.clone(),
            "réinitialiser".into(),
            None,
        ));
        for (i, wf) in cfg.workflows.iter().enumerate().filter(|(_, w)| w.enabled) {
            if let crate::config::Trigger::Hotkey { combo } = &wf.trigger {
                wanted.push((
                    HK_WORKFLOW + i as i32,
                    combo.clone(),
                    format!("« {} »", wf.name),
                    Some(wf.id),
                ));
            }
        }
    }
    wanted.retain(|w| !w.1.trim().is_empty());
    let mut applied = APPLIED_HOTKEYS.lock().unwrap();
    if !force && *applied == wanted {
        return;
    }
    // Pause / reprise pendant l'enregistrement d'un raccourci : pas de bruit.
    let quiet = !force && (wanted.is_empty() || applied.is_empty());
    unsafe {
        for (id, ..) in applied.iter() {
            let _ = UnregisterHotKey(Some(hwnd), *id);
        }
        let mut ok = Vec::new();
        for spec in &wanted {
            let (id, combo, name, _) = spec;
            match hotkey::parse_combo(combo) {
                Ok((mods, vk)) => {
                    if RegisterHotKey(Some(hwnd), *id, HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT, vk)
                        .is_ok()
                    {
                        ok.push(spec.clone());
                        let was = applied.iter().any(|a| a.1 == *combo && a.2 == *name);
                        if !quiet && !was {
                            s.log(format!(
                                "Raccourci actif : {} → {name}",
                                hotkey::pretty(combo)
                            ));
                        }
                    } else {
                        s.log(format!("[erreur] Raccourci {} déjà utilisé (ailleurs ou par un autre workflow)", hotkey::pretty(combo)));
                    }
                }
                Err(e) => s.log(format!("[erreur] Raccourci « {combo} » invalide : {e}")),
            }
        }
        *applied = ok;
    }
}

fn on_hotkey(id: i32) {
    let s = sh();
    let cfg = s.cfg();
    if id >= HK_WORKFLOW {
        let wf_id = APPLIED_HOTKEYS
            .lock()
            .unwrap()
            .iter()
            .find(|a| a.0 == id)
            .and_then(|a| a.3);
        if let Some(wf) = wf_id.and_then(|w| cfg.workflows.iter().find(|x| x.id == w)) {
            crate::monitor::run_workflow(s, wf, Some("raccourci"));
        }
        return;
    }
    match id {
        HK_TOGGLE => {
            let now = !s.suspended.load(Ordering::SeqCst);
            s.set_suspended(now);
            if cfg.notifications.toggle {
                notify(if now {
                    "Surveillance suspendue"
                } else {
                    "Surveillance reprise"
                });
            }
        }
        HK_RESET => {
            s.send(MonCmd::Reset);
            if cfg.notifications.reset {
                notify("Vérification réinitialisée — le check reprend de zéro");
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- wndproc

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    const WTS_SESSION_LOCK: usize = 0x7;
    const WTS_SESSION_UNLOCK: usize = 0x8;
    const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
    match msg {
        WM_TRAY => {
            match lp.0 as u32 {
                WM_LBUTTONUP => open_ui("live"),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            on_hotkey(wp.0 as i32);
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            let s = sh();
            match wp.0 {
                WTS_SESSION_LOCK => {
                    s.locked.store(true, Ordering::SeqCst);
                    s.send(MonCmd::Wake);
                }
                WTS_SESSION_UNLOCK => {
                    s.locked.store(false, Ordering::SeqCst);
                    s.log("Session déverrouillée");
                    s.send(MonCmd::Wake);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wp.0 == PBT_APMRESUMEAUTOMATIC {
                let s = sh();
                s.locked.store(session_locked(), Ordering::SeqCst);
                s.send(MonCmd::Wake);
            }
            LRESULT(1)
        }
        WM_REFRESH => {
            tray_update(hwnd, false);
            LRESULT(0)
        }
        WM_HOTKEYS => {
            apply_hotkeys(hwnd, false);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if Some(&m) == TASKBAR_CREATED.get() => {
            // Explorer a redémarré : on remet l'icône.
            tray_add(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ------------------------------------------------------------------- UI

fn open_ui(page: &str) {
    let s = sh();
    let mut child = UI_CHILD.lock().unwrap();
    if let Some(c) = child.as_mut() {
        if matches!(c.try_wait(), Ok(None)) {
            *s.nav.lock().unwrap() = Some(page.to_string());
            unsafe {
                let _ = AllowSetForegroundWindow(c.id());
            }
            return;
        }
    }
    match std::env::current_exe()
        .and_then(|exe| std::process::Command::new(exe).args(["--ui", page]).spawn())
    {
        Ok(c) => {
            unsafe {
                let _ = AllowSetForegroundWindow(c.id());
            }
            *child = Some(c);
        }
        Err(e) => s.log(format!(
            "[erreur] Ouverture de l'interface impossible : {e}"
        )),
    }
}

// ------------------------------------------------------------------- IPC

fn ipc_server(s: Arc<Shared>) {
    let name: Vec<u16> = ipc::pipe_name().encode_utf16().chain(Some(0)).collect();
    loop {
        let h = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                1 << 16,
                1 << 16,
                0,
                None,
            )
        };
        if h.is_invalid() {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        let connected = unsafe { ConnectNamedPipe(h, None) }.is_ok()
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if !connected {
            unsafe {
                let _ = CloseHandle(h);
            }
            continue;
        }
        let file = unsafe { std::fs::File::from_raw_handle(h.0 as _) };
        let s2 = s.clone();
        let _ = std::thread::Builder::new()
            .name("ipc-client".into())
            .spawn(move || client(s2, file));
    }
}

fn client(s: Arc<Shared>, mut f: std::fs::File) {
    while let Ok((req, _)) = ipc::read_msg::<_, Req>(&mut f) {
        let res = match req {
            Req::Poll {
                log_since,
                want_frame,
                frame_seq,
            } => {
                let mut bin = Arc::new(Vec::new());
                let mut meta = None;
                if want_frame {
                    let was = s.frame_wanted();
                    *s.frame_wanted.lock().unwrap() = Some(Instant::now());
                    if !was {
                        s.send(MonCmd::Wake);
                    }
                    if let Some(p) = s.frame.lock().unwrap().as_ref() {
                        if p.seq != frame_seq {
                            bin = p.bgra.clone();
                            meta = Some(FrameMeta {
                                seq: p.seq,
                                w: p.w,
                                h: p.h,
                                faces: p.faces.clone(),
                            });
                        }
                    }
                }
                let (logs, log_next) = s.logs_since(log_since);
                let status = Status {
                    suspended: s.suspended.load(Ordering::SeqCst),
                    camera_active: s.camera_active.load(Ordering::SeqCst),
                    locked: s.locked.load(Ordering::SeqCst),
                    profiles: s.profiles.load(Ordering::Relaxed) as usize,
                    enroll: s.enroll.lock().unwrap().clone(),
                    logs,
                    log_next,
                    frame: meta,
                    nav: s.nav.lock().unwrap().take(),
                };
                ipc::write_msg(
                    &mut f,
                    &Reply {
                        ok: true,
                        msg: None,
                        status: Some(status),
                    },
                    &bin,
                )
            }
            Req::Cmd { cmd } => {
                let (ok, msg) = handle_cmd(&s, cmd);
                ipc::write_msg(
                    &mut f,
                    &Reply {
                        ok,
                        msg,
                        status: None,
                    },
                    &[],
                )
            }
        };
        if res.is_err() {
            break;
        }
    }
}

fn handle_cmd(s: &Arc<Shared>, cmd: Cmd) -> (bool, Option<String>) {
    match cmd {
        Cmd::ToggleSuspend => s.set_suspended(!s.suspended.load(Ordering::SeqCst)),
        Cmd::Reset => s.send(MonCmd::Reset),
        Cmd::ReloadConfig => {
            *s.cfg.write().unwrap() = Config::load();
            s.send(MonCmd::Wake);
            post_hotkeys(s);
        }
        Cmd::ReloadProfiles => s.send(MonCmd::ReloadProfiles),
        Cmd::EnrollStart => s.send(MonCmd::EnrollStart),
        Cmd::EnrollCapture => s.send(MonCmd::EnrollCapture),
        Cmd::EnrollCancel => s.send(MonCmd::EnrollCancel),
        Cmd::EnrollSave { name } => {
            let (tx, rx) = mpsc::channel();
            s.send(MonCmd::EnrollSave(name, tx));
            return match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(m)) => (true, Some(m)),
                Ok(Err(e)) => (false, Some(e)),
                Err(_) => (false, Some("Pas de réponse du moniteur".into())),
            };
        }
        Cmd::PauseHotkeys { paused } => {
            s.hotkeys_paused.store(paused, Ordering::SeqCst);
            post_hotkeys(s);
        }
        Cmd::OpenUi { page } => open_ui(&page),
        Cmd::RunWorkflow { id } => {
            let cfg = s.cfg();
            match cfg.workflows.iter().find(|w| w.id == id) {
                Some(wf) => crate::monitor::run_workflow(s, wf, Some("test")),
                None => {
                    return (
                        false,
                        Some("Workflow introuvable (enregistrement en cours ?)".into()),
                    )
                }
            }
        }
    }
    (true, None)
}
