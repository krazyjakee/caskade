; The main caskade.exe hands normal starts to caskade-native.exe and exits.
; Tauri checks the main executable itself; check the native UI as well before
; replacing or removing its executable (including passive updater installs).
!macro NSIS_HOOK_PREINSTALL
  !insertmacro CheckIfAppIsRunning "caskade-native.exe" "Caskade"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro CheckIfAppIsRunning "caskade-native.exe" "Caskade"
!macroend
