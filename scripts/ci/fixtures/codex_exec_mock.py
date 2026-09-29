#!/usr/bin/env python3
"""Credential-free exec JSONL fixture for copied-binary CI smoke."""
import json
import os
from pathlib import Path
import sys

if "--help" in sys.argv or "--version" in sys.argv:
    print("codex fixture exec --json")
    raise SystemExit(0)
prompt = sys.stdin.read()
state_path = Path(os.environ["CODEX_EXEC_MOCK_STATE"])
state = json.loads(state_path.read_text()) if state_path.exists() else {"index": 0, "prompts": []}
replies = json.loads(os.environ["CODEX_EXEC_MOCK_REPLIES"])
reply = replies[min(state["index"], len(replies) - 1)]
state["index"] += 1
state["prompts"].append(prompt)
state_path.write_text(json.dumps(state))
print(json.dumps({"type": "thread.started", "thread_id": "smoke-exec-thread"}))
print(json.dumps({"type": "item.completed", "item": {"type": "agent_message", "text": reply}}))
