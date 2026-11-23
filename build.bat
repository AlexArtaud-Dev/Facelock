@echo off
rem Compile FaceLock (Rust) puis genere Output\FaceLock-Setup.exe (Inno Setup 6)
cargo build --release || exit /b 1
set ISCC="%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe"
if not exist %ISCC% (
  echo Inno Setup 6 introuvable : seul target\release\facelock.exe a ete produit.
  exit /b 0
)
%ISCC% installer.iss
