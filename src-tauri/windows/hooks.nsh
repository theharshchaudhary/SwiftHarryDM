; Start with Windows: the installer adds the startup entry (the app itself never
; adds it on its own; it only changes it when the option is switched in Options). An update
; keeps the user's choice, which the app remembers in Software\Keepfetch.
;
; The app was called SwiftHarryDM before. Installing Keepfetch over it keeps the user's
; startup choice and removes the old program (its uninstaller deletes only the files it
; installed, so the download list and settings stay in %LOCALAPPDATA%\SwiftHarryDM, where
; Keepfetch copies them from on its first start).
!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $0 HKCU "Software\Keepfetch" "StartWithWindows"
  ${If} $0 == ""
    ReadRegStr $0 HKCU "Software\SwiftHarryDM" "StartWithWindows"
  ${EndIf}
  ${If} $0 != "0"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Keepfetch" '"$INSTDIR\${MAINBINARYNAME}.exe" --minimized'
  ${EndIf}
  WriteRegStr HKCU "Software\Keepfetch" "StartWithWindows" "$0"

  ReadRegStr $1 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SwiftHarryDM" "UninstallString"
  ${If} $1 != ""
    nsExec::Exec 'taskkill /F /T /IM swiftharrydm.exe'
    Pop $2
    ; _? makes the uninstaller run in place, so ExecWait really waits for it.
    ExecWait '$1 /S _?=$LOCALAPPDATA\SwiftHarryDM'
    Delete "$LOCALAPPDATA\SwiftHarryDM\uninstall.exe"
  ${EndIf}
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SwiftHarryDM"
!macroend

; Remove what the app registers for the current user: the browser connection for Chrome,
; Edge, Brave and Chromium, and the "start with Windows" entry (under both names).
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\Chromium\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Keepfetch"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Keepfetch"
  DeleteRegKey HKCU "Software\Keepfetch"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SwiftHarryDM"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "SwiftHarryDM"
!macroend
