# Agent Bridge

Mouse Suite exposes a file-based command bridge for automation agents.

## Files

- Command input: `data/agent_command.json`
- Response output: `data/agent_response.json`

Both paths are relative to the executable directory.

## Command format

```json
{
  "id": "unique-command-id",
  "action": "status",
  "args": {}
}
```

- `id` must be unique per command.
- Reusing the same `id` is ignored (dedup protection).

## Response format

```json
{
  "id": "unique-command-id",
  "ok": true,
  "message": "ok",
  "data": {}
}
```

## Supported actions

- `status` (includes threshold / retries / on_fail / hide_wait_ms / save_match_debug)
- `switch_tab` (`args.tab`: `recorder|clicker|flow|scribe`)
- `scribe_start`
- `scribe_stop`
- `scribe_export_html` (`args.path`)
- `scribe_to_flow` (会话步骤 → 流程图人工节点草稿)
- `recorder_refresh`
- `recorder_export_csv` (`args.name`)
- `recorder_set_hide_wait_ms` (`args.ms` 500–3000)
- `clicker_set_delay` (`args.ms`)
- `clicker_set_element_folder` (`args.path`)
- `clicker_set_threshold` (`args.threshold` 0.5–0.99)
- `clicker_set_pure_vision` (`args.enabled` bool)
- `clicker_set_retries` (`args.retries` 0–20)
- `clicker_set_retry_ms` (`args.ms`)
- `clicker_set_on_fail` (`args.action`: `skip|abort`)
- `clicker_set_save_match_debug` (`args.enabled` bool)
- `clicker_load_workflow` (`args.path`)
- `clicker_start_workflow`
- `clicker_stop`
- `flow_reset`
- `flow_add_node` (`args.kind`: `start|end|click|wait|type|pause|manual|loop|loop_end|if_vision|loop_while`)
- `flow_connect` (`args.from`, `args.to`, optional `args.branch`: `main|true|false` — required for IfVision dual outs)
- `flow_compile` (runs static checks: loop pairing, IfVision missing edges, Start out, orphans)
- `flow_build_from_steps` (`args.steps`: supports `loop`/`loop_while`/`loop_end`/`if_vision`/`goto`/`type`, click `threshold`/`pure_vision`/`retries`/`retry_ms`/`on_fail`)
- `flow_build_from_text` (`args.text`: workflow text content)
- `flow_run`
- `flow_load` (`args.path` — supports `.md` / `.flow.json` / `.json` / `.txt`)
- `flow_save` (`args.path` — `.md` writes Mermaid + `mouse-suite-flow` JSON; else JSON graph)
- `flow_load_example` (`args.name`: `vision-click-10` — built-in sample)
- `flow_ai_generate` (`args.prompt` required; optional `args.replace` default `true` — replace canvas with AI graph)
- `flow_nodes`
- `flow_auto_layout`
- `flow_duplicate` (copy+paste current selection)
- `flow_undo`
- `flow_redo`
- `ai_get_config` (masked; no raw keys)
- `ai_set_config` (clickscribe-compatible: empty keys do not overwrite)

## Markdown flowchart (Agent-friendly)

Round-trip format written by `flow_save` to `*.md`:

1. `# Title` + `>` description lines  
2. ```mermaid``` diagram (human / preview)  
3. ```mouse-suite-flow``` JSON — **source of truth** (`version`, `title`, `nodes`, `edges`, `next_id`)

Import (`flow_load` / UI 打开): prefers the JSON fence; if missing, best-effort Mermaid parse.

Example path in repo: `workflows/examples/vision-click-10.md`  
Load via UI「加载示例」or Agent:

```json
{ "id": "ex1", "action": "flow_load_example", "args": { "name": "vision-click-10" } }
```

## AI config (ccswitch / GLM / custom)

Stored at `data/ai_config.json` (migrates legacy `data/scribe_ai.json`). Same schema as clickscribe:

```json
{
  "provider": "ccswitch",
  "glm_key": "",
  "custom_base": "",
  "custom_key": "",
  "custom_model": ""
}
```

