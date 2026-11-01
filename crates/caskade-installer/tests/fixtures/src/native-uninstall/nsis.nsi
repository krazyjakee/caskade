Unicode true
Name "Caskade Native NSIS Fixture"
OutFile "out\setup.exe"
InstallDir "$LOCALAPPDATA\CaskadeNativeNSISFixture"
RequestExecutionLevel user
SetCompressor /SOLID lzma
Section
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /r "payload\*"
  WriteINIStr "$INSTDIR\config.ini" "Game" "InstallPath" "$INSTDIR"
  WriteRegStr HKCU "Software\CaskadeNativeFixture\nsis" "InstallPath" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CaskadeNativeNSISFixture" "DisplayName" "Caskade Native NSIS Fixture"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CaskadeNativeNSISFixture" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CaskadeNativeNSISFixture" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  CreateShortCut "$DESKTOP\Caskade Native NSIS Fixture.lnk" "$INSTDIR\FixtureGame.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd
Section "Uninstall"
  SetShellVarContext current
  DeleteRegKey HKCU "Software\CaskadeNativeFixture\nsis"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CaskadeNativeNSISFixture"
  Delete "$DESKTOP\Caskade Native NSIS Fixture.lnk"
  RMDir /r "$INSTDIR\FixtureGame_Data"
  Delete "$INSTDIR\FixtureGame.exe"
  Delete "$INSTDIR\UnityCrashHandler64.exe"
  Delete "$INSTDIR\UnityPlayer.dll"
  Delete "$INSTDIR\config.ini"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
