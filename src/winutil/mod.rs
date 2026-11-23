//! Utilitaires système côté interface : liste des applications (avec icônes
//! quand le système le permet), démarrage automatique, ouverture de dossier.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

/// Taille des icônes d'application (pixels).
pub const ICON: usize = 32;

pub struct ProcEntry {
    pub name: String,
    /// RGBA ICON x ICON
    pub icon: Option<Vec<u8>>,
}
