# OCR click example

```workflow
# Wait until on-screen text "确定" appears, then click it.
if_text: 确定 | retries=5 | retry_ms=500 | then=1 | else=2
click_text: 确定 | retries=1 | on_fail=skip
wait: 1
goto: 0
```

Notes:
- Prefer building this in the Flow editor with **文字条件** + **点击文字** nodes.
- Windows uses system OCR; other platforms need AI config (书写页 / AI 设置).
