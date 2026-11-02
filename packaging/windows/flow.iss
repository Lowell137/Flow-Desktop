; Inno Setup script for Flow Desktop (Windows x64).
; Build: pnpm tauri build --no-bundle --config src-tauri/tauri.local.conf.json
;        ISCC packaging\windows\flow.iss /DAppVersion=0.1.0-beta3

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\src-tauri"
#endif

#define AppName "Flow"
#define AppExe "flow.exe"

[Setup]
AppId={{6B0E2C54-3D1F-4F0B-9B7A-5A1E4C2D8F10}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=A-EDev
AppPublisherURL=https://github.com/Lowell137/Flow-Desktop
AppSupportURL=https://github.com/Lowell137/Flow-Desktop/issues
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist-installer
OutputBaseFilename=Flow-Setup-{#AppVersion}-x64
SetupIconFile={#SourceDir}\icons\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
LicenseFile=..\..\LICENSE
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "turkish"; MessagesFile: "compiler:Languages\Turkish.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\sidecar\integrity.cjs"; DestDir: "{app}\sidecar"; Flags: ignoreversion
Source: "{#SourceDir}\sidecar\nsig.cjs"; DestDir: "{app}\sidecar"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

; flow:// deep links (tauri-plugin-deep-link)
[Registry]
Root: HKA; Subkey: "Software\Classes\flow"; ValueType: string; ValueData: "URL:Flow"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\flow"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\flow\DefaultIcon"; ValueType: string; ValueData: "{app}\{#AppExe},0"
Root: HKA; Subkey: "Software\Classes\flow\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""

[Run]
Filename: "{tmp}\MicrosoftEdgeWebview2Setup.exe"; Parameters: "/silent /install"; StatusMsg: "Installing Microsoft Edge WebView2 Runtime..."; Check: NeedsWebView2; Flags: waituntilterminated
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
const
  WebView2Guid = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';

function WebView2Installed: Boolean;
var
  Ver: String;
begin
  Result :=
    (RegQueryStringValue(HKLM, 'SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\' + WebView2Guid, 'pv', Ver) and (Ver <> '') and (Ver <> '0.0.0.0')) or
    (RegQueryStringValue(HKCU, 'Software\Microsoft\EdgeUpdate\Clients\' + WebView2Guid, 'pv', Ver) and (Ver <> '') and (Ver <> '0.0.0.0'));
end;

function NeedsWebView2: Boolean;
begin
  Result := not WebView2Installed;
end;

function NodeOnPath: Boolean;
begin
  Result := (FileSearch('node.exe', GetEnv('PATH')) <> '');
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not WebView2Installed then
  begin
    try
      DownloadTemporaryFile('https://go.microsoft.com/fwlink/p/?LinkId=2124703', 'MicrosoftEdgeWebview2Setup.exe', '', nil);
    except
      Result := 'Could not download the Microsoft Edge WebView2 Runtime: ' + GetExceptionMessage;
    end;
  end;
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if (CurPageID = wpFinished) and (not NodeOnPath) and (not WizardSilent) then
    MsgBox('Node.js was not found on PATH. Flow uses it to mint playback tokens; install Node.js 22+ (https://nodejs.org) for reliable YouTube playback.', mbInformation, MB_OK);
end;