| provider | Endpoint | Auth |
|----------|----------|------|
| `ccswitch` (default) | `http://127.0.0.1:15721/v1/messages` (Anthropic) | none (CC Switch local proxy must be green) |
| `glm` | 智谱 `.../paas/v4/chat/completions` | Bearer `glm_key` |
| `custom` | OpenAI-compatible base | Bearer `custom_key` (optional) |

```json
{ "id": "ai1", "action": "ai_get_config", "args": {} }
{ "id": "ai2", "action": "ai_set_config", "args": { "provider": "ccswitch" } }
```

### Natural-language flow generation

Uses the same `data/ai_config.json` (default CC Switch `:15721` Anthropic Messages). On success switches to the Flow tab and replaces the canvas when `replace` is true (default).

```json
{
  "id": "gen1",
  "action": "flow_ai_generate",
  "args": {
    "prompt": "匹配登录按钮成功则点击，共成功 10 次",
    "replace": true
  }
}
```

Response `data`: `{ "title": "...", "nodes": <count>, "edges": <count> }`.

## PowerShell helpers

From repo root:

```powershell
# Send one command and wait for matching response
.\scripts\send-agent-command.ps1 -Action status

# Build a sample flow graph (app must be running)
.\scripts\demo-build-flow.ps1
```

`send-agent-command.ps1` writes `data/agent_command.json` next to the release exe and waits for `data/agent_response.json`.

## `flow_build_from_steps` example

```json
{
  "id": "cmd-build-01",
  "action": "flow_build_from_steps",
  "args": {
    "steps": [
      {"type": "click", "element": "btn_login", "threshold": 0.85, "retries": 2, "retry_ms": 500, "on_fail": "abort", "pure_vision": true},
      {"type": "wait", "seconds": 2},
      {"type": "type", "text": "hello{Enter}", "interval_ms": 30},
      {"type": "manual", "message": "输入验证码", "instruction": "填写 6 位验证码"},
      {"type": "click", "element": "btn_submit", "fallback": "btn_submit_alt"}
    ]
  }
}
```

### `type` (keyboard input)

Types into the **currently focused** control. Supports Unicode (中文) and special tokens.

```text
type: 用户名{Tab}密码{Enter} | interval_ms=40
```

Tokens: `{Enter}` `{Tab}` `{Esc}` `{Backspace}` `{Delete}` `{Space}` `{Up/Down/Left/Right}` `{Home}` `{End}` `{Ctrl+A/C/V/X/Z}` — literal braces: `{{` `}}`.

JSON: `{"type":"type","text":"hello{Enter}","interval_ms":30}`

## Branch / conditional loop notes

### `if_vision` (vision-only branch, no click)

Graph: add `if_vision` node, connect **True** (`branch: "true"`) and **False** (`branch: "false"`) outs. Compile emits jumps.

Text / steps:

```text
if_vision: banner | threshold=0.8 | retries=1 | then=1 | else=3
click: path_a
goto: 4
click: path_b
```

JSON step: `{"type":"if_vision","element":"banner","threshold":0.8,"then":1,"else":3}`

### `loop_while` (continue while template matches)

Pairs with `loop_end`. Stops on miss or `max_times` (default 50).

```text
loop_while: loading | threshold=0.75 | max_times=50
wait: 1
loop_end:
```

JSON: `{"type":"loop_while","element":"loading","threshold":0.75,"max_times":50}`

Agent connect example for IfVision:

```json
{"action":"flow_connect","args":{"from":3,"to":4,"branch":"true"}}
{"action":"flow_connect","args":{"from":3,"to":5,"branch":"false"}}
```

## Vision debug notes

- Match logs include `score` and `thr` for ROI / full-screen attempts.
- When `save_match_debug` is enabled, a failed full-screen match writes a PNG under `{element_folder}/debug/`.

## Global hotkeys (Windows)

- **Ctrl+Alt+F9** — compile current flow and start (works while minimized)
- **Ctrl+Alt+F10** — stop running workflow / clicker
