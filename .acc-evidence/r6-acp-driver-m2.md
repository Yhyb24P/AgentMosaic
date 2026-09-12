# M2 live evidence — ACP driver against local `qwen --acp`

- recorded: 2026-09-12
- tested_source_commit: `57be775307aab9db813838f19ac4bde1cda71c6f` (M2-M9
  action-guide run checkpoint; tree clean, preflight passed)
- guide refs: action guide §4.3 (live entry sequence), §4.4 (a probe proves
  only what was run, not readiness), §4.5 (evidence fields); §5 keeps
  `M3_QWEN_WORKER_READY` at IN_PROGRESS
- carried-over references (NOT restated here): `r6-qwen-acp-initialize.md`
  (an earlier Rust-driver bootstrap that ended in `BLOCKED_RUNTIME_UNRESPONSIVE`)
  and `r6-codex-qwen-live.md` — this file supersedes their runtime facts only
- resume discipline: no resume / newSession / cancel-like frame was
  pre-implemented or probed; this run sent no resume frame

All probe output stayed operator-local under `/tmp/m2acp_probe_evidence/`.
No in-repo file embeds a raw transcript: the 16 KiB per-file cap plus the
per-capability 50-event cap make the full driver log inadmissible, so only
the trimmed sample file below is committed; log values are attached via
sha256 only.

## Run ledger

| field | value |
|---|---|
| driver | `python3 /tmp/m2acp_probe_evidence/probeD_driver.py` (fixed pipeline: off-by-one + missing close) |
| driver bytes / sha256 | 10284 / `95074d07d2d27f50001c42dc7cce64e77f2db421009b717a323f71a7246b787f` |
| log lines / bytes / sha256 | 995 / 113718 / `978b13f5fd2f999c9698bb5c2aff2becf640236259be31fbda31f022738396a2` |
| driver exit_code | 0 |
| probeC local output sha256 | `685ecd9f0dfe778af857bba34d1b45662abb0bdc8e4d412de906c83f2191bf3e` |
| probeC stderr (-32601 dump) sha256 | `7046a89dedddd9a30a3ba053a63ec1629338d8d12bd841d4e1622b5505676f63` |
| probeA/B initialize sha256 | `dc098ec133a4346d13023629142ded245ff670ff2257700f6b80cb272e64258d` |
| sandbox cwd (user-advised) | `/tmp/m2acp_probe_cwd` (fresh empty dir; `session/new` sends `mcpServers: []`, no project files / `.env`) |

Trim: the in-repo sample set is `.acc-evidence/r6-acp-driver-m2-samples.jsonl`
(29 lines), `sha256 3d313c68ad3a0079138e179475df4464d975beb4697277f80baba76aa5814cac`.
It retains 29 rows: 24 per-capability consequence rows + 5 first-occurrence
notify rows, each JSON-decoded from the full local stream (971 records, log
sha above).

## Capabilities (guide §4.5 fields; cumulative, first blank field inherits above)

1. **initialize → SUPPORTED**
   - command: driver primitive 1 (`initialize`)
   - executed_at: 2026-09-12T05:15:32Z (probeD log start); follow-on probes a few seconds later
   - observed: protocolVersion 1; agentInfo `qwen-code 0.23.3`; authMethods `openai`, `openai-responses`; caps: `loadSession: true`, `promptCapabilities { image, audio, embeddedContext }`, `sessionCapabilities { list, resume }`, `mcpCapabilities { sse, http }`, image cap `maxBytes 10380902`, `maxImagesPerTurn 4`
   - evidence: `samples.jsonl` lines 1-2 (request/resp)

2. **session/new + identity → SUPPORTED**
   - observed: `sessionId` = `6841a9a2-9678-40ef-8e87-05bb3ab8c5c5` (UUIDv4, fresh object), `models:2`, `modes:2`, `error: null`
   - evidence: lines 3-5

3. **bounded prompt → SUPPORTED**
   - prompt: one word, answer `OK`; stop `end_turn`; finish ~0.9 s in-pipeline; final `text_tail "OK"` (trailing `"\n\nOK`)
   - notifications across the whole run (all 971 in the full JSONL): agent_message_chunk 724, agent_thought_chunk 241, user_message_chunk 3, usage_update 2, available_commands_update 1
   - evidence: lines 6-8

4. **follow-up in same session → SUPPORTED**
   - prompt: one-word recall instruction; stop `end_turn`; `text_tail ' is "OK".'`, `followup_ok: true`
   - evidence: lines 9-11

5. **active cancel → SUPPORTED**
   - prompt: count 0..9999 (long-running), cancel issued at ts≈12.9 s (`cancel_notification_sent`), stop = `cancelled`, target markers absent, `update_type_delta` +716/+168
   - no `cancelled`-typed update in the stream (`canc2: n/a`) — cancellation surfaces only via the final stopReason
   - evidence: lines 12-15 + 29 (first `user_message_chunk` sample)

6. **session/load → SUPPORTED**
   - result keys `[configOptions, models, modes]`, status ok, `error: null`
   - evidence: lines 16-17

7. **session/list → SUPPORTED**
   - list ok; oldest session `createdAt 2026-09-12T05:13:50.153Z`; full list kept local-only
   - evidence: lines 18-19

8. **error / retry shape → UNKNOWN**
   - unknown method → JSON-RPC `-32601` `session/probe_nocat`; retry identical (`same_as_first`); stderr shows the Node-style error dump (credential/PID/home-path scrubbed) — shape recorded; a single-datum probe does not prove product-grade recoverability
   - evidence: lines 20-24

9. **auth observation → SUPPORTED**
   - endpoint auto-resolved (local vLLM OpenAI-compatible), `OPENAI_API_KEY` present in env, `OPENAI_MODEL` unset; no credential value read or copied
   - MCP server `nvidia-cuda-docs` failed to start; non-blocking
   - evidence: initialize record (line 2) + probeC stderr (local)

10. **resume → UNKNOWN / NOT_PROBED**
    - not probed this run (guide §4.3 forbids pre-naming it); no resume frame sent

## M2 gate

- `M2_LIVE_RECON = COMPLETED`
- `M2_ACP_DRIVER_READY = UNKNOWN / NOT_PASSED` — gated on: at least one real follow-up primitive, a recovered/observed session, and a verified cancel
- `M3_QWEN_WORKER_READY = IN_PROGRESS` (unchanged)
- this single live probe does not itself establish full driver readiness (guide §4.4)
