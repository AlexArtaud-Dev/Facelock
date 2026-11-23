//! FaceLock — verrouillage automatique par reconnaissance faciale.
//!
//!   facelock                    service résident (icône de notification)
//!   facelock --ui [page]        interface (lancée à la demande par le service)
//!   facelock --toggle           met en pause / relance la surveillance
//!   facelock --reset            réinitialise la vérification
//!   facelock --run "<nom>"      exécute un workflow (raccourcis du bureau, scripts)
//!   facelock --quit             arrête le service
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

use ipc::Cmd;

const HELP: &str = "FaceLock — verrouillage automatique par reconnaissance faciale

Usage :
  facelock                  démarre le service (icône de notification)
  facelock --ui [page]      ouvre l'interface (live, workflows, faces, settings)
  facelock --toggle         met en pause / relance la surveillance
  facelock --reset          réinitialise la vérification
  facelock --run \"<nom>\"    exécute le workflow portant ce nom
  facelock --quit           arrête le service
  facelock --version        affiche la version";

fn send(cmd: Cmd) -> i32 {
    match ipc::send_once(cmd) {
        Some(r) if r.ok => 0,
        Some(r) => {
            eprintln!("facelock : {}", r.msg.unwrap_or_else(|| "échec".into()));
            1
        }
        None => {
            eprintln!("facelock : le service n'est pas démarré");
            2
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        None => {
            daemon::run();
            0
        }
        Some("--ui") => {
            ui::run(args.get(1).map(String::as_str).unwrap_or("live"));
            0
        }
        Some("--toggle") => send(Cmd::ToggleSuspend),
        Some("--reset") => send(Cmd::Reset),
        Some("--quit") => send(Cmd::Quit),
        Some("--run") => match args.get(1) {
            Some(name) => send(Cmd::RunNamed { name: name.clone() }),
            None => {
                eprintln!("facelock : --run attend le nom d'un workflow");
                2
            }
        },
        Some("--version" | "-V") => {
            println!("facelock {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--help" | "-h") => {
            println!("{HELP}");
            0
        }
        Some(other) => {
            eprintln!("facelock : option inconnue « {other} »\n\n{HELP}");
            2
        }
    };
    std::process::exit(code);
}
