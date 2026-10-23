; HomeUp — instalador Windows (Inno Setup 6)
; Gerar:  scripts\build-installer.ps1   (ou: ISCC installer\homeup.iss)

#define AppName "HomeUp"
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#define AppExe "homeup.exe"

[Setup]
AppId={{6F1E2B8C-4D3A-4C52-9E1B-7A0D5C3F9B21}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Pedro Cordeiro Lima
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; instala só para o usuário (sem administrador) ou, se escolher, para todos
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=HomeUp-{#AppVersion}-setup
SetupIconFile=..\assets\homeup.ico
UninstallDisplayIcon={app}\{#AppExe}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes

[Languages]
Name: "pt"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "assoc_homeup"; Description: "Abrir arquivos .homeup com o HomeUp"; GroupDescription: "Associações de arquivo:"
Name: "assoc_skp"; Description: "Abrir arquivos .skp (SketchUp) com o HomeUp"; GroupDescription: "Associações de arquivo:"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\HomeUp.Model"; ValueType: string; ValueData: "Modelo do HomeUp"; Flags: uninsdeletekey; Tasks: assoc_homeup assoc_skp
Root: HKA; Subkey: "Software\Classes\HomeUp.Model\DefaultIcon"; ValueType: string; ValueData: """{app}\{#AppExe}"",0"; Tasks: assoc_homeup assoc_skp
Root: HKA; Subkey: "Software\Classes\HomeUp.Model\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: assoc_homeup assoc_skp
Root: HKA; Subkey: "Software\Classes\.homeup\OpenWithProgids"; ValueType: string; ValueName: "HomeUp.Model"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc_homeup
Root: HKA; Subkey: "Software\Classes\.skp\OpenWithProgids"; ValueType: string; ValueName: "HomeUp.Model"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc_skp
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".homeup"; ValueData: ""; Flags: uninsdeletekey

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
