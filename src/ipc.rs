//! IPC daemon <-> interface : pipe nommé local, messages
//! [u32 len][json][u32 len][binaire] (le binaire transporte l'image BGRA).

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub fn pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    let user: String = user.chars().filter(|c| c.is_alphanumeric()).collect();
    format!(r"\\.\pipe\FaceLock-{user}")
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
    EnrollSave { name: String },
    EnrollCancel,
    PauseHotkeys { paused: bool },
    OpenUi { page: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "t")]
pub enum Req {
    Poll { log_since: u64, want_frame: bool, frame_seq: u64 },
    Cmd { cmd: Cmd },
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

/// Connexion cliente au daemon (None s'il ne tourne pas).
pub fn connect() -> Option<std::fs::File> {
    std::fs::OpenOptions::new().read(true).write(true).open(pipe_name()).ok()
}

/// Envoi ponctuel d'une commande (utilisé par une 2e instance).
pub fn send_once(cmd: Cmd) -> bool {
    let Some(mut f) = connect() else { return false };
    if write_msg(&mut f, &Req::Cmd { cmd }, &[]).is_err() {
        return false;
    }
    read_msg::<_, Reply>(&mut f).is_ok()
}
