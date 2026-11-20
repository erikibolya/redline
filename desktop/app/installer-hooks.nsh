; The RedLine Desktop installer also installs the separate RedLine Agent tray app, so a PC that
; runs reviews gets two trays: RedLine Desktop (the review client) and RedLine Agent (the bridge).
; The agent is its own product with its own config, updater and uninstaller; uninstalling the
; desktop app leaves it in place, because reviews must keep running without the review client.

; Tauri v2 includes this file before the installer defines and inserts
; `NSIS_HOOK_POSTINSTALL` after the app files and shortcuts are installed.
Var AgentExe
Var AgentResult

!macro NSIS_HOOK_POSTINSTALL
  ; Tauri's currentUser installer places the agent at $LOCALAPPDATA\<product>; older builds used
  ; the Programs subdirectory. Accept both so an upgrade does not install the agent twice.
  StrCpy $AgentExe "$LOCALAPPDATA\RedLineAgent\redline-agent-app.exe"
  ${IfNot} ${FileExists} "$AgentExe"
    StrCpy $AgentExe "$LOCALAPPDATA\Programs\RedLineAgent\redline-agent-app.exe"
  ${EndIf}
  ${IfNot} ${FileExists} "$AgentExe"
    DetailPrint "Installing RedLine Agent…"
    NSISdl::download "https://github.com/erikibolya/redline/releases/download/v${VERSION}/RedLineAgent_${VERSION}_x64-setup.exe" "$TEMP\RedLineAgent-setup.exe"
    Pop $AgentResult
    ${If} $AgentResult == "success"
      ExecWait '"$TEMP\RedLineAgent-setup.exe" /S'
      Delete "$TEMP\RedLineAgent-setup.exe"
      StrCpy $AgentExe "$LOCALAPPDATA\RedLineAgent\redline-agent-app.exe"
      ${IfNot} ${FileExists} "$AgentExe"
        StrCpy $AgentExe "$LOCALAPPDATA\Programs\RedLineAgent\redline-agent-app.exe"
      ${EndIf}
      ${If} ${FileExists} "$AgentExe"
        ; First run enables the agent's own start-at-login and opens its Settings window.
        Exec "$AgentExe"
      ${EndIf}
    ${Else}
      MessageBox MB_ICONEXCLAMATION "RedLine Agent could not be downloaded. Install it later from the RedLine release page; the desktop app is installed."
    ${EndIf}
  ${EndIf}
!macroend
