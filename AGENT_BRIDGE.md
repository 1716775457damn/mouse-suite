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

- `status`
- `switch_tab` (`args.tab`: `recorder|clicker|flow`)
- `recorder_refresh`
- `recorder_export_csv` (`args.name`)
- `clicker_set_delay` (`args.ms`)
- `clicker_set_element_folder` (`args.path`)
- `clicker_load_workflow` (`args.path`)
- `clicker_start_workflow`
- `clicker_stop`
- `flow_reset`
- `flow_add_node` (`args.kind`: `start|end|click|wait|pause|manual`)
- `flow_connect` (`args.from`, `args.to`)
- `flow_compile`
- `flow_build_from_steps` (`args.steps`: step array)
- `flow_build_from_text` (`args.text`: workflow text content)
- `flow_run`
- `flow_load` (`args.path`)
- `flow_save` (`args.path`)
- `flow_nodes`

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
      {"type": "click", "element": "btn_login"},
      {"type": "wait", "seconds": 2},
      {"type": "manual", "message": "输入验证码", "instruction": "填写 6 位验证码"},
      {"type": "click", "element": "btn_submit", "fallback": "btn_submit_alt"}
    ]
  }
}
```
