#!/usr/bin/env bash
# Construit les paquets de distribution à partir de target/release.
#
#   scripts/package.sh <version> <dossier de sortie>
#
# Windows : installeur Inno Setup (FaceLock-Setup-<v>.exe) + exécutable portable.
# Linux   : paquet Debian (.deb) + archive (.tar.gz).
# Chaque système produit aussi SHA256SUMS-<os>.txt.
set -euo pipefail

VERSION="${1:?version requise}"
OUT="${2:?dossier de sortie requis}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
cd "$ROOT"
BIN="${CARGO_TARGET_DIR:-target}/release"

case "$(uname -s)" in
MINGW* | MSYS* | CYGWIN*)
    export MSYS_NO_PATHCONV=1
    ISCC="/c/Program Files (x86)/Inno Setup 6/ISCC.exe"
    if [ ! -f "$ISCC" ]; then
        choco install innosetup -y --no-progress >/dev/null
    fi
    "$ISCC" /Q "/DAppVersion=$VERSION" "/DOutputDir=$OUT" installer.iss
    cp "$BIN/facelock.exe" "$OUT/FaceLock-$VERSION-portable-x64.exe"
    (cd "$OUT" && sha256sum FaceLock-* >SHA256SUMS-windows.txt)
    ;;
Linux)
    ARCH="$(dpkg --print-architecture 2>/dev/null || echo amd64)"
    PKG="$(mktemp -d)/facelock_${VERSION}_${ARCH}"
    install -Dm755 "$BIN/facelock" "$PKG/usr/bin/facelock"
    install -Dm644 assets/linux/facelock.desktop "$PKG/usr/share/applications/facelock.desktop"
    install -Dm644 assets/logo.png "$PKG/usr/share/icons/hicolor/512x512/apps/facelock.png"
    install -Dm644 LICENSE "$PKG/usr/share/doc/facelock/copyright"
    install -Dm644 README.md "$PKG/usr/share/doc/facelock/README.md"
    SIZE="$(du -sk "$PKG/usr" | cut -f1)"
    mkdir -p "$PKG/DEBIAN"
    cat >"$PKG/DEBIAN/control" <<EOF
Package: facelock
Version: $VERSION
Architecture: $ARCH
Maintainer: AlexArtaud-Dev <https://github.com/AlexArtaud-Dev>
Installed-Size: $SIZE
Depends: libc6 (>= 2.31), libgl1, libegl1, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libxcursor1, libxrandr2, libxi6, libwayland-client0
Recommends: systemd, xdotool, wmctrl, playerctl, pulseaudio-utils | pipewire-pulse, x11-xserver-utils
Suggests: gnome-shell-extension-appindicator
Section: utils
Priority: optional
Homepage: https://github.com/AlexArtaud-Dev/Facelock
Description: Verrouillage automatique par reconnaissance faciale
 FaceLock surveille la webcam et déclenche des workflows (verrouillage,
 écran de confidentialité, bureaux virtuels…) quand tu t'absentes ou qu'un
 visage inconnu apparaît. Reconnaissance 100 % locale.
EOF
    dpkg-deb --build --root-owner-group "$PKG" "$OUT/facelock_${VERSION}_${ARCH}.deb" >/dev/null

    TAR="$(mktemp -d)/facelock-$VERSION-linux-x86_64"
    mkdir -p "$TAR"
    cp "$BIN/facelock" assets/linux/facelock.desktop assets/logo.png LICENSE README.md "$TAR/"
    tar -C "$(dirname "$TAR")" -czf "$OUT/facelock-$VERSION-linux-x86_64.tar.gz" "$(basename "$TAR")"
    (cd "$OUT" && sha256sum facelock* >SHA256SUMS-linux.txt)
    ;;
*)
    echo "Système non pris en charge : $(uname -s)" >&2
    exit 1
    ;;
esac
ls -la "$OUT"
