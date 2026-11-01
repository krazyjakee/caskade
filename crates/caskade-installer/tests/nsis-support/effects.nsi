Unicode true
Name "Caskade NSIS Effects Fixture"
OutFile "effects.exe"
InstallDir "$LOCALAPPDATA\Caskade NSIS Effects Fixture"
RequestExecutionLevel user
SetCompressor lzma
Section
  SetOutPath "$INSTDIR\空白 Game"
  File /oname=game.txt "effects.nsi"
  WriteRegStr HKCU "Software\CaskadeEffectFixture" "InstallDir" "$INSTDIR"
  WriteRegDWORD HKCU "Software\CaskadeEffectFixture" "Enabled" 1
  DeleteRegValue HKCU "Software\CaskadeEffectFixture" "OldValue"
  ExecWait '"$INSTDIR\game.exe" --token "空白 test"'
  ExecShell "open" "$INSTDIR\manual.txt"
  CreateShortcut "$SMPROGRAMS\Caskade Effect Fixture.lnk" "$INSTDIR\空白 Game\game.exe" "--safe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd
Section "Uninstall"
  DeleteRegKey HKCU "Software\CaskadeEffectFixture"
  Delete "$INSTDIR\空白 Game\game.txt"
SectionEnd
