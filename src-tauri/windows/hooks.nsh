; Remove the browser connection that the app registers for Chrome, Edge, Brave and Chromium.
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
  DeleteRegKey HKCU "Software\Chromium\NativeMessagingHosts\np.com.harshchaudhary.swiftharrydm"
!macroend
