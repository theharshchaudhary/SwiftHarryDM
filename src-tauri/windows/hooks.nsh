; Start with Windows, like IDM: the installer adds the startup entry (the app itself never
; adds it on its own; it only changes it when the option is switched in Options). An update
; keeps the user's choice, which the app remembers in Software\SwiftHarryDM.
!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $0 HKCU "Software\SwiftHarryDM" "StartWithWindows"
  ${If} $0 != "0"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SwiftHarryDM" '"$INSTDIR\${MAINBINARYNAME}.exe" --minimized'
  ${EndIf}
!macroend

; Remove what the app registers for the current user: the browser connection for Chrome,
; Edge, Brave and Chromium, and the "start with Windows" entry.
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\Chromium\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SwiftHarryDM"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "SwiftHarryDM"
  DeleteRegKey HKCU "Software\SwiftHarryDM"
!macroend
