Unicode True
!include "MUI2.nsh"
!include "LogicLib.nsh"
!ifndef PICKLE_VERSION
  !define PICKLE_VERSION "0.1.0"
!endif
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\PickleRune"
!ifdef PICKLE_SIGN_CERT_THUMBPRINT
  !ifndef PICKLE_SIGNTOOL
    !error "PICKLE_SIGNTOOL must identify the resolved Windows SDK signtool.exe"
  !endif
  !uninstfinalize '"${PICKLE_SIGNTOOL}" sign /sha1 ${PICKLE_SIGN_CERT_THUMBPRINT} /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 "%1"'
!endif
Name "Pickle Rune"
Icon "..\assets\Pickle.ico"
UninstallIcon "..\assets\Pickle.ico"
OutFile "${PICKLE_OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\Pickle Rune"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetOverwrite try
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

!macro Fail Message Code
  SetErrorLevel ${Code}
  ${IfNot} ${Silent}
    MessageBox MB_OK|MB_ICONSTOP "${Message}"
  ${EndIf}
  Abort
!macroend

; This probe only inspects a lease; it never forwards Reopen or initializes UI.
Function .onInit
  SetShellVarContext current
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=PickleProbe.exe "${PICKLE_STAGE}\PickleRune.exe"
  Call EnsureNotRunning
FunctionEnd

Function EnsureNotRunning
retry_running_check:
  StrCpy $0 1
  ClearErrors
  ExecWait '"$PLUGINSDIR\PickleProbe.exe" --check-running' $0
  ${If} ${Errors}
    !insertmacro Fail "Could not check whether Pickle is running. Verify Windows runtime prerequisites and retry. Existing user data was not removed." 1
  ${EndIf}
  ${If} $0 == 0
    Return
  ${ElseIf} $0 == 10
    ${If} ${Silent}
      SetErrorLevel 10
      Abort
    ${EndIf}
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Quit Pickle from its system tray menu before installing or upgrading. Click Retry after it has closed." IDRETRY retry_running_check
    SetErrorLevel 10
    Abort
  ${Else}
    !insertmacro Fail "Pickle's running status could not be checked safely. Quit Pickle and retry; repair the existing installation if the problem persists." 1
  ${EndIf}
FunctionEnd

Function un.EnsureNotRunning
retry_running_check:
  IfFileExists "$INSTDIR\PickleRune.exe" 0 missing_application
  StrCpy $0 1
  ClearErrors
  ExecWait '"$INSTDIR\PickleRune.exe" --check-running' $0
  ${If} ${Errors}
    !insertmacro Fail "Could not check the installed application. Reinstall into this same directory to repair it before uninstalling." 1
  ${EndIf}
  ${If} $0 == 0
    Return
  ${ElseIf} $0 == 10
    ${If} ${Silent}
      SetErrorLevel 10
      Abort
    ${EndIf}
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Quit Pickle from its system tray menu before uninstalling. Click Retry after it has closed." IDRETRY retry_running_check
    SetErrorLevel 10
    Abort
  ${Else}
    !insertmacro Fail "Pickle's running status could not be checked safely. No user history or credentials were removed." 1
  ${EndIf}
missing_application:
  !insertmacro Fail "The installed Pickle executable is missing. Reinstall into this same directory so its browser hosts can be disconnected safely, then uninstall again." 1
FunctionEnd

Function un.onInit
  SetShellVarContext current
  Call un.EnsureNotRunning
FunctionEnd

!macro DeleteOwnedFile RelativePath
  ${If} ${FileExists} "$INSTDIR\${RelativePath}"
    ClearErrors
    Delete "$INSTDIR\${RelativePath}"
    ${If} ${Errors}
      !insertmacro Fail "A Pickle application file could not be removed. Close browser context requests and retry. User history and credentials were retained." 1
    ${EndIf}
  ${EndIf}
!macroend

Section "Pickle Rune"
  SetShellVarContext current
  Call EnsureNotRunning
  ClearErrors
  SetOutPath "$INSTDIR"
  File /r "${PICKLE_STAGE}\*"
  ${If} ${Errors}
    !insertmacro Fail "Application files could not all be copied. Check storage and close running copies, then rerun this installer to repair the installation. User data was not removed." 1
  ${EndIf}
  ClearErrors
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\Pickle Rune"
  CreateShortcut "$SMPROGRAMS\Pickle Rune\Pickle Rune.lnk" "$INSTDIR\PickleRune.exe" "" "$INSTDIR\Pickle.ico"
  ${If} ${Errors}
    !insertmacro Fail "The uninstall entry or shortcut could not be created. Rerun this installer to repair the installation." 1
  ${EndIf}
  ClearErrors
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Pickle Rune"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\Pickle.ico"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${PICKLE_VERSION}"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${If} ${Errors}
    !insertmacro Fail "The per-user installed-app entry could not be saved. Rerun this installer to repair the installation." 1
  ${EndIf}
  SetErrorLevel 0
SectionEnd

Section "Uninstall"
  SetShellVarContext current
retry_browser_cleanup:
  Call un.EnsureNotRunning
  StrCpy $1 1
  ClearErrors
  ExecWait '"$INSTDIR\PickleRune.exe" --disconnect-browsers' $1
  ${If} ${Errors}
    !insertmacro Fail "Could not run browser cleanup. The application was retained so it can be repaired and cleanup retried." 1
  ${EndIf}
  ${If} $1 != 0
    ${If} ${Silent}
      SetErrorLevel $1
      Abort
    ${EndIf}
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Pickle could not safely disconnect its browser hosts. Close browser context requests and check the connected installation, then Retry. Cancel retains the application and saved data." IDRETRY retry_browser_cleanup
    SetErrorLevel $1
    Abort
  ${EndIf}
  Call un.EnsureNotRunning
  ; Only known installed files. Unknown files, user history and credentials are retained.
  !insertmacro DeleteOwnedFile "PickleRune.exe"
  !insertmacro DeleteOwnedFile "pickle-browser-host.exe"
  !insertmacro DeleteOwnedFile "Pickle.ico"
  !insertmacro DeleteOwnedFile "THIRD-PARTY-NOTICES.md"
  !insertmacro DeleteOwnedFile "THIRD_PARTY_NOTICES.txt"
  !insertmacro DeleteOwnedFile "dependency-inventory.json"
  !insertmacro DeleteOwnedFile "browser-extension\manifest.json"
  !insertmacro DeleteOwnedFile "browser-extension\extract.js"
  !insertmacro DeleteOwnedFile "browser-extension\background.js"
  !insertmacro DeleteOwnedFile "browser-extension\popup.js"
  !insertmacro DeleteOwnedFile "browser-extension\popup.html"
  !insertmacro DeleteOwnedFile "browser-extension\Readability.js"
  !insertmacro DeleteOwnedFile "browser-extension\Readability-LICENSE.md"
  !insertmacro DeleteOwnedFile "browser-extension\README.md"
  RMDir "$INSTDIR\browser-extension"
  ; An older installation must not remove a newer installation's registry entry or shortcut.
  ReadRegStr $2 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $2 == $INSTDIR
    Delete "$SMPROGRAMS\Pickle Rune\Pickle Rune.lnk"
    RMDir "$SMPROGRAMS\Pickle Rune"
    DeleteRegKey HKCU "${UNINSTALL_KEY}"
  ${EndIf}
  ; Normal uninstall runs from a temporary copy. With NSIS _?=, the caller must
  ; delete this in-use executable after a successful exit; do not report a false failure.
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  SetErrorLevel 0
SectionEnd
