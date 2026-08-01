#!/usr/bin/env python3
"""Smoke-test NL flow generation via CC Switch Anthropic proxy (:15721)."""
from __future__ import annotations

import json
import sqlite3
import sys
import urllib.error
import urllib.request
from pathlib import Path

PROXY = "http://127.0.0.1:15721/v1/messages"
MODEL = "claude-haiku-4-5"
PROMPT = "匹配 element 成功则点击，共成功 10 次"

SYSTEM = r"""你是 Mouse Suite 流程图生成器。根据用户的中文需求，输出**一个** JSON 对象（不要 Markdown 说明文字，不要 mermaid）。

JSON schema（mouse-suite-flow v1）:
{
  "version": 1,
  "title": "短标题",
  "nodes": [ FlowNode, ... ],
  "edges": [ { "from": id, "to": id, "branch": "main"|"true"|"false" }, ... ],
  "next_id": <最大id+1>
}

FlowNode 必填字段:
- id: number (从 1 起)
- kind: "Start"|"End"|"Click"|"Wait"|"TypeText"|"Pause"|"Manual"|"LoopStart"|"LoopEnd"|"IfVision"|"LoopWhile"
- pos: [x, y]
- element_name: string
- or_elements: []
- fallback: ""
- threshold: 0.85
- pure_vision: false
- retries: 0
- retry_ms: 300
- on_fail: "skip"
- seconds: number
- max_times: 50
- message / instruction: string
- type_text: string
- type_interval_ms: 30

规则:
1. 必须有且仅有一个 Start、一个 End；边要连通。
2. 「成功点击 N 次 / 匹配到才算一次」: LoopStart(seconds=N) → IfVision —true→ Click → LoopEnd → End；IfVision 的 false **接回自身**（不占次数）。
3. IfVision 必须有 true 与 false 两条出边。
6. 只输出 JSON 对象本身。"""


def ensure_cc_key() -> None:
    db = Path.home() / ".cc-switch" / "cc-switch.db"
    if not db.exists():
        return
    con = sqlite3.connect(str(db))
    row = con.execute("SELECT settings_config FROM providers WHERE id='cc'").fetchone()
    if not row:
        con.close()
        return
    key = json.loads(row[0])["options"]["apiKey"]
    cid = "universal-claude-8cb92e3d-5651-4628-806b-c2ec48d3190e"
    cfg = json.loads(
        con.execute("SELECT settings_config FROM providers WHERE id=?", (cid,)).fetchone()[0]
    )
    cfg.setdefault("env", {})["ANTHROPIC_AUTH_TOKEN"] = key
    con.execute(
        "UPDATE providers SET settings_config=? WHERE id=?",
        (json.dumps(cfg, ensure_ascii=False), cid),
    )
    con.execute("UPDATE proxy_config SET proxy_enabled=1, enabled=1, live_takeover_active=1")
    con.commit()
    con.close()
    print(f"patched provider key prefix={key[:8]}")


def extract_json(text: str) -> dict:
    text = text.strip()
    if text.startswith("```"):
        lines = text.splitlines()
        if lines and lines[0].startswith("```"):
            lines = lines[1:]
        if lines and lines[-1].strip() == "```":
            lines = lines[:-1]
        text = "\n".join(lines).strip()
        for prefix in ("json", "mouse-suite-flow"):
            if text.startswith(prefix):
                text = text[len(prefix) :].lstrip()
    start, end = text.find("{"), text.rfind("}")
    if start < 0 or end <= start:
        raise ValueError("no JSON object in model output")
    return json.loads(text[start : end + 1])


def main() -> int:
    ensure_cc_key()
    user = (
        f"用户需求：\n{PROMPT}\n\n"
        "元素库：暂无已录制元素，请自行起合理的 element_name（英文或拼音）。\n\n"
        "请输出 mouse-suite-flow JSON。"
    )
    payload = {
        "model": MODEL,
        "max_tokens": 4096,
        "temperature": 0.2,
        "system": SYSTEM,
        "messages": [{"role": "user", "content": user}],
    }
    req = urllib.request.Request(
        PROXY,
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "anthropic-version": "2023-06-01",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=180) as resp:
            body = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        print("HTTP", e.code, e.read().decode("utf-8", errors="replace")[:500])
        return 1
    except Exception as e:
        print("request failed:", e)
        return 1

    text = "".join(
        b.get("text", "") for b in body.get("content", []) if b.get("type") == "text"
    )
    print("--- model text (first 600) ---")
    print(text[:600])
    doc = extract_json(text)
    kinds = [n.get("kind") for n in doc.get("nodes", [])]
    edges = doc.get("edges", [])
    print("--- parsed ---")
    print("title=", doc.get("title"))
    print("kinds=", kinds)
    print("edges=", len(edges), "next_id=", doc.get("next_id"))

    ok = True
    if "Start" not in kinds or "End" not in kinds:
        print("FAIL: missing Start/End")
        ok = False
    if "LoopStart" not in kinds or "IfVision" not in kinds:
        print("FAIL: expected LoopStart + IfVision for success-count pattern")
        ok = False
    # false self-loop on IfVision
    if_ids = {n["id"] for n in doc["nodes"] if n.get("kind") == "IfVision"}
    self_false = [
        e
        for e in edges
        if e.get("from") in if_ids
        and e.get("to") == e.get("from")
        and str(e.get("branch", "")).lower() == "false"
    ]
    if not self_false:
        print("FAIL: IfVision false edge should self-loop")
        ok = False
    else:
        print("OK: IfVision false self-loop present")

    if ok:
        print("PASS: CC Switch flow AI smoke test")
        return 0
    print("FAIL: structure checks")
    return 2


if __name__ == "__main__":
    sys.exit(main())
