#ifndef AppVersion
  #error Pass the version, for example: iscc /DAppVersion=0.1.0 bawkbit.iss
#endif
#define Classes "Software\Classes"
#define Run "Software\Microsoft\Windows\CurrentVersion\Run"

[Setup]
AppId={{8C41E2B7-5D3A-4F60-9B1E-7A2D6C9E4F15}
AppName=bawkbit
AppVersion={#AppVersion}
AppPublisher=juddisjudd
AppPublisherURL=https://github.com/juddisjudd/bawkbit
AppSupportURL=https://github.com/juddisjudd/bawkbit/issues
AppUpdatesURL=https://github.com/juddisjudd/bawkbit/releases
CloseApplications=yes
RestartApplications=no
DefaultDirName={autopf}\bawkbit
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
ChangesAssociations=yes
SourceDir=..\..
OutputDir=dist
OutputBaseFilename=bawkbit-v{#AppVersion}-windows-x64-setup
SetupIconFile=assets\brand\bawkbit.ico
UninstallDisplayIcon={app}\bawkbit.exe
UninstallDisplayName=bawkbit
VersionInfoVersion={#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern dynamic

[Tasks]
Name: links; Description: "Open magnet links and .torrent files with bawkbit"
Name: desktopicon; Description: "Put bawkbit on the desktop"; Flags: unchecked

[Files]
Source: "target\release\bawkbit.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "target\release\bawkbitd.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\bawkbit"; Filename: "{app}\bawkbit.exe"
Name: "{autodesktop}\bawkbit"; Filename: "{app}\bawkbit.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "{#Classes}\magnet"; ValueType: string; ValueName: ""; ValueData: "URL:Magnet link"; Tasks: links
Root: HKCU; Subkey: "{#Classes}\magnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKCU; Subkey: "{#Classes}\magnet\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\bawkbit.exe"" ""%1"""; Tasks: links
Root: HKCU; Subkey: "{#Classes}\bawkbit.torrent"; ValueType: string; ValueName: ""; ValueData: "Torrent file"; Flags: uninsdeletekey; Tasks: links
Root: HKCU; Subkey: "{#Classes}\bawkbit.torrent\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\bawkbit.exe"",0"; Tasks: links
Root: HKCU; Subkey: "{#Classes}\bawkbit.torrent\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\bawkbit.exe"" ""%1"""; Tasks: links
Root: HKCU; Subkey: "{#Classes}\.torrent"; ValueType: string; ValueName: ""; ValueData: "bawkbit.torrent"; Tasks: links

[UninstallDelete]
Type: files; Name: "{app}\bawkbit.old.exe"
Type: files; Name: "{app}\bawkbit.new.exe"
Type: files; Name: "{app}\bawkbitd.old.exe"
Type: files; Name: "{app}\bawkbitd.new.exe"

[Run]
Filename: "{app}\bawkbit.exe"; Description: "Start bawkbit"; Flags: nowait postinstall skipifsilent

[Code]
// Another client may have taken magnet links and .torrent files since, so they go only when they still open this copy.
procedure CurUninstallStepChanged(Step: TUninstallStep);
var
  Value: String;
begin
  if Step <> usUninstall then
    exit;
  if RegQueryStringValue(HKCU, '{#Classes}\magnet\shell\open\command', '', Value)
    and (Pos(Lowercase(ExpandConstant('{app}\bawkbit.exe')), Lowercase(Value)) > 0) then
    RegDeleteKeyIncludingSubkeys(HKCU, '{#Classes}\magnet');
  if RegQueryStringValue(HKCU, '{#Classes}\.torrent', '', Value) and (Value = 'bawkbit.torrent') then
    RegDeleteValue(HKCU, '{#Classes}\.torrent', '');
  if RegQueryStringValue(HKCU, '{#Run}', 'bawkbit', Value)
    and (Pos(Lowercase(ExpandConstant('{app}\bawkbit.exe')), Lowercase(Value)) > 0) then
    RegDeleteValue(HKCU, '{#Run}', 'bawkbit');
end;
