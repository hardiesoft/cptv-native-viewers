; Inno Setup script for the Windows x64 installer. Built by
; scripts/package-windows.sh installer (or `all`), which passes:
;   /DAppVersion=<version> /DBuildDir=<dir with the built DLLs> /DOutDir=<dist>
;
; Installs (per machine, needs admin):
;   {app}\vlc-plugins\libcptv_plugin.dll   VLC 3 plugin, found through a machine-wide
;                                          VLC_PLUGIN_PATH, so VLC's own folder is
;                                          untouched and VLC upgrades don't remove it
;   {app}\cptv_thumbnail_handler.dll       Explorer thumbnail handler (COM)
; and a .cptv file type that opens in the installed VLC.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef BuildDir
  #define BuildDir "..\..\build\windows"
#endif
#ifndef OutDir
  #define OutDir "..\..\dist"
#endif

#define ProgId "CPTVNativeViewers.cptv"
; GUIDs below start with "{{": Inno Setup reads "{" as the start of one of its
; constants, and "{{" is a literal brace.
; Must match CLSID_CPTV_THUMBNAIL in windows/thumbnail-handler/src/lib.rs.
#define ThumbClsid "{{FBF13CFA-0D80-4899-926E-C50711CDFA5A}"
; Shell's IThumbnailProvider handler slot.
#define ThumbSlot "{{e357fccd-a995-4576-b01f-234630154e96}"

