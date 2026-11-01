[Setup]
AppName=Caskade T7 {#FixtureId}
AppVersion=1.0
AppId={#FixtureId}
DefaultDirName={localappdata}\CaskadeT7\{#FixtureId}
PrivilegesRequired=lowest
Uninstallable=yes
UninstallDisplayName=Caskade T7 {#FixtureId}
DisableProgramGroupPage=yes
AllowNoIcons=yes
Compression=lzma
SolidCompression=yes
OutputBaseFilename=setup
OutputDir=out
[Files]
Source: "payload\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{userprograms}\Caskade T7 {#FixtureId}"; Filename: "{app}\FixtureGame.exe"; Check: not WizardNoIcons
[Registry]
Root: HKCU; Subkey: "Software\CaskadeT7\{#FixtureId}"; ValueType: string; ValueName: "InstallPath"; ValueData: "{app}"; Flags: uninsdeletekey
