; インストール先の HKCU\Software\VRCLibMan を消す。ライブラリと WebView のフォルダは残す。
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\VRCLibMan"
!macroend
