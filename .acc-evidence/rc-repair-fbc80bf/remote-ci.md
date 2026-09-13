# Remote deterministic CI on the frozen candidate

- date: 2026-09-13
- repository: `Yhyb24P/research-agent-system`
- branch: `v2/rust-agent-team`
- frozen candidate (executable source): `89ac979d333fe3fc2e311fb566f3ab0056bec4c5`
- pushed HEAD at the time of these runs: `b24bfb0` (docs/evidence only relative to the
  candidate: `git diff --stat 89ac979 b24bfb0 -- crates Cargo.toml Cargo.lock` is empty)
- credentials used by CI: none; both workflows are deterministic and credential-free.
  Real authenticated Codex/Qwen E2E is a local reference-profile gate and is not run in CI.

## `rust.yml` (push trigger)

- run `34759201679`
- result: **success** in 2m55s
- steps: Format, Clippy, Test, Release build, Diff hygiene — all pass

## `rust-candidate` / `candidate.yml` (workflow_dispatch)

- run `34759220090`
- result: **success** in 4m28s
- steps: Clean tree, Format, Clippy, Test, Release build, Diff hygiene,
  Authentic historical v8 migration, Copied-binary board smoke — all pass

## Earlier run (superseded candidate)

The first candidate `295c96a` passed `rust.yml` (`34758810650`) but failed the
`rust-candidate` Test step (`34758964050`) on `ubuntu-22.04` at
`crates/agent-code-workspace/tests/integration.rs::n07_search_bounded`, revealing the
directory-enumeration-order defect in `search_dir`. That defect was repaired in
`89ac979`, after which both workflows are green. This is recorded because it is the
reason the frozen candidate is not the first one.

## Readiness consequence

```text
LOCAL_PRODUCT_RC_READY        = true
REMOTE_DETERMINISTIC_CI_READY = true   (both workflows green on the frozen candidate)
PUBLIC_RELEASE_READY          = false  (no tag, no GitHub Release, not authorized)
```

No git tag and no GitHub Release was created or pushed.
