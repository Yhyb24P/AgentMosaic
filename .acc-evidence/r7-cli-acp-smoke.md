# R7 CLI ACP smoke (sanitized)

Executed 2026-09-12. The normal-path CLI registered one `qwen --acp` worker,
submitted one bounded task, and invoked `run-acp` with auth method `openai`, a
180-second timeout, an isolated empty working directory, and no artifact paths.

The command completed with exit code `0` in 10.3 seconds. The only retained
observations are:

- registration returned `registered agent=qwen-worker`;
- the driver returned `completed task=1 agent=qwen-worker`;
- the authoritative SQLite board reported task 1 as `succeeded` and assigned
  to `qwen-worker`;
- the disposable SQLite file SHA-256 was
  `eef1904adabd13b8bef268304ecd7b6a345e2378bdb7ddd6c121f3799778b37a`.

No credential, endpoint, prompt body, model response, external session ID,
raw frame, PID, or absolute temporary path is retained. This is evidence that
the registered CLI ACP path can start and persist one bounded worker task; it
does not establish continuation, active cancel, recovery, a Codex runtime, or
R7/M2/M3 readiness.
