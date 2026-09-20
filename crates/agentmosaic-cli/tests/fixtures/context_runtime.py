"""Deterministic JSONL peer for the public CLI context-integrity regression.

The product creates tasks, invokes workers, collects artifacts and constructs
the final context. This fixture only supplies external runtime responses.
"""
import json
import pathlib
import sys

prompt = sys.stdin.read()
if sys.argv[1] == "lead":
    prefix = "Current lead context (compact JSON):\n"
    segment = prompt.split(prefix, 1)[1].split("\nReply with exactly", 1)[0]
    context = json.loads(segment)  # Reject malformed JSON at the actual transport.
    assert len(segment.encode()) <= 32554
    if not context["results"]:
        reply = {
            "action": "delegate",
            "tasks": [
                {"kind": "bulk", "target": "worker", "objective": f"produce result {n}"}
                for n in range(32)
            ],
        }
    else:
        assert len(context["results"]) == 32
        assert len(context["artifacts"]) == 32
        expected_path = pathlib.Path("expected-path.txt").read_text()
        for artifact in context["artifacts"]:
            assert artifact["path"] == expected_path
            assert artifact["task_id"] in [item["task_id"] for item in context["results"]]
        pathlib.Path("delivered-context.json").write_text(segment)
        reply = {
            "action": "complete",
            "answer": "all 32 results and artifact owners received",
            "selected_task_ids": [result["task_id"] for result in context["results"]],
            "selected_artifacts": context["artifacts"],
        }
else:
    reply = {"summary": ('quoted "line"\\\n中文🦀 ' * 150)}

print(json.dumps({"type": "thread.started", "thread_id": "context-fixture-thread"}))
print(json.dumps({"type": "item.completed", "item": {
    "type": "agent_message", "text": json.dumps(reply, ensure_ascii=False),
}}))
