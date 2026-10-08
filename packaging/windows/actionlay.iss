#ifndef AppVersion
  #define AppVersion "1.3.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist\windows\ActionLay"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif
[Setup]
AppId={{B7B4CFBE-D778-4FB8-A976-84E90F770ACF}
AppName=ActionLay
AppVersion={#AppVersion}
AppPublisher=Alessandro Rinaldi
AppPublisherURL=https://github.com/porech/actionlay
AppSupportURL=https://github.com/porech/actionlay/issues
DefaultDirName={autopf}\ActionLay
DefaultGroupName=ActionLay
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog commandline
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
ChangesAssociations=yes
UninstallDisplayIcon={app}\actionlay.exe
SetupIconFile=..\..\assets\icons\actionlay.ico
OutputDir={#OutputDir}
OutputBaseFilename=actionlay-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
WizardImageFile=..\..\assets\installer\wizard.bmp
WizardSmallImageFile=..\..\assets\installer\wizard-small.bmp
WizardImageStretch=yes
WizardImageBackColor=$2D250C
CloseApplications=yes
UsePreviousAppDir=yes
UsePreviousPrivileges=yes
[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked
[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{autoprograms}\ActionLay"; Filename: "{app}\actionlay.exe"
Name: "{autodesktop}\ActionLay"; Filename: "{app}\actionlay.exe"; Tasks: desktopicon
[Registry]
Root: HKA; Subkey: "Software\Classes\ActionLay.InstalledVideo"; ValueType: string; ValueData: "ActionLay video"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\ActionLay.InstalledVideo\Application"; ValueType: string; ValueName: "ApplicationName"; ValueData: "ActionLay"
Root: HKA; Subkey: "Software\Classes\ActionLay.InstalledVideo\DefaultIcon"; ValueType: string; ValueData: """{app}\actionlay.exe"",0"
Root: HKA; Subkey: "Software\Classes\ActionLay.InstalledVideo\shell\open\command"; ValueType: string; ValueData: """{app}\actionlay.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\.mp4\OpenWithProgids"; ValueType: string; ValueName: "ActionLay.InstalledVideo"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.mov\OpenWithProgids"; ValueType: string; ValueName: "ActionLay.InstalledVideo"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.lrv\OpenWithProgids"; ValueType: string; ValueName: "ActionLay.InstalledVideo"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.insv\OpenWithProgids"; ValueType: string; ValueName: "ActionLay.InstalledVideo"; ValueData: ""; Flags: uninsdeletevalue
[Run]
Filename: "{app}\actionlay.exe"; Description: "Launch ActionLay"; Flags: nowait postinstall skipifsilent
