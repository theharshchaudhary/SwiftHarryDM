; Inno Setup 6 script. Built by: python packaging/build.py --installer
; Code signing is done by build.py with a certificate from the Windows certificate
; store (SIGNTOOL_CERT_THUMBPRINT); no signing material belongs in this repository.
#ifndef AppVersion
  #define AppVersion "2.0.0"
#endif

[Setup]
AppId={{7C2B8E1A-5D4F-4C1E-9A7B-2F6E3D8C1A90}
AppName=SwiftHarryDM
AppVersion={#AppVersion}
AppPublisher=Harsh Chaudhary
DefaultDirName={autopf}\SwiftHarryDM
DefaultGroupName=SwiftHarryDM
DisableProgramGroupPage=yes
OutputDir=dist
OutputBaseFilename=SwiftHarryDM-{#AppVersion}-setup
SetupIconFile=..\app.ico
UninstallDisplayIcon={app}\SwiftHarryDM.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequiredOverridesAllowed=dialog
LicenseFile=..\LICENSE.txt

[Files]
Source: "dist\SwiftHarryDM\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\SwiftHarryDM"; Filename: "{app}\SwiftHarryDM.exe"
Name: "{group}\Browser extension folder"; Filename: "{app}\_internal\extension"
Name: "{autodesktop}\SwiftHarryDM"; Filename: "{app}\SwiftHarryDM.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Run]
Filename: "{app}\SwiftHarryDM.exe"; Description: "Launch SwiftHarryDM"; Flags: nowait postinstall skipifsilent

; Downloads, settings and the job database (%LOCALAPPDATA%\SwiftHarryDM) are user data
; and are intentionally left in place on uninstall.
