; Inno Setup script for AlwaysEnglish (Rust + Slint edition).
; Build with packaging\build-release.ps1, or:
;   ISCC.exe /DAppVersion=2.0.0 packaging\AlwaysEnglish.iss
; The original Qt version (https://github.com/ChaXxl/AlwaysEnglish) also shipped an
; Inno Setup installer; this script follows it.

#ifndef AppVersion
  #define AppVersion "2.0.0"
#endif
#define AppName "AlwaysEnglish"
#define AppExeName "AlwaysEnglish.exe"
#define AppPublisher "just-liuyi (based on AlwaysEnglish by ChaXxl)"
#define AppURL "https://github.com/just-liuyi/AlwaysEnglish-rs"
#define SourceExe "..\target\release\AlwaysEnglish.exe"

[Setup]
; A new AppId, so it doesn't collide with the original Qt version's installer.
AppId={{2AFB8310-CDA1-4EE9-BF54-30DFB923CB2E}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
AppCopyright=Apache-2.0. Based on AlwaysEnglish by ChaXxl.
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} Setup
DefaultDirName={autopf}\AlwaysEnglish-rs
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Per-user install by default (no administrator rights needed); "all users" can be chosen.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
SetupIconFile=..\assets\favicon.ico
UninstallDisplayIcon={app}\{#AppExeName}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=force
RestartApplications=no
OutputDir=..\dist
OutputBaseFilename=AlwaysEnglish-{#AppVersion}-setup-x64

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "ChineseSimplified.isl"

[CustomMessages]
english.AutoStart=Run AlwaysEnglish when I sign in to Windows (starts hidden in the tray)
chinesesimplified.AutoStart=开机自启动（登录 Windows 后在托盘中运行）
english.Startup=Startup:
chinesesimplified.Startup=启动：

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "{cm:AutoStart}"; GroupDescription: "{cm:Startup}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\NOTICE"; DestDir: "{app}"; DestName: "NOTICE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Registry]
; Same value the app writes from Settings > Running at start.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "AlwaysEnglish"; ValueData: """{app}\{#AppExeName}"" --minimized"; Tasks: autostart

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExeName}"; Flags: runhidden; RunOnceId: "StopAlwaysEnglish"

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Value: String;
begin
  // Remove the startup entry, but only if it points at this installation
  // (the original Qt version uses the same value name).
  if CurUninstallStep = usPostUninstall then
    if RegQueryStringValue(HKCU, RunKey, 'AlwaysEnglish', Value) then
      if Pos(Lowercase(ExpandConstant('{app}')), Lowercase(Value)) > 0 then
        RegDeleteValue(HKCU, RunKey, 'AlwaysEnglish');
end;
