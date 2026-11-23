<div align="center">

<img src="assets/logo.png" width="112" alt="FaceLock">

# FaceLock

### Ton PC connaît ton visage. Et il ne fait confiance à personne d'autre.

*Verrouillage et confidentialité par reconnaissance faciale — Windows et Linux, 100 % local, 0 % CPU au repos.*

[![CI](https://github.com/AlexArtaud-Dev/Facelock/actions/workflows/ci.yml/badge.svg)](https://github.com/AlexArtaud-Dev/Facelock/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/AlexArtaud-Dev/Facelock?label=release)](https://github.com/AlexArtaud-Dev/Facelock/releases/latest)
![Windows](https://img.shields.io/badge/Windows-10%2F11-0078D6?logo=windows&logoColor=white)
![Linux](https://img.shields.io/badge/Linux-X11%20%7C%20Wayland-FCC624?logo=linux&logoColor=black)
![Rust](https://img.shields.io/badge/Rust-1.95+-B7410E?logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-yellow)

</div>

---

Tu te lèves, tu oublies `Win+L`, et ton écran raconte ta vie à tout l'open space. **FaceLock** regarde discrètement par ta webcam : dès que *ton* visage disparaît, ou qu'un inconnu s'installe ou lit par-dessus ton épaule, il déclenche **tes workflows**.

## ✨ Fonctionnalités

- 🎯 **Reconnaissance, pas seulement détection** : seuls les visages enrôlés comptent comme « toi ».
- ⚡ **Workflows visuels** : un déclencheur (*je m'absente*, *visage inconnu*, *je reviens*, *raccourci clavier*) suivi d'une chaîne d'actions, construits bloc par bloc, activables un par un, avec 9 modèles prêts à l'emploi.
- 🛡️ **Écran de confidentialité** : un message « Accès non autorisé » ou ton image, en plein écran par-dessus tout le reste. Il se ferme avec un **raccourci de secours obligatoire** (qui marche même si l'écran se fige) ou automatiquement à ton retour.
- 🧰 **13 actions** : verrouiller, écran de confidentialité, changer de bureau virtuel, afficher le bureau, minimiser / fermer / déplacer une appli (par processus et titre de fenêtre), lecture/pause, couper le son, éteindre l'écran, mise en veille, notification, attendre.
- 🪶 **Ultra léger** : un seul exécutable, 0 % CPU au repos (événements système, pas de sondage), caméra coupée dès que la session est verrouillée.
- 🔒 **Vie privée** : aucune photo stockée, uniquement des empreintes 128-D ; rien ne quitte ta machine.

## 🚀 Installation

Télécharge la [dernière release](https://github.com/AlexArtaud-Dev/Facelock/releases/latest).

| Système | Fichier | |
|---|---|---|
| Windows 10/11 | `FaceLock-Setup-x.y.z.exe` | installation par utilisateur, sans droits admin |
| Windows (portable) | `FaceLock-x.y.z-portable-x64.exe` | un seul fichier, rien à installer |
| Debian / Ubuntu | `facelock_x.y.z_amd64.deb` | `sudo apt install ./facelock_x.y.z_amd64.deb` |
| Autres Linux | `facelock-x.y.z-linux-x86_64.tar.gz` | binaire + `.desktop` + icône |

Les sommes de contrôle sont dans `SHA256SUMS-*.txt`. Au premier lancement : **Visages → Ajouter un visage** (12 captures, une minute).

### Linux : bon à savoir

- Icône de notification : KDE, XFCE, Cinnamon… nativement ; GNOME avec l'extension *AppIndicator*.
- Actions système via les outils standard : `loginctl` / `systemctl` (systemd), `xdotool` et `wmctrl` (fenêtres, bureaux), `playerctl`, `pactl`, `xset`. Le `.deb` les recommande ; un outil manquant est signalé dans le journal.
- Raccourcis globaux : natifs sous X11. Sous **Wayland**, associe un raccourci du bureau à `facelock --run "<nom du workflow>"`.

## ⌨️ Ligne de commande

```text
facelock                    démarre le service (icône de notification)
facelock --ui [page]        ouvre l'interface (live, workflows, faces, settings)
facelock --toggle           met en pause / relance la surveillance
facelock --reset            réinitialise la vérification
facelock --run "<nom>"      exécute un workflow
facelock --dismiss          ferme l'écran de confidentialité
facelock --quit             arrête le service
```

## 🏗️ Architecture

```
facelock               service résident, aucune bibliothèque graphique chargée
├─ système             icône, raccourcis globaux, verrouillage (Win32/WTS ou SNI/X11/logind)
├─ monitor             webcam → YuNet → SFace → déclencheurs → workflows
└─ ipc                 pipe nommé (Windows) / socket Unix 0600 (Linux)

facelock --ui          interface egui, lancée à la demande, mémoire libérée à la fermeture
facelock --overlay     écran de confidentialité (processus séparé, tué par le service)
```

Le moniteur dort entre deux vérifications et se réveille instantanément sur commande. Un visage déjà identifié n'est ré-identifié que s'il bouge ou toutes les 3 s : une vérification coûte environ 10 ms. L'inférence passe par [tract](https://github.com/sonos/tract), en pur Rust et sans DLL. Elle donne les **mêmes résultats qu'OpenCV** (cosinus des empreintes ≥ 0,999), ce qui garde valable le seuil recommandé de 0,363.

| Dossier | Rôle |
|---|---|
| `src/daemon/` | service : cycle de vie, IPC, raccourcis, écran de confidentialité (+ `windows.rs` / `linux.rs`) |
| `src/monitor.rs` | boucle de surveillance, cache d'identité, moteur de workflows, enrôlement |
| `src/engine.rs` | YuNet + SFace (pré/post-traitement identique à OpenCV) |
| `src/camera/` | Media Foundation (Windows), V4L2 (Linux) |
| `src/actions/` | catalogue des actions + implémentations par système |
| `src/ui/` | interface : thème verre, éditeur de workflows, écran de confidentialité |

## 🛠️ Développement

```bash
cargo run                     # service
cargo run -- --ui workflows   # interface seule
cargo test                    # tests unitaires
cargo run --release --example selftest   # auto-test des modèles
scripts/package.sh 0.0.0-dev dist        # installeurs (Inno Setup / dpkg-deb)
```

Linux : `sudo apt install libclang-dev` (en-têtes V4L2). La version du compilateur est fixée par `rust-toolchain.toml`.

Les commits suivent [Conventional Commits](https://www.conventionalcommits.org/fr/) : voir [CONTRIBUTING.md](CONTRIBUTING.md). Les releases sont automatiques : *release-please* tient à jour une PR de release (changelog et version) ; la fusionner publie la release GitHub avec les installeurs Windows et Linux.

## 🔄 Depuis la version Python (1.x)

- `config.json` est migré automatiquement vers les workflows (copie de l'ancien fichier dans `config.v1.json`).
- Les profils de visage `.npz` ne sont pas importés : ré-enrôle-toi (une minute).
- Les raccourcis prennent des modificateurs plus **une** touche (`ctrl+alt+p`).

## 🤔 FAQ

**Peut-on le tromper avec une photo ?** Une bonne photo tenue bien droite, probablement. FaceLock protège contre les oublis de verrouillage et les curieux opportunistes, pas contre un espion déterminé ; pour de l'anti-spoofing matériel, utilise Windows Hello avec une caméra IR.

**Et si l'écran de confidentialité se fige ?** Le raccourci de secours est capté par le service, pas par la fenêtre : il ferme l'écran même si celui-ci ne répond plus. En dernier recours, il reste `facelock --dismiss`.

## 📄 Licence

MIT, voir [LICENSE](LICENSE). Modèles [YuNet](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet) (MIT) et [SFace](https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface) (Apache 2.0) issus d'opencv_zoo ; icônes [Phosphor](https://phosphoricons.com/) (MIT).
