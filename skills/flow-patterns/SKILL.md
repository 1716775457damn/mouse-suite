# Flow Patterns Skill

按用户意图选择下列标准拓扑。下面的 JSON 是**完整可运行示例**，生成时替换元素名与次数，保持边的结构。

## Pattern A — 线性：点 A → 等待 → 点 B

适用：「打开登录，等 2 秒，再点提交」

```json
{"version":1,"title":"登录提交","nodes":[{"id":1,"kind":"Start","pos":[60,180]},{"id":2,"kind":"Click","pos":[260,180],"element_name":"btn_login","threshold":0.85,"pure_vision":true,"retries":2,"retry_ms":300,"on_fail":"skip"},{"id":3,"kind":"Wait","pos":[460,180],"seconds":2},{"id":4,"kind":"Click","pos":[660,180],"element_name":"btn_submit","threshold":0.85,"pure_vision":true,"retries":2,"retry_ms":300,"on_fail":"skip"},{"id":5,"kind":"End","pos":[860,180]}],"edges":[{"from":1,"to":2,"branch":"main"},{"from":2,"to":3,"branch":"main"},{"from":3,"to":4,"branch":"main"},{"from":4,"to":5,"branch":"main"}],"next_id":6}
```

## Pattern B — 成功点击 N 次（匹配到才算一次）

适用：「看到按钮就点，成功点 10 次」「匹配成功才计数」

拓扑：`Start → LoopStart(seconds=N) → IfVision —true→ Click → LoopEnd → End`  
**关键**：`IfVision` 的 `false` **接回 IfVision 自身**（重试，不消耗循环次数）。

```json
{"version":1,"title":"视觉成功点击10次","nodes":[{"id":1,"kind":"Start","pos":[60,180]},{"id":2,"kind":"LoopStart","pos":[240,180],"seconds":10},{"id":3,"kind":"IfVision","pos":[440,180],"element_name":"target","threshold":0.85,"retries":0,"retry_ms":300},{"id":4,"kind":"Click","pos":[640,120],"element_name":"target","threshold":0.85,"pure_vision":true,"retries":0,"retry_ms":300,"on_fail":"skip"},{"id":5,"kind":"LoopEnd","pos":[840,180]},{"id":6,"kind":"End","pos":[1040,180]}],"edges":[{"from":1,"to":2,"branch":"main"},{"from":2,"to":3,"branch":"main"},{"from":3,"to":4,"branch":"true"},{"from":3,"to":3,"branch":"false"},{"from":4,"to":5,"branch":"main"},{"from":5,"to":6,"branch":"main"}],"next_id":7}
```

## Pattern C — 盲目循环 N 次（每次都点，不问是否匹配）

适用：「循环点 5 次」且未强调「成功才算」

拓扑：`Start → LoopStart(seconds=N) → Click → LoopEnd → End`

```json
{"version":1,"title":"循环点击5次","nodes":[{"id":1,"kind":"Start","pos":[60,180]},{"id":2,"kind":"LoopStart","pos":[240,180],"seconds":5},{"id":3,"kind":"Click","pos":[440,180],"element_name":"target","threshold":0.85,"retries":1,"retry_ms":300,"on_fail":"skip"},{"id":4,"kind":"LoopEnd","pos":[640,180]},{"id":5,"kind":"End","pos":[840,180]}],"edges":[{"from":1,"to":2,"branch":"main"},{"from":2,"to":3,"branch":"main"},{"from":3,"to":4,"branch":"main"},{"from":4,"to":5,"branch":"main"}],"next_id":6}
```

## Pattern D — 条件分支

适用：「如果出现错误提示就点重试，否则点继续」

`IfVision` **必须**两条出边：`true` 与 `false`，分别接到不同后续节点，最后汇合到 End（或各自结束前汇合）。

## Pattern E — 条件循环 LoopWhile

适用：「只要还能看到『下一步』就继续点，最多 20 次」

拓扑：`Start → LoopWhile(element,max_times) → Click → LoopEnd → End`  
说明：匹配到元素则进入循环体；匹配失败或达到 `max_times` 则走出 LoopEnd。

```json
{"version":1,"title":"一直点下一步","nodes":[{"id":1,"kind":"Start","pos":[60,180]},{"id":2,"kind":"LoopWhile","pos":[260,180],"element_name":"btn_next","threshold":0.85,"max_times":20,"retries":0,"retry_ms":300},{"id":3,"kind":"Click","pos":[480,180],"element_name":"btn_next","threshold":0.85,"pure_vision":true,"on_fail":"skip"},{"id":4,"kind":"LoopEnd","pos":[700,180]},{"id":5,"kind":"End","pos":[900,180]}],"edges":[{"from":1,"to":2,"branch":"main"},{"from":2,"to":3,"branch":"main"},{"from":3,"to":4,"branch":"main"},{"from":4,"to":5,"branch":"main"}],"next_id":6}
```

## Pattern F — 输入文本

适用：「在搜索框输入 hello 并回车」

在聚焦假设成立时：`Click(输入框)` → `TypeText("hello{Enter}")`。  
若用户只说输入、未提点击，也可直接 `TypeText`。

## Pattern G — 文字识别条件 / 点文字

适用：「屏幕上出现『确定』就点它」「看到错误提示就走失败分支」

- `IfText`：OCR 找到 `needle` → true，否则 false（两出边必填）
- `ClickText`：OCR 找到后点击文字包围盒中心

拓扑示例：`Start → IfText("确定") —true→ ClickText("确定") → End`，false 可接 Wait 再绕回 IfText。

```json
{"version":1,"title":"文字确定","nodes":[{"id":1,"kind":"Start","pos":[60,180]},{"id":2,"kind":"IfText","pos":[260,180],"needle":"确定","match_exact":false,"case_sensitive":false,"retries":2,"retry_ms":400},{"id":3,"kind":"ClickText","pos":[500,120],"needle":"确定","retries":1,"retry_ms":300,"on_fail":"skip"},{"id":4,"kind":"Wait","pos":[500,260],"seconds":1},{"id":5,"kind":"End","pos":[720,180]}],"edges":[{"from":1,"to":2,"branch":"main"},{"from":2,"to":3,"branch":"true"},{"from":2,"to":4,"branch":"false"},{"from":3,"to":5,"branch":"main"},{"from":4,"to":2,"branch":"main"}],"next_id":6}
```

## 意图判别口诀

1. 提到「成功 / 匹配到 / 看到才点 / 才算一次」且是**图片模板** → **Pattern B**
2. 只说「循环 N 次点击」→ **Pattern C**
3. 说「一直 / 直到消失 / 有就继续」→ **Pattern E**
4. 「如果…否则…」用模板图 → **Pattern D**
5. 「出现某段文字 / OCR / 点这个字」→ **Pattern G**
6. 普通顺序操作 → **Pattern A**
7. 输入文字 → **Pattern F**
