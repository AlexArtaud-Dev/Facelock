<div align="center">

# 🔐 FaceLock (Rust)

### Ton PC connaît ton visage. Et il ne fait confiance à personne d'autre.

*Verrouillage automatique par reconnaissance faciale pour Windows : un seul `.exe`, 100 % local, 0 % CPU au repos.*

</div>

---

Réécriture complète en Rust de [FaceLock](https://github.com/AlexArtaud-Dev/Facelock) : mêmes fonctionnalités, même `config.json`, mais sans Python, OpenCV ni Tk.

## Ce qui change

| | Python (v1) | Rust (v2) |
|---|---|---|
| Distribution | dossier PyInstaller + modèles | **un seul `FaceLock.exe`** (modèles embarqués) |
| CPU au repos (session verrouillée, suspendu) | sondage toutes les 5 s | **0** : réveil par événement Windows |
| Détection du verrouillage | `OpenInputDesktop` en boucle | `WTSRegisterSessionNotification` |
| Raccourcis globaux | hook clavier sur **chaque** frappe (`keyboard`) | `RegisterHotKey` : le noyau ne réveille FaceLock que sur le combo |
| Reconnaissance | SFace à chaque vérification | SFace seulement si un visage bouge, sinon toutes les 3 s (détection seule ≈ 10 ms) |
| Interface | Tk chargé en permanence | processus séparé lancé à la demande, **mémoire libérée à la fermeture** |
| Inférence | OpenCV DNN | [tract](https://github.com/sonos/tract), pur Rust, aucune DLL |

Les détections et empreintes sont **identiques à OpenCV** (vérifié : mêmes boîtes, repères à < 0,25 px, cosinus des empreintes ≥ 0,999), donc le seuil recommandé de 0,363 reste valable.

## Architecture

```
FaceLock.exe                    (service résident, aucune lib graphique)
├─ thread principal   icône de notification Win32, menu, RegisterHotKey, événements WTS
├─ thread monitor     Media Foundation → YuNet → SFace → déclencheurs → actions
└─ thread ipc         pipe nommé \\.\pipe\FaceLock-<utilisateur>

FaceLock.exe --ui <page>        (interface egui, lancée au clic sur l'icône)
```

Le monitor dort sur un canal entre deux vérifications : une commande (suspendre, réinitialiser, verrouillage, enrôlement) le réveille instantanément.
Session verrouillée, surveillance suspendue ou aucun profil : la caméra est libérée (LED éteinte) et le thread dort indéfiniment.

| Fichier | Rôle |
|---|---|
| `src/daemon.rs` | tray, menu, raccourcis, WTS, serveur IPC, lancement de l'UI |
| `src/monitor.rs` | boucle de surveillance, cache d'identité, déclencheurs, enrôlement |
| `src/engine.rs` | YuNet + SFace (pré/post-traitement identique à OpenCV) |
| `src/camera.rs` | capture Media Foundation synchrone à la demande |
| `src/actions.rs` | verrouiller, veille, bureaux virtuels, fermer / minimiser / déplacer une appli |
| `src/ui.rs` | interface : vue en direct, réglages, enrôlement, sélecteur de processus |
| `src/config.rs` | `config.json` (compatible v1) |

## Compiler

Rust stable (1.91+, cible MSVC) :

```powershell
cargo build --release
```

→ `target\release\facelock.exe`. Pour l'installeur ([Inno Setup 6](https://jrsoftware.org/isinfo.php)) : `build.bat` → `Output\FaceLock-Setup.exe`.
L'installeur reprend l'`AppId` de la v1 : il met à jour l'installation Python sur place.

## Migration depuis la v1

- `config.json` est relu tel quel (raccourcis, déclencheurs, panic button…).
- Les profils de visage v1 (`.npz`) ne sont pas importés : ré-enrôle-toi dans **Visages** (12 captures, une minute).
- Les raccourcis acceptent **une** touche principale plus des modificateurs (`ctrl+alt+p`). Les combos à plusieurs touches normales de la v1 (ex. `a+b`) ne sont plus possibles avec `RegisterHotKey`.

## Modèles

[YuNet](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet) (MIT) et [SFace](https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface) (Apache 2.0), issus d'opencv_zoo, embarqués dans l'exécutable.

## Licence

MIT. Voir [LICENSE](LICENSE).