[Setup]
AppId={{1EC42460-B200-4815-BA11-8453E7ABDA1F}
AppName=CPTV Native Viewers
AppVersion={#AppVersion}
AppVerName=CPTV Native Viewers {#AppVersion}
AppPublisher=The Cacophony Project
AppPublisherURL=https://github.com/hardiesoft/cptv-native-viewers
DefaultDirName={autopf}\CPTV Native Viewers
DisableDirPage=auto
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
PrivilegesRequired=admin
ChangesEnvironment=yes
ChangesAssociations=yes
OutputDir={#OutDir}
OutputBaseFilename=cptv-native-viewers-{#AppVersion}-windows-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName=CPTV Native Viewers

[Files]
Source: "{#BuildDir}\libcptv_plugin.dll"; DestDir: "{app}\vlc-plugins"; Flags: ignoreversion
; Explorer's thumbnail host may have the handler loaded: replace/remove it on
; restart if it's in use.
Source: "{#BuildDir}\cptv_thumbnail_handler.dll"; DestDir: "{app}"; Flags: ignoreversion restartreplace uninsrestartdelete

[Registry]
; --- .cptv file type, opened by VLC ---------------------------------------------
Root: HKLM; Subkey: "Software\Classes\.cptv"; ValueType: string; ValueName: ""; ValueData: "{#ProgId}"; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKLM; Subkey: "Software\Classes\.cptv"; ValueType: string; ValueName: "Content Type"; ValueData: "video/x-cptv"; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKLM; Subkey: "Software\Classes\.cptv\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKLM; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: ""; ValueData: "Cacophony thermal video"; Flags: uninsdeletekey
; No app-icon badge on thumbnails.
Root: HKLM; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: "TypeOverlay"; ValueData: ""
Root: HKLM; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{code:VlcExe}"",0"
Root: HKLM; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{code:VlcExe}"" --started-from-file ""%1"""

; --- Thumbnail handler ------------------------------------------------------------
Root: HKLM; Subkey: "Software\Classes\CLSID\{#ThumbClsid}"; ValueType: string; ValueName: ""; ValueData: "CPTV Thumbnail Handler"; Flags: uninsdeletekey
Root: HKLM; Subkey: "Software\Classes\CLSID\{#ThumbClsid}\InprocServer32"; ValueType: string; ValueName: ""; ValueData: "{app}\cptv_thumbnail_handler.dll"
Root: HKLM; Subkey: "Software\Classes\CLSID\{#ThumbClsid}\InprocServer32"; ValueType: string; ValueName: "ThreadingModel"; ValueData: "Apartment"
; Uninstall runs in reverse order: the slot goes first, then this empty
; ShellEx key, then .cptv itself (above) once it's empty.
Root: HKLM; Subkey: "Software\Classes\.cptv\ShellEx"; Flags: uninsdeletekeyifempty
Root: HKLM; Subkey: "Software\Classes\.cptv\ShellEx\{#ThumbSlot}"; ValueType: string; ValueName: ""; ValueData: "{#ThumbClsid}"; Flags: uninsdeletekey
Root: HKLM; Subkey: "Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved"; ValueType: string; ValueName: "{#ThumbClsid}"; ValueData: "CPTV Thumbnail Handler"; Flags: uninsdeletevalue

[Code]
const
  EnvKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';
  EnvName = 'VLC_PLUGIN_PATH';

function VlcExe(Param: String): String;
var
  Dir: String;
begin
  { VLC's installer records vlc.exe's path as the default value of this key. }
  if RegQueryStringValue(HKLM64, 'Software\VideoLAN\VLC', '', Result) and FileExists(Result) then
    Exit;
  if RegQueryStringValue(HKLM64, 'Software\VideoLAN\VLC', 'InstallDir', Dir) and
     FileExists(AddBackslash(Dir) + 'vlc.exe') then
  begin
    Result := AddBackslash(Dir) + 'vlc.exe';
    Exit;
  end;
  Result := ExpandConstant('{autopf}\VideoLAN\VLC\vlc.exe');
end;

function InitializeSetup(): Boolean;
var
  Version: String;
begin
  Result := True;
  if not FileExists(VlcExe('')) then
    SuppressibleMsgBox('VLC media player (64-bit) wasn''t found. Install VLC 3 from ' +
      'videolan.org to play .cptv files; Explorer thumbnails will work without it.',
      mbInformation, MB_OK, IDOK)
  else if RegQueryStringValue(HKLM64, 'Software\VideoLAN\VLC', 'Version', Version) and
          (Copy(Version, 1, 2) <> '3.') then
    SuppressibleMsgBox('This plugin is for VLC 3, but VLC ' + Version + ' is installed, ' +
      'which may not load it. Explorer thumbnails will still work.',
      mbInformation, MB_OK, IDOK);
end;

{ VLC_PLUGIN_PATH is a ';'-separated list: add our folder to any existing
  value, and on uninstall remove only our entry. }
function PluginDir(): String;
begin
  Result := ExpandConstant('{app}\vlc-plugins');
end;

function RemoveEntry(List, Entry: String): String;
var
  Item: String;
  P: Integer;
begin
  Result := '';
  List := List + ';';
  while List <> '' do
  begin
    P := Pos(';', List);
    Item := Trim(Copy(List, 1, P - 1));
    Delete(List, 1, P);
    if (Item <> '') and (CompareText(RemoveBackslashUnlessRoot(Item), Entry) <> 0) then
    begin
      if Result <> '' then Result := Result + ';';
      Result := Result + Item;
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Value: String;
begin
  if CurStep = ssPostInstall then
  begin
    if not RegQueryStringValue(HKLM, EnvKey, EnvName, Value) then
      Value := '';
    Value := RemoveEntry(Value, PluginDir());
    if Value <> '' then Value := Value + ';';
    RegWriteExpandStringValue(HKLM, EnvKey, EnvName, Value + PluginDir());
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Value: String;
begin
  if (CurUninstallStep = usUninstall) and RegQueryStringValue(HKLM, EnvKey, EnvName, Value) then
  begin
    Value := RemoveEntry(Value, PluginDir());
    if Value = '' then
      RegDeleteValue(HKLM, EnvKey, EnvName)
    else
      RegWriteExpandStringValue(HKLM, EnvKey, EnvName, Value);
  end;
end;
