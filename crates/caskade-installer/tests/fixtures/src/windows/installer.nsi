!ifdef NSIS3
Unicode ${UNICODE}
!endif
Name "Caskade Fixture Game"
OutFile "out\setup.exe"
InstallDir "$PROGRAMFILES\Caskade Fixture"
RequestExecutionLevel user
SetCompressor ${COMPRESSOR}
Section
  SetOutPath "$INSTDIR"
  File /r "payload\*"
  WriteINIStr "$INSTDIR\config.ini" "Game" "InstallPath" "$INSTDIR"
  CreateDirectory "$SMPROGRAMS\Caskade Fixture"
  CreateShortcut "$SMPROGRAMS\Caskade Fixture\Fixture Game.lnk" "$INSTDIR\FixtureGame.exe"
SectionEnd
