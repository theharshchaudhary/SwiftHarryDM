; Remove what the app registers for the current user: the browser connection for Chrome,
; Edge, Brave and Chromium, and the "start with Windows" entry.
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\Chromium\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SwiftHarryDM"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "SwiftHarryDM"
!macroend
