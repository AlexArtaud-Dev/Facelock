//! FaceLock — verrouillage automatique par reconnaissance faciale (Windows).
//!
//!   facelock.exe            -> service résident (icône de notification)
//!   facelock.exe --ui live  -> interface (lancée à la demande par le service)
#![windows_subsystem = "windows"]

mod actions;
mod camera;
mod config;
mod daemon;
mod engine;
mod hotkey;
mod icon;
mod ipc;
mod monitor;
mod state;
mod ui;
mod winutil;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.iter().position(|a| a == "--ui") {
        Some(i) => ui::run(args.get(i + 1).map(String::as_str).unwrap_or("live")),
        None => daemon::run(),
    }
}
