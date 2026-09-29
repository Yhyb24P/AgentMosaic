#!/usr/bin/env python3
"""Scripted external Codex exec peer for product CLI integration tests."""
import json
import os
import pathlib
import sys

if "--help" in sys.argv:
    print("codex exec --json")
    sys.exit(0)
if os.environ.get("AM_TEST_FORBID_RUNTIME"):
    raise SystemExit("runtime must not be replayed")
sys.stdin.read()
state = pathlib.Path(".agentmosaic/test-reply-index")
index = int(state.read_text()) if state.exists() else 0
replies = json.loads(os.environ.get("AM_TEST_EXEC_REPLIES", "[]"))
reply = replies[min(index, len(replies) - 1)] if replies else "invalid response"
state.write_text(str(index + 1))
print(json.dumps({"type": "thread.started", "thread_id": "mock-thread"}))
print(json.dumps({"type": "item.completed", "item": {"type": "agent_message", "text": reply}}))
