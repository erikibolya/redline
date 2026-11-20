#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Install the RedLine agent for this PC as a Windows service (WinSW).

.DESCRIPTION
  The desktop app's embedded agent always reports itself as "local". This script runs the
  standalone `redline-agent.exe` as a real service instead, with a device id (`erik-pc` by
  default), a scoped agent token and a fixed repository root.

  The token is stored in `C:\ProgramData\RedLine\agent\agent-token` (SYSTEM/Administrators only)
  and read by a wrapper at service start, so it never sits in the service XML or in the registry.

  Run after the v0.28.5 release (the script downloads its assets from it), or pass -AgentExe and
  -WinSW for an offline install.

.EXAMPLE
  .\install-agent-service.ps1 -RepoRoot C:\Users\eriki\source\repos `
      -OpenCodeServer http://127.0.0.1:4096

.EXAMPLE
  .\install-agent-service.ps1 -Uninstall
#>
[CmdletBinding()]
param(
    [string]$Token,
    [string]$AgentId = "erik-pc",
    [string]$RepoRoot = "C:\Users\eriki\source\repos",
    [string]$Repo = "",
    [string]$OpenCodeServer = "http://127.0.0.1:4096",
    [string]$Model = "deepseek/deepseek-flash",
    [string]$ServerUrl = "https://redline.ibolya.cz",
    [string]$Version = "0.28.5",
    [string]$AgentExe = "",
    [string]$WinSW = "",
    [switch]$Download,
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"

$ServiceName = "RedLineAgent"
$Root = "C:\ProgramData\RedLine"
$Data = Join-Path $Root "agent"
$Logs = Join-Path $Root "logs"
$TokenFile = Join-Path $Data "agent-token"
$Wrapper = Join-Path $Data "run-agent.cmd"
$XmlPath = Join-Path $Root "redline-agent-service.xml"
if (-not $WinSW) { $WinSW = Join-Path $Root "WinSW.exe" }

function Get-WinSW {
    if (Test-Path $WinSW) { return }
    Write-Host "Downloading WinSW 2.12.0..."
    New-Item -ItemType Directory -Force -Path (Split-Path $WinSW) | Out-Null
    Invoke-WebRequest -UseBasicParsing -Uri `
        "https://github.com/winsw/winsw/releases/download/v2.12.0/WinSW-x64.exe" -OutFile $WinSW
}

if ($Uninstall) {
    if (Test-Path $WinSW) {
        & $WinSW stop $ServiceName 2>$null
        & $WinSW uninstall $ServiceName 2>$null
    } else {
        sc.exe stop $ServiceName 2>$null | Out-Null
        sc.exe delete $ServiceName 2>$null | Out-Null
    }
    Write-Host "RedLineAgent removed. Files under $Root were kept."
    return
}

if (-not (Test-Path $RepoRoot)) {
    throw "Repository root not found: $RepoRoot"
}
if (-not $Repo) {
    $Repo = Join-Path $RepoRoot "redline"
}
if (-not (Test-Path $Repo)) {
    throw "Default repository not found: $Repo (pass -Repo)"
}
if (-not $AgentExe) {
    $AgentExe = "C:\Program Files\RedLine\redline-agent.exe"
}
if (-not (Test-Path $AgentExe)) {
    $url = "https://github.com/erikibolya/redline/releases/download/v$Version/redline-agent.exe"
    Write-Host "Downloading $url"
    New-Item -ItemType Directory -Force -Path (Split-Path $AgentExe) | Out-Null
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $AgentExe
}

if (-not $Token) {
    $secure = Read-Host -AsSecureString "RedLine agent token for $AgentId (rl_agt_...)"
    $Token = [Runtime.InteropServices.Marshal]::PtrToStringBSTR(
        [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))
}
if (-not $Token.StartsWith("rl_agt_")) {
    throw "That does not look like an agent token (expected rl_agt_...). Create one in RedLine -> People -> Agent tokens."
}

# Validate the token and the server before touching the service manager. This is the same
# read-only MCP call the bridge uses; a wrong token or a blocked server fails here, not in a log.
Write-Host "Checking $ServerUrl and the token..."
$probe = @{
    jsonrpc = "2.0"; id = 1; method = "tools/call"
    params  = @{ name = "list_projects"; arguments = @{} }
} | ConvertTo-Json -Depth 8
try {
    $reply = Invoke-RestMethod -Uri "$($ServerUrl.TrimEnd('/'))/mcp" -Method Post `
        -Headers @{ Authorization = "Bearer $Token" } -ContentType "application/json" -Body $probe
} catch {
    throw "The server did not answer: $($_.Exception.Message)"
}
if ($reply.error) {
    throw "The token was refused: $($reply.error.message)"
}
Write-Host "Token accepted."

New-Item -ItemType Directory -Force -Path $Data, $Logs | Out-Null
[IO.File]::WriteAllText($TokenFile, $Token, (New-Object Text.UTF8Encoding($false)))
& icacls.exe $TokenFile /inheritance:r /grant:r "SYSTEM:(R)" "Administrators:(R)" | Out-Null

$lines = @(
    "@echo off",
    "setlocal",
    "set /p REDLINE_TOKEN=<`"$TokenFile`"",
    "set `"REDLINE_URL=$($ServerUrl.TrimEnd('/'))`"",
    "set `"REDLINE_AGENT_ID=$AgentId`"",
    "set `"REDLINE_AGENT_REPO=$Repo`"",
    "set `"REDLINE_AGENT_REPO_ROOT=$RepoRoot`"",
    "set `"REDLINE_AGENT_OPENCODE_SERVER=$OpenCodeServer`"",
    "set `"REDLINE_AGENT_MODEL=$Model`"",
    "set `"GIT_CONFIG_COUNT=1`"",
    "set `"GIT_CONFIG_KEY_0=safe.directory`"",
    "set `"GIT_CONFIG_VALUE_0=*`"",
    "`"$AgentExe`""
)
[IO.File]::WriteAllLines($Wrapper, $lines, (New-Object Text.UTF8Encoding($false)))

$serviceXml = @"
<service>
  <id>$ServiceName</id>
  <name>RedLine Agent ($AgentId)</name>
  <description>RedLine review agent for this PC. Claims reviews assigned to $AgentId and runs them through OpenCode at $OpenCodeServer.</description>
  <executable>cmd.exe</executable>
  <arguments>/c "$Wrapper"</arguments>
  <workingdirectory>$RepoRoot</workingdirectory>
  <logpath>$Logs</logpath>
  <log mode="roll-by-size">
    <sizeThreshold>10240</sizeThreshold>
    <keepFiles>5</keepFiles>
  </log>
  <onfailure action="restart" delay="10 sec"/>
  <stoptimeout>30 sec</stoptimeout>
</service>
"@
[IO.File]::WriteAllText($XmlPath, $serviceXml, (New-Object Text.UTF8Encoding($false)))

Get-WinSW
& $WinSW stop $ServiceName 2>$null
& $WinSW uninstall $ServiceName 2>$null
& $WinSW install $XmlPath
& $WinSW start $ServiceName

Start-Sleep -Seconds 2
& $WinSW status $ServiceName
Write-Host ""
Write-Host "Installed. Logs: $Logs"
Write-Host "The service runs as LocalSystem and uses the OpenCode server at $OpenCodeServer."
Write-Host "If the repositories or OpenCode are only reachable in your user session, run the"
Write-Host "service as that user (WinSW <serviceaccount>) or use the desktop app's local agent."
