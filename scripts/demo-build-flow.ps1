param(
    [string]$ExeDir = ""
)

$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$send = Join-Path $here "send-agent-command.ps1"

& $send -Action "switch_tab" -Args @{ tab = "flow" } -ExeDir $ExeDir
& $send -Action "flow_build_from_steps" -Args @{
    steps = @(
        @{ type = "click"; element = "btn_login" },
        @{ type = "wait"; seconds = 2 },
        @{ type = "manual"; message = "输入验证码"; instruction = "填写 6 位验证码" },
        @{ type = "click"; element = "btn_submit"; fallback = "btn_submit_alt" }
    )
} -ExeDir $ExeDir

& $send -Action "flow_nodes" -ExeDir $ExeDir
& $send -Action "flow_compile" -ExeDir $ExeDir

Write-Host "demo done: flow graph rebuilt and compiled"
