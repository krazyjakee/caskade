; Tauri's bundler includes these in the NSIS installer (Tauri.windows.toml).
; Its own check covers caskade.exe, the native app. Installs from 0.1.x ran
; the native app as caskade-native.exe beside a Tauri caskade.exe: close it
; before an update or uninstall, then remove the executable this layout no
; longer has, so it can't keep running the old UI or keep the folder.
!macro NSIS_HOOK_PREINSTALL
  !insertmacro CheckIfAppIsRunning "$INSTDIR\caskade-native.exe" "${PRODUCTNAME}"
  Delete "$INSTDIR\caskade-native.exe"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro CheckIfAppIsRunning "$INSTDIR\caskade-native.exe" "${PRODUCTNAME}"
  Delete "$INSTDIR\caskade-native.exe"
!macroend
