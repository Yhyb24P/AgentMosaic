# R7 CLI live submit/run/continue receipt

On candidate code `8b73657de52c4a342e85c0fa0893aafbb676c0e9`, an isolated
directory and SQLite board ran only Rust CLI commands against local
`qwen --acp`:

```text
register                         exit 0
submit task 1                    exit 0
run-acp task 1                   exit 0; completed
submit task 2                    exit 0
continue-acp task 2 from task 1  exit 0; continued
status                           exit 0; both tasks succeeded
binding task 1 / task 2          exit 0; both completed with an external reference
```

`continue-acp` reads the completed source binding from the authoritative board;
no session identifier is supplied on the CLI.  No credential, endpoint,
prompt, response, raw frame, native identifier, or temporary path is retained.
