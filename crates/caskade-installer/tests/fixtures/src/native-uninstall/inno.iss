[Setup]
AppName=Caskade Native Inno Fixture
AppVersion=1.0
AppId=CaskadeNativeInnoFixture
DefaultDirName={localappdata}\CaskadeNativeInnoFixture
PrivilegesRequired=lowest
Uninstallable=yes
DisableProgramGroupPage=yes
Compression=lzma
SolidCompression=yes
OutputBaseFilename=setup
OutputDir=out
[Files]
Source: "payload\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{userdesktop}\Caskade Native Inno Fixture"; Filename: "{app}\FixtureGame.exe"
Name: "{userprograms}\Caskade Native Inno Fixture"; Filename: "{app}\FixtureGame.exe"
[Registry]
Root: HKCU; Subkey: "Software\CaskadeNativeFixture\inno"; ValueType: string; ValueName: "InstallPath"; ValueData: "{app}"; Flags: uninsdeletekey
[INI]
Filename: "{app}\config.ini"; Section: "Game"; Key: "InstallPath"; String: "{app}"
