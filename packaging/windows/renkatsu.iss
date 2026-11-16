; Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
; SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0
;
; The Windows installer, built by the release workflow with Inno Setup:
;   iscc /DVersion=0.1.1 packaging\windows\renkatsu.iss

#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId={{6F2E7C1A-4B8D-4E2F-9A57-3C1D8B0E5A21}
AppName=Renkatsu Studio
AppVersion={#Version}
AppVerName=Renkatsu Studio {#Version}
AppPublisher=Mihnea Gîrbăcică (Mihnea8848)
AppPublisherURL=https://github.com/Mihnea8848/renkatsu
AppSupportURL=https://github.com/Mihnea8848/renkatsu/issues
DefaultDirName={autopf}\Renkatsu
DefaultGroupName=Renkatsu
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\renkatsu.exe
LicenseFile=..\..\LICENSE
SetupIconFile=renkatsu.ico
OutputDir=..\..\dist
OutputBaseFilename=Renkatsu-{#Version}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequiredOverridesAllowed=dialog

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\..\target\release\renkatsu.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\rk.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "renkatsu.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\..\LICENSES\*"; DestDir: "{app}\LICENSES"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Renkatsu Studio"; Filename: "{app}\renkatsu.exe"; IconFilename: "{app}\renkatsu.ico"
Name: "{autodesktop}\Renkatsu Studio"; Filename: "{app}\renkatsu.exe"; IconFilename: "{app}\renkatsu.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\renkatsu.exe"; Description: "{cm:LaunchProgram,Renkatsu Studio}"; Flags: nowait postinstall skipifsilent
