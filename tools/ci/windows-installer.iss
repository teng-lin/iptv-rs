; Release-only, per-user installer. The stable AppId makes upgrades replace the app.
#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef StageDir
  #error StageDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif
[Setup]
AppId=com.hantekllc.iptvrs
AppName=Patina
AppVersion={#AppVersion}
AppPublisher=Patina maintainers
AppPublisherURL=https://github.com/teng-lin/patina-iptv
DefaultDirName={localappdata}\Programs\Patina
DefaultGroupName=Patina
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=Patina-{#AppVersion}-windows-x86_64-setup
SetupIconFile={#StageDir}\icon.ico
UninstallDisplayIcon={app}\Patina.exe
LicenseFile={#StageDir}\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{autoprograms}\Patina"; Filename: "{app}\Patina.exe"; AppUserModelID: "com.hantekllc.iptvrs"
[Run]
Filename: "{app}\Patina.exe"; Description: "Launch Patina"; Flags: nowait postinstall skipifsilent
; No UninstallDelete entries: playlists, settings and credentials are user data.
