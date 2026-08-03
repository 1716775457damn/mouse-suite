# -*- coding: utf-8 -*-
from pathlib import Path

REPLACEMENTS = {
    "recorder.rs": [
        ('"元素库"', 'crate::i18n::t("recorder.header.title")'),
        ('"框选屏幕区域，保存为可复用模板"', 'crate::i18n::t("recorder.header.subtitle")'),
        ('"新建元素  F5"', 'crate::i18n::t("recorder.btn.new")'),
        ('"刷新"', 'crate::i18n::t("recorder.btn.refresh")'),
        ('"添加状态  F6"', 'crate::i18n::t("recorder.btn.add_state")'),
        ('"导出 CSV"', 'crate::i18n::t("recorder.btn.export")'),
        ('"筛选"', 'crate::i18n::t("recorder.filter")'),
        ('"按名称过滤…"', 'crate::i18n::t("recorder.filter.hint")'),
        (
            '"暂无元素。点击「新建元素」开始录制模板。"',
            'crate::i18n::t("recorder.empty")',
        ),
        ('"没有匹配的元素。"', 'crate::i18n::t("recorder.no_match")'),
        ('"截屏前隐藏等待"', 'crate::i18n::t("recorder.hide_wait")'),
        ('"开始录制  F5"', 'crate::i18n::t("recorder.capture.start")'),
    ],
    "clicker.rs": [
        ('"自动点击"', 'crate::i18n::t("clicker.header.title")'),
        (
            '"CSV 坐标序列或工作流文件回放"',
            'crate::i18n::t("clicker.header.subtitle")',
        ),
        ('"浏览"', 'crate::i18n::t("clicker.btn.browse")'),
        ('"加载"', 'crate::i18n::t("clicker.btn.load")'),
        ('"开始（3 秒倒计时）"', 'crate::i18n::t("clicker.btn.start")'),
        ('"停止"', 'crate::i18n::t("clicker.btn.stop")'),
        ('"重置"', 'crate::i18n::t("clicker.btn.reset")'),
        (
            '"开始工作流（3 秒倒计时）"',
            'crate::i18n::t("clicker.btn.start_wf")',
        ),
        ('"CSV 文件"', 'crate::i18n::t("clicker.field.csv")'),
        ('"点击间隔"', 'crate::i18n::t("clicker.field.interval")'),
        ('"视觉阈值"', 'crate::i18n::t("clicker.field.threshold")'),
        ('"纯视觉"', 'crate::i18n::t("clicker.pure_vision")'),
        ('"元素目录"', 'crate::i18n::t("clicker.field.element_dir")'),
        ('"工作流"', 'crate::i18n::t("clicker.field.workflow")'),
        ('"运行日志"', 'crate::i18n::t("clicker.log")'),
        ('"高级选项"', 'crate::i18n::t("clicker.advanced")'),
        ('"模式"', 'crate::i18n::t("clicker.mode")'),
        ('"工作流文件"', 'crate::i18n::t("clicker.mode.workflow")'),
    ],
    "flow.rs": [
        ('"节点库"', 'crate::i18n::t("flow.toolbox.title")'),
        (
            '"拖出端口连线 · 双端口=条件"',
            'crate::i18n::t("flow.toolbox.subtitle")',
        ),
        ('"属性"', 'crate::i18n::t("flow.inspector.title")'),
        ('"选中节点以编辑属性"', 'crate::i18n::t("flow.inspector.select")'),
        ('"系统节点，无需配置"', 'crate::i18n::t("flow.inspector.system")'),
        ('"重置示例图"', 'crate::i18n::t("flow.btn.reset")'),
        ('"自动布局"', 'crate::i18n::t("flow.btn.layout")'),
        ('"撤销  Ctrl+Z"', 'crate::i18n::t("flow.btn.undo")'),
        ('"重做  Ctrl+Y"', 'crate::i18n::t("flow.btn.redo")'),
        ('"复制选中  Ctrl+C"', 'crate::i18n::t("flow.btn.copy")'),
        ('"粘贴  Ctrl+V"', 'crate::i18n::t("flow.btn.paste")'),
        ('"删除选中  Del"', 'crate::i18n::t("flow.btn.delete")'),
        ('"打开"', 'crate::i18n::t("flow.btn.open")'),
        ('"保存"', 'crate::i18n::t("flow.btn.save")'),
        ('"▶  运行流程"', 'crate::i18n::t("flow.btn.run")'),
        (
            '"加载示例：视觉点击×10"',
            'crate::i18n::t("flow.btn.example")',
        ),
        ('"📷 截屏绑定模板"', 'crate::i18n::t("flow.btn.shot")'),
        ('"文件"', 'crate::i18n::t("flow.section.file")'),
        ('"标题（导出 MD）"', 'crate::i18n::t("flow.field.title")'),
        ('"说明"', 'crate::i18n::t("flow.field.desc")'),
        ('"元素"', 'crate::i18n::t("flow.field.element")'),
        ('"视觉匹配阈值"', 'crate::i18n::t("flow.field.threshold")'),
        (
            '"本节点纯视觉（匹配才点）"',
            'crate::i18n::t("flow.field.pure_vision")',
        ),
        ('"重试次数"', 'crate::i18n::t("flow.field.retries")'),
        ('"重试间隔(ms)"', 'crate::i18n::t("flow.field.retry_ms")'),
        ('"失败策略"', 'crate::i18n::t("flow.field.on_fail")'),
        ('"跳过继续"', 'crate::i18n::t("flow.fail.skip")'),
        ('"中止流程"', 'crate::i18n::t("flow.fail.abort")'),
        ('"秒数"', 'crate::i18n::t("flow.field.seconds")'),
        ('"输入内容"', 'crate::i18n::t("flow.field.type_text")'),
        ('"按键间隔(ms)"', 'crate::i18n::t("flow.field.type_ms")'),
        ('"循环次数"', 'crate::i18n::t("flow.field.loop_times")'),
        ('"最大次数"', 'crate::i18n::t("flow.field.max_times")'),
        ('"提示消息"', 'crate::i18n::t("flow.field.message")'),
        ('"操作说明"', 'crate::i18n::t("flow.field.instruction")'),
        ('"AI 生成流程图"', 'crate::i18n::t("flow.ai.title")'),
        (
            '"自然语言 → 可编辑流程图（需 CC Switch 或文档页 AI 设置）"',
            'crate::i18n::t("flow.ai.subtitle")',
        ),
        (
            '"例：看到登录按钮就点，成功点 10 次"',
            'crate::i18n::t("flow.ai.hint")',
        ),
        ('"生成中…"', 'crate::i18n::t("flow.ai.busy")'),
        ('"正在请求模型，请稍候…"', 'crate::i18n::t("flow.ai.wait")'),
        (
            '"空白拖平移 · Ctrl+拖框选 · Ctrl+Z/Y · Ctrl+L 布局"',
            'crate::i18n::t("flow.canvas.hints")',
        ),
    ],
    "scribe.rs": [
        ('"操作文档"', 'crate::i18n::t("scribe.header.title")'),
        (
            '"F8 录制 · 编辑说明 · 导出指南"',
            'crate::i18n::t("scribe.header.subtitle")',
        ),
        ('"开始录制"', 'crate::i18n::t("scribe.btn.start")'),
        ('"停止录制"', 'crate::i18n::t("scribe.btn.stop")'),
        ('"AI 设置"', 'crate::i18n::t("scribe.btn.ai_settings")'),
        ('"AI 写说明"', 'crate::i18n::t("scribe.btn.ai_write")'),
        ('"AI 生成中…"', 'crate::i18n::t("scribe.btn.ai_busy")'),
        ('"生成流程图"', 'crate::i18n::t("scribe.btn.gen_flow")'),
        ('"录制时最小化"', 'crate::i18n::t("scribe.minimize")'),
        ('"会话"', 'crate::i18n::t("scribe.sessions")'),
        ('"刷新"', 'crate::i18n::t("scribe.refresh")'),
        ('"暂无会话"', 'crate::i18n::t("scribe.no_sessions")'),
        (
            '"开始录制，或从左侧打开会话"',
            'crate::i18n::t("scribe.empty")',
        ),
        ('"正在录制"', 'crate::i18n::t("scribe.recording")'),
        ('"文档标题"', 'crate::i18n::t("scribe.field.title")'),
        ('"应用"', 'crate::i18n::t("scribe.btn.apply")'),
        ('"删除此步"', 'crate::i18n::t("scribe.btn.delete_step")'),
        ('"切聚焦"', 'crate::i18n::t("scribe.preview.focus")'),
        ('"切全屏"', 'crate::i18n::t("scribe.preview.full")'),
        (
            '"截图预览 · 全屏（橙圈已烧录）"',
            'crate::i18n::t("scribe.preview.full_label")',
        ),
        (
            '"截图预览 · 聚焦（橙圈已烧录）"',
            'crate::i18n::t("scribe.preview.crop_label")',
        ),
        ('"无预览图"', 'crate::i18n::t("scribe.preview.none")'),
        ('"重新加载预览"', 'crate::i18n::t("scribe.preview.reload")'),
        ('"步骤标题"', 'crate::i18n::t("scribe.field.step_title")'),
        ('"步骤"', 'crate::i18n::t("scribe.field.step")'),
        ('"说明"', 'crate::i18n::t("scribe.field.step_desc")'),
        ('"AI 接入"', 'crate::i18n::t("scribe.ai.provider")'),
        ('"CC Switch 跟随代理"', 'crate::i18n::t("scribe.ai.ccswitch")'),
        ('"智谱 GLM 直连"', 'crate::i18n::t("scribe.ai.glm")'),
        ('"自定义 API"', 'crate::i18n::t("scribe.ai.custom")'),
        ('"保存 AI 设置"', 'crate::i18n::t("scribe.ai.save")'),
    ],
}


def main() -> None:
    root = Path(__file__).resolve().parents[1] / "src"
    for fname, pairs in REPLACEMENTS.items():
        path = root / fname
        text = path.read_text(encoding="utf-8")
        total = 0
        for old, new in pairs:
            n = text.count(old)
            if n:
                text = text.replace(old, new)
                total += n
                print(f"{fname}: {n}x ok")
            else:
                print(f"{fname}: MISS key->{new}")
        # scribe Save button (RichText::new("保存")) — only the toolbar one left as literal
        if fname == "scribe.rs":
            old = 'RichText::new("保存")'
            new = 'RichText::new(crate::i18n::t("scribe.btn.save"))'
            n = text.count(old)
            if n:
                text = text.replace(old, new)
                total += n
                print(f"{fname}: {n}x save-btn")
        path.write_text(text, encoding="utf-8")
        print(f"== {fname}: {total} replacements\n")


if __name__ == "__main__":
    main()
