; Older releases could be installed per user (%LOCALAPPDATA%). The installer is now per machine
; (Program Files, asks for admin), so remove such a copy first instead of leaving a second one.
!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}" "InstallLocation"
  ${If} $0 != ""
    ; stored quoted: "C:\...\OpenPano"
    StrCpy $1 $0 1
    ${If} $1 == '"'
      StrCpy $0 $0 -1 1
    ${EndIf}
    ${If} $0 != $INSTDIR
    ${AndIf} ${FileExists} "$0\uninstall.exe"
      DetailPrint "Removing per-user install: $0"
      ExecWait '"$0\uninstall.exe" /S _?=$0'
      Delete "$0\uninstall.exe"
      RMDir "$0"
    ${EndIf}
  ${EndIf}
!macroend
