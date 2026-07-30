param(
    [Parameter(Mandatory = $true)]
    [string]$Action,

    [hashtable]$Args = @{},

    [string]$Id = ("cmd-" + [guid]::NewGuid().ToString("N").Substring(0, 8)),

    [string]$ExeDir = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExeDir)) {
    $ExeDir = Split-Path -Parent $PSScriptRoot
    $candidate = Join-Path $ExeDir "target\release"
    if (Test-Path (Join-Path $candidate "mouse-suite.exe")) {
        $ExeDir = $candidate
    }
}

$dataDir = Join-Path $ExeDir "data"
New-Item -ItemType Directory -Force -Path $dataDir | Out-Null

$cmdPath = Join-Path $dataDir "agent_command.json"
$respPath = Join-Path $dataDir "agent_response.json"

$payload = [ordered]@{
    id     = $Id
    action = $Action
    args   = $Args
}

$payload | ConvertTo-Json -Depth 20 | Set-Content -Encoding UTF8 -Path $cmdPath
Write-Host "sent: $cmdPath"
Write-Host "id=$Id action=$Action"

$deadline = (Get-Date).AddSeconds(8)
while ((Get-Date) -lt $deadline) {
    if (Test-Path $respPath) {
        $raw = Get-Content -Raw -Path $respPath
        try {
            $obj = $raw | ConvertFrom-Json
            if ($obj.id -eq $Id) {
                Write-Host "response:"
                $raw
                if (-not $obj.ok) { exit 1 }
                exit 0
            }
        } catch {
            # keep waiting
        }
    }
    Start-Sleep -Milliseconds 200
}

Write-Error "timeout waiting for response id=$Id"
exit 1
