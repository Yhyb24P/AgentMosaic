# Public release record — Research Agent System v0.1.0

- date: 2026-09-13
- repository: `Yhyb24P/research-agent-system` (public)
- channel: GitHub Release only (no crates.io publication)
- platform: `x86_64-unknown-linux-gnu` only
- release URL: https://github.com/Yhyb24P/research-agent-system/releases/tag/v0.1.0
- **Latest** release, **not** a prerelease, **not** a draft

## Commit chain

```text
89ac979d333fe3fc2e311fb566f3ab0056bec4c5   executable source freeze
b24bfb0                                     evidence
f1aec45                                     remote-CI evidence
f2c8af9060395252ec06d6e71d6dc54f65c2e2fc   release: prepare v0.1.0   <-- tag v0.1.0
```

`git diff --stat 89ac979 f2c8af9 -- crates Cargo.toml Cargo.lock` is empty, so the
released source contains exactly the frozen executable source. The workspace version was
already `0.1.0` with `publish = false`, so the release required no Cargo change.

## Tag

```text
v0.1.0   annotated tag object 2ca9547f96b307a244ec6a40aa3bc69b07b647ab
         -> commit f2c8af9060395252ec06d6e71d6dc54f65c2e2fc
```

The tag is annotated but **not GPG-signed**: no signing key is configured in this
environment (`git config user.signingkey` unset, no secret key in the keyring). To sign
it later:

```bash
git tag -d v0.1.0 && git push origin :refs/tags/v0.1.0
git tag -s v0.1.0 f2c8af9 -m "Research Agent System v0.1.0"
git push origin v0.1.0
```

Earlier historical tags (`v0.1.0-rc*`, `v1.0.0-rc.*`) were left untouched.

## Exact-commit remote qualification

`rust-candidate` run `34761651463` was dispatched with `ref=f2c8af9060395252ec06d6e71d6dc54f65c2e2fc`;
the Actions log shows checkout of exactly that SHA, and the run is **green** (Clean tree,
Format, Clippy, Test, Release build, Diff hygiene, Authentic historical v8 migration,
Copied-binary board smoke). The push-triggered `rust.yml` for the same commit is also green.

## Assets

```text
research-agent-system-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
  sha256 2b0e51816271f6e2d688cb6d215c2cb276bc8ebdb39abe44e96005c85479f39a  (6,557,507 bytes)
SHA256SUMS.txt
release-manifest.json
```

Tarball contents: `agent-code-cli`, `agent-code-tui`, `ras_codex_mcp`, `LICENSE`,
`README.md`, `README.zh-CN.md`, `THIRD_PARTY_LICENSES.html`. The test/mock binaries
`acp_m2_mock` and `codex_bridge_mock` are deliberately excluded.

Built from the pristine release worktree (`git worktree` at `f2c8af9`, clean) with
**rustc 1.98.1 (48a229cea 2026-09-01)** — the same version the remote qualification used.

Binary hashes:

```text
agent-code-cli ecdf3135d5063eaf79e7dfc43f074640a8ecc932219a12b34a2dc2e79d0817e2
agent-code-tui 62389c1004f62ca31a487047c121a58ee4ba52f4d2018cc7d50251921c6a6046
ras_codex_mcp  4d27ed61ab875c97dc6cdc30b8112afb574ca3ec3ac1b4ec1d825bceea8464dc
```

## Dependency license pass

Offline pass over `cargo metadata --locked`: **273** third-party crates, all declaring a
permissive license under an SPDX `OR`/`AND`/`WITH` evaluation (Apache-2.0, MIT, ISC,
BSD, Zlib, Unicode-3.0, Unlicense, MPL-2.0, CDLA-Permissive-2.0, BSL-1.0, 0BSD, CC0-1.0).
No crate was flagged for review. `THIRD_PARTY_LICENSES.html` lists every crate and embeds
the license texts shipped with it.

`cargo-about` / `cargo-deny` were not installed in this environment; the generator is
offline and deterministic and performs the same two jobs (list + texts + policy pass).

## Published-package verification (post-publish)

Downloaded the three assets back from GitHub and verified:

- `sha256sum -c SHA256SUMS.txt` -> both entries `OK`;
- downloaded tarball hash equals `release-manifest.json.asset_sha256`;
- the three shipped binaries hash-match `release-manifest.json.binaries`;
- the extracted `agent-code-cli` reports `agent-code-cli 0.1.0`;
- a **real Codex + Qwen team run using only the extracted package and the extracted
  `ras_codex_mcp`** completed with `run_team_exit=0`, `TOKEN_IN_ROOT_ANSWER=yes`,
  `TOKEN_IN_WORKER_RESULT=yes`.

## Readiness

```text
LOCAL_PRODUCT_RC_READY        = true
REMOTE_DETERMINISTIC_CI_READY = true
PUBLIC_RELEASE_READY          = true   (v0.1.0 published, Latest, non-prerelease)
```

## Deviations from the agreed plan

1. **Tag signing.** The plan called for a signed annotated tag. No GPG key exists in this
   environment, so the tag is annotated but unsigned. Re-signing commands are above.
2. **Toolchain.** The plan listed Rust 1.98.1; the local rustup channel was on 1.94.1, so
   1.98.1 was installed and the release artifacts were built with it to match the remote
   qualification rather than recording two different compilers.
3. **`docs/releases/` was gitignored** by the existing `docs/*` rule; a `docs/releases/`
   exception was added during release preparation so the release notes are tracked.
4. **Mock binaries.** `acp_m2_mock` and `codex_bridge_mock` are excluded from the tarball,
   as agreed.
5. Repository topics still include `python` and other pre-rewrite labels; the topic list
   was not changed (only the description, as agreed).
