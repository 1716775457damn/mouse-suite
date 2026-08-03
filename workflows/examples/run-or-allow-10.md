# 成功点击10次

> mouse-suite-flow v1
> AI 生成：看到 run/allow 就点，成功 10 次

```mouse-suite-flow
{
  "version": 1,
  "title": "成功点击10次",
  "nodes": [
    {
      "id": 1,
      "kind": "Start",
      "pos": [
        60,
        180
      ]
    },
    {
      "id": 2,
      "kind": "LoopStart",
      "pos": [
        240,
        180
      ],
      "seconds": 10
    },
    {
      "id": 3,
      "kind": "IfVision",
      "pos": [
        440,
        180
      ],
      "element_name": "run",
      "or_elements": [
        "allow"
      ],
      "threshold": 0.85,
      "retries": 0,
      "retry_ms": 300
    },
    {
      "id": 4,
      "kind": "Click",
      "pos": [
        640,
        120
      ],
      "element_name": "run",
      "or_elements": [
        "allow"
      ],
      "threshold": 0.85,
      "pure_vision": true,
      "retries": 0,
      "retry_ms": 300,
      "on_fail": "skip"
    },
    {
      "id": 5,
      "kind": "LoopEnd",
      "pos": [
        840,
        180
      ]
    },
    {
      "id": 6,
      "kind": "End",
      "pos": [
        1040,
        180
      ]
    }
  ],
  "edges": [
    {
      "from": 1,
      "to": 2,
      "branch": "main"
    },
    {
      "from": 2,
      "to": 3,
      "branch": "main"
    },
    {
      "from": 3,
      "to": 4,
      "branch": "true"
    },
    {
      "from": 3,
      "to": 3,
      "branch": "false"
    },
    {
      "from": 4,
      "to": 5,
      "branch": "main"
    },
    {
      "from": 5,
      "to": 6,
      "branch": "main"
    }
  ],
  "next_id": 7
}
```
