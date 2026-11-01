[Setup]
AppName=Caskade Fixture Game
AppVersion=1.0
AppId=CaskadeFixture
DefaultDirName={pf}\Caskade Fixture
#ifdef REGISTRY
PrivilegesRequired=admin
#else
PrivilegesRequired=lowest
#endif
#ifdef UNINSTALL
Uninstallable=yes
#else
Uninstallable=no
#endif
DisableProgramGroupPage=yes
Compression=lzma
SolidCompression=yes
OutputBaseFilename=setup
OutputDir=out
#ifdef SLICED
DiskSpanning=yes
DiskSliceSize=2000000
#endif
#ifdef PASSWORD
Password=caskade-fixture
Encryption=yes
#endif
[Files]
Source: "payload\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
#ifdef REGISTRY
[Registry]
Root: HKLM64; Subkey: "Software\CaskadeFixture\FixtureGame"; ValueType: string; ValueName: "InstallPath"; ValueData: "{app}"; Flags: uninsdeletekey
#endif
#ifdef INI
[INI]
Filename: "{app}\config.ini"; Section: "Game"; Key: "InstallPath"; String: "{app}"
#endif
#ifdef CODE
[Code]
#ifdef GOG
function NeverRunRedist: Boolean;
begin
  Result := False;
end;
#endif
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    SaveStringToFile(ExpandConstant('{app}\code-path.txt'), ExpandConstant('{app}'), False);
end;
#endif
#ifdef GOG
[Run]
Filename: "{app}\__redist\vcredist_x64.exe"; Parameters: "/quiet /norestart"; Flags: skipifdoesntexist; Check: NeverRunRedist
#endif
