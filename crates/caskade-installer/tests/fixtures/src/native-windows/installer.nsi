!ifdef NSIS3
Unicode true
!endif
!include "FileFunc.nsh"
Name "Caskade T7 ${FixtureId}"
OutFile "out\setup.exe"
InstallDir "$LOCALAPPDATA\CaskadeT7\${FixtureId}"
RequestExecutionLevel user
SetCompressor /SOLID lzma
Page directory
Page instfiles
UninstPage instfiles
Var Parameters
Var NoShortcuts
Section
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /r "payload\*"
  WriteRegStr HKCU "Software\CaskadeT7\${FixtureId}" "InstallPath" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${FixtureId}" "DisplayName" "Caskade T7 ${FixtureId}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${FixtureId}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${FixtureId}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  ${GetParameters} $Parameters
  ${GetOptions} $Parameters "/CASKADE_NO_SHORTCUTS=" $NoShortcuts
  StrCmp $NoShortcuts "1" no_icons
  CreateShortCut "$SMPROGRAMS\Caskade T7 ${FixtureId}.lnk" "$INSTDIR\FixtureGame.exe"
no_icons:
  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd
Section "Uninstall"
  SetShellVarContext current
  DeleteRegKey HKCU "Software\CaskadeT7\${FixtureId}"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${FixtureId}"
  Delete "$SMPROGRAMS\Caskade T7 ${FixtureId}.lnk"
  RMDir /r "$INSTDIR"
SectionEnd
