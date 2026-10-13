#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif

[Setup]
AppId={{8C1B6E2A-5F3D-4C7A-9E21-7A2F0B6D9E44}
AppName=Panoramer
AppVersion={#AppVersion}
AppPublisher=Panoramer contributors
AppPublisherURL=https://github.com/BlackSingularities/Panoramer
AppSupportURL=https://github.com/BlackSingularities/Panoramer/issues
DefaultDirName={autopf}\Panoramer
DefaultGroupName=Panoramer
UninstallDisplayIcon={app}\panoramer.exe
SetupIconFile=..\assets\icon.ico
LicenseFile=..\LICENSE
OutputDir=..\dist
OutputBaseFilename=Panoramer-{#AppVersion}-setup-x64
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequiredOverridesAllowed=dialog
ChangesAssociations=yes

[Languages]
Name: "polish"; MessagesFile: "compiler:Languages\Polish.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\panoramer.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
Name: "{group}\Panoramer"; Filename: "{app}\panoramer.exe"
Name: "{group}\{cm:UninstallProgram,Panoramer}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Panoramer"; Filename: "{app}\panoramer.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\panoramer.exe"; Description: "{cm:LaunchProgram,Panoramer}"; Flags: nowait postinstall skipifsilent
