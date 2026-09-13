# R7 CLI live ACP cancellation receipt

Code candidate: `8b73657de52c4a342e85c0fa0893aafbb676c0e9`.

An isolated temporary SQLite board registered the existing direct Qwen ACP
profile (`qwen --acp`), submitted one bounded task, and started the real Rust
`run-acp` command.  A separate Rust CLI invocation observed `running` and then
issued `cancel` against the same authoritative task.

Observed results:

```text
cancel command exit 0: cancelled task=1
run-acp exit 2: ACP worker cancelled
reopened task state: cancelled
reopened attempt state: cancelled
reopened ACP binding lifecycle: cancelled
external reference present: true
post-run Qwen ACP process: absent
```

The running CLI obtained cancellation only from the durable board state and
sent it through its own live ACP session.  No caller supplied a native session
reference.  A result racing cancellation is rejected rather than committed as
success.  No credential, endpoint, prompt, response, raw frame, native ID, or
temporary path is retained.
