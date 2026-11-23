; Inno Setup 6 — génère <OutputDir>\FaceLock-Setup-<version>.exe
;   ISCC /DAppVersion=x.y.z /DOutputDir=dist installer.iss   (voir scripts/package.sh)
; Même AppId que la version Python : l'installation se met à jour sur place.
; Installation PAR UTILISATEUR (pas d'UAC) dans %LocalAppData%\Programs\FaceLock.
; Les données (config, visages) vivent dans %AppData%\FaceLock.

#define AppName "FaceLock"
#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#ifndef OutputDir
  #define OutputDir "Output"
#endif
#define AppExe "FaceLock.exe"

[Setup]
AppId={{7F3C1B7E-5A24-4D1B-9C33-FACEL0CK0001}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=AlexArtaud-Dev
DefaultDirName={localappdata}\Programs\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename={#AppName}-Setup-{#AppVersion}
SetupIconFile=assets\facelock.ico
UninstallDisplayIcon={app}\{#AppExe}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "desktopicon"; Description: "Créer une icône sur le Bureau"; \
    GroupDescription: "Raccourcis :"; Flags: unchecked
Name: "autostart"; Description: "Lancer FaceLock à l'ouverture de session Windows"; \
    GroupDescription: "Démarrage :"

[InstallDelete]
; Restes de la version Python (runtime PyInstaller, modèles séparés)
Type: filesandordirs; Name: "{app}\_internal"
Type: filesandordirs; Name: "{app}\models"

[Files]
Source: "target\release\facelock.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; \
    ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"""; \
    Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#AppExe}"; Description: "Lancer {#AppName} maintenant"; \
    Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}"
