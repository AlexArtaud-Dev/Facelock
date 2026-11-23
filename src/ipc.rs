//! IPC service <-> interface / ligne de commande. Transport local :
//! pipe nommé (Windows) ou socket Unix 0600 (Linux). Messages
//! [u32 len][json][u32 len][binaire] (le binaire transporte l'image BGRA).

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[cfg(windows)]
pub type Stream = std::fs::File;
#[cfg(unix)]
pub type Stream = std::os::unix::net::UnixStream;

#[cfg(windows)]
pub fn endpoint() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    let user: String = user.chars().filter(|c| c.is_alphanumeric()).collect();
    format!(r"\\.\pipe\FaceLock-{user}")
}

#[cfg(unix)]
pub fn endpoint() -> String {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => std::path::Path::new(&dir).join("facelock.sock").display().to_string(),
        None => format!("/tmp/facelock-{}.sock", unsafe { libc::getuid() }),
    }
}

/// Connexion cliente au service (None s'il ne tourne pas).
#[cfg(windows)]
pub fn connect() -> Option<Stream> {
    std::fs::OpenOptions::new().read(true).write(true).open(endpoint()).ok()
}

#[cfg(unix)]
pub fn connect() -> Option<Stream> {
    std::os::unix::net::UnixStream::connect(endpoint()).ok()
}

/// Boucle d'acceptation : chaque client est servi dans son propre thread.
#[cfg(windows)]
pub fn serve(handler: impl Fn(Stream) + Send + Sync + 'static) {
    use std::os::windows::io::FromRawHandle;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_PIPE_CONNECTED};
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::*;
    let handler = std::sync::Arc::new(handler);
    let name: Vec<u16> = endpoint().encode_utf16().chain(Some(0)).collect();
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
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }
        let connected =
            unsafe { ConnectNamedPipe(h, None) }.is_ok() || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if !connected {
            unsafe {
                let _ = CloseHandle(h);
            }
            continue;
        }
        let file = unsafe { std::fs::File::from_raw_handle(h.0 as _) };
        let handler = handler.clone();
        let _ = std::thread::Builder::new()
            .name("ipc-client".into())
            .spawn(move || handler(file));
    }
}

#[cfg(unix)]
pub fn serve(handler: impl Fn(Stream) + Send + Sync + 'static) {
    use std::os::unix::fs::PermissionsExt;
    let path = endpoint();
    // Socket orphelin d'une instance précédente (le verrou d'instance unique
    // a déjà été vérifié par une tentative de connexion).
    let _ = std::fs::remove_file(&path);
    let listener = match std::os::unix::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("facelock: socket {path} : {e}");
            return;
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    let handler = std::sync::Arc::new(handler);
    for stream in listener.incoming().flatten() {
        let handler = handler.clone();
        let _ = std::thread::Builder::new()
            .name("ipc-client".into())
            .spawn(move || handler(stream));
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "t")]
pub enum Cmd {
    ToggleSuspend,
    Reset,
    ReloadConfig,
    ReloadProfiles,
    EnrollStart,
    EnrollCapture,
    EnrollSave {
        name: String,
    },
    EnrollCancel,
    PauseHotkeys {
        paused: bool,
    },
    OpenUi {
        page: String,
    },
    RunWorkflow {
        id: u64,
    },
    /// Exécute un workflow par son nom (ligne de commande, raccourcis Wayland)
    RunNamed {
        name: String,
    },
    Quit,
    DismissOverlay,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "t")]
pub enum Req {
    Poll {
        log_since: u64,
        want_frame: bool,
        frame_seq: u64,
    },
    Cmd {
        cmd: Cmd,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FaceBox {
    pub bbox: [f32; 4],
    pub name: Option<String>,
    pub score: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FrameMeta {
    pub seq: u64,
    pub w: usize,
    pub h: usize,
    pub faces: Vec<FaceBox>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct EnrollInfo {
    pub count: usize,
    pub face_ok: bool,
    pub several: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Status {
    pub suspended: bool,
    pub camera_active: bool,
    pub locked: bool,
    pub profiles: usize,
    pub enroll: Option<EnrollInfo>,
    pub logs: Vec<String>,
    pub log_next: u64,
    pub frame: Option<FrameMeta>,
    pub nav: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Reply {
    pub ok: bool,
    pub msg: Option<String>,
    pub status: Option<Status>,
}

pub fn write_msg<W: Write, T: Serialize>(w: &mut W, msg: &T, bin: &[u8]) -> std::io::Result<()> {
    let json = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    let mut buf = Vec::with_capacity(8 + json.len() + bin.len());
    buf.extend_from_slice(&(json.len() as u32).to_le_bytes());
    buf.extend_from_slice(&json);
    buf.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    buf.extend_from_slice(bin);
    w.write_all(&buf)?;
    w.flush()
}

pub fn read_msg<R: Read, T: for<'de> Deserialize<'de>>(r: &mut R) -> std::io::Result<(T, Vec<u8>)> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let n = u32::from_le_bytes(len) as usize;
    if n > 16 << 20 {
        return Err(std::io::Error::other("message trop grand"));
    }
    let mut json = vec![0u8; n];
    r.read_exact(&mut json)?;
    r.read_exact(&mut len)?;
    let n = u32::from_le_bytes(len) as usize;
    if n > 64 << 20 {
        return Err(std::io::Error::other("image trop grande"));
    }
    let mut bin = vec![0u8; n];
    r.read_exact(&mut bin)?;
    let msg = serde_json::from_slice(&json).map_err(std::io::Error::other)?;
    Ok((msg, bin))
}

/// Envoi ponctuel d'une commande (utilisé par une 2e instance).
pub fn send_once(cmd: Cmd) -> Option<Reply> {
    let mut f = connect()?;
    write_msg(&mut f, &Req::Cmd { cmd }, &[]).ok()?;
    read_msg::<_, Reply>(&mut f).ok().map(|(r, _)| r)
}
