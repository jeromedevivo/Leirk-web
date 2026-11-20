; Leirk installer (Inno Setup 6). Built by scripts/package.ps1, which passes the version.
#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif

[Setup]
AppId={{6E0C2B7A-4C1D-4E7B-9C55-1E2F3A4B5C6D}
AppName=Leirk
AppVersion={#AppVersion}
AppVerName=Leirk {#AppVersion}
AppPublisher=Jérôme De Vivo
AppPublisherURL=https://github.com/jeromedevivo/Leirk-web
AppSupportURL=https://github.com/jeromedevivo/Leirk-web/issues
DefaultDirName={localappdata}\Programs\Leirk
DefaultGroupName=Leirk
DisableProgramGroupPage=yes
; per-user install: no administrator prompt
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=..\dist
OutputBaseFilename=Leirk-Setup-{#AppVersion}
SetupIconFile=..\assets\leirk.ico
UninstallDisplayIcon={app}\leirk.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\leirk.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Leirk"; Filename: "{app}\leirk.exe"
Name: "{group}\{cm:UninstallProgram,Leirk}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Leirk"; Filename: "{app}\leirk.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\leirk.exe"; Description: "{cm:LaunchProgram,Leirk}"; Flags: nowait postinstall skipifsilent

; The profile (%LOCALAPPDATA%\Leirk) is kept on uninstall, like other browsers.
