#ifndef AppVersion
#define AppVersion "0.1.3"
#endif

#ifndef AppArch
#define AppArch "x64"
#endif

#ifndef SourceDir
#define SourceDir "."
#endif

#ifndef OutputDir
#define OutputDir "."
#endif

#ifndef IconFile
#define IconFile ".\icon.ico"
#endif

#ifndef LicenseFile
#define LicenseFile ".\LICENSE"
#endif

[Setup]
AppId={{B7B7C5DF-EE40-4D67-8D99-9DD904A59D2C}
AppName=GDS3D
AppVersion={#AppVersion}
AppPublisher=GDS3D
DefaultDirName={autopf}\GDS3D
DefaultGroupName=GDS3D
DisableProgramGroupPage=yes
LicenseFile={#LicenseFile}
OutputDir={#OutputDir}
OutputBaseFilename=GDS3D-{#AppVersion}-windows-{#AppArch}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\icon.ico

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\gds3d.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\icon.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\GDS3D"; Filename: "{app}\gds3d.exe"; IconFilename: "{app}\icon.ico"
Name: "{autodesktop}\GDS3D"; Filename: "{app}\gds3d.exe"; IconFilename: "{app}\icon.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\gds3d.exe"; Description: "{cm:LaunchProgram,GDS3D}"; Flags: nowait postinstall skipifsilent
