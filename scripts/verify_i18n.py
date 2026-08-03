# -*- coding: utf-8 -*-
from pathlib import Path

root = Path(__file__).resolve().parents[1] / "src"
for name in ("recorder.rs", "clicker.rs", "flow.rs", "scribe.rs"):
    text = (root / name).read_text(encoding="utf-8")
    print(name, "i18n::t count =", text.count("crate::i18n::t"))
    print("  toolbox.title", "flow.toolbox.title" in text)
    print("  节点库 literal", '"节点库"' in text)
    print("  操作文档 literal", '"操作文档"' in text)
