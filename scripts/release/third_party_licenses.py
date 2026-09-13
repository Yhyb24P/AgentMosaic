#!/usr/bin/env python3
"""Generate THIRD_PARTY_LICENSES.html for the release binary tarball.

Deterministic and offline: reads `cargo metadata --locked` and the license texts
shipped inside each dependency's registry source directory. Also performs a simple
policy pass, flagging dependencies whose declared license is missing or outside a
permissive allowlist, so the release build fails loudly rather than shipping an
unreviewed license.

usage: third_party_licenses.py <cargo-manifest-dir> <output.html> [--package-name NAME]
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

# Permissive licenses we accept for a binary distribution. Anything else is flagged.
ALLOWED_LICENSES = {
    "Apache-2.0",
    "MIT",
    "MIT-0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Zlib",
    "CC0-1.0",
    "BSL-1.0",
    "0BSD",
    "MPL-2.0",
    "CDLA-Permissive-2.0",
    "Unlicense",
}

# SPDX license exceptions we accept, used as `<license> WITH <exception>`.
ALLOWED_EXCEPTIONS = {"LLVM-exception"}

LICENSE_FILE_HINTS = ("license", "licence", "copying", "notice", "unlicense")

# --- Minimal SPDX expression evaluation -------------------------------------
# Grammar: expr := term (OR term)* ; term := factor (AND factor)* ;
#          factor := '(' expr ')' | id (WITH id)?
# OR means "any of", AND means "all of". A legacy `/` separator (e.g.
# `MIT/Apache-2.0`) is treated as OR, matching how those crates are used.


def _tokenize(expr: str) -> list[str]:
    cleaned = expr.replace("(", " ( ").replace(")", " ) ").replace("/", " OR ")
    return [token for token in cleaned.split() if token]


def evaluate_spdx(expr: str) -> tuple[bool, str]:
    """Return (allowed, reason) for one SPDX license expression."""
    tokens = _tokenize(expr)
    if not tokens:
        return False, "no license declared"

    def parse_expr(pos: int) -> tuple[bool, int]:
        ok, pos = parse_term(pos)
        while pos < len(tokens) and tokens[pos] == "OR":
            rhs, pos = parse_term(pos + 1)
            ok = ok or rhs
        return ok, pos

    def parse_term(pos: int) -> tuple[bool, int]:
        ok, pos = parse_factor(pos)
        while pos < len(tokens) and tokens[pos] == "AND":
            rhs, pos = parse_factor(pos + 1)
            ok = ok and rhs
        return ok, pos

    def parse_factor(pos: int) -> tuple[bool, int]:
        if pos < len(tokens) and tokens[pos] == "(":
            ok, pos = parse_expr(pos + 1)
            if pos < len(tokens) and tokens[pos] == ")":
                pos += 1
            return ok, pos
        if pos >= len(tokens):
            return False, pos
        base = tokens[pos]
        pos += 1
        if pos < len(tokens) and tokens[pos] == "WITH":
            exception = tokens[pos + 1] if pos + 1 < len(tokens) else ""
            pos += 2
            return (
                base in ALLOWED_LICENSES and exception in ALLOWED_EXCEPTIONS,
                pos,
            )
        return base in ALLOWED_LICENSES, pos

    try:
        allowed, _ = parse_expr(0)
    except (IndexError, RecursionError):
        return False, f"could not evaluate: {expr}"
    return allowed, "" if allowed else f"no accepted alternative in '{expr}'"


def load_metadata(manifest_dir: str) -> dict:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=manifest_dir,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)


def license_texts(crate_dir: Path) -> list[tuple[str, str]]:
    found: list[tuple[str, str]] = []
    if not crate_dir.is_dir():
        return found
    for entry in sorted(crate_dir.iterdir()):
        if not entry.is_file():
            continue
        name = entry.name.lower()
        if not any(hint in name for hint in LICENSE_FILE_HINTS):
            continue
        try:
            text = entry.read_text(encoding="utf-8", errors="replace").strip()
        except OSError:
            continue
        if text:
            found.append((entry.name, text))
    return found


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    manifest_dir, output = sys.argv[1], sys.argv[2]

    meta = load_metadata(manifest_dir)
    workspace_members = set(meta["workspace_members"])
    root = Path(manifest_dir).resolve()

    crates = []
    for package in meta["packages"]:
        if package["id"] in workspace_members:
            continue
        manifest_path = Path(package["manifest_path"])
        try:
            manifest_path.relative_to(root)
            continue  # path dependency inside this repository: not third-party
        except ValueError:
            pass
        crates.append(package)

    crates.sort(key=lambda p: (p["name"], p["version"]))

    flagged = []
    rows = []
    details = []
    for package in crates:
        name, version = package["name"], package["version"]
        license_id = package.get("license") or ""
        repository = package.get("repository") or ""
        crate_dir = Path(package["manifest_path"]).parent

        accepted, reason = evaluate_spdx(license_id)
        if not accepted:
            flagged.append((name, version, reason))

        rows.append(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>".format(
                name,
                version,
                license_id or "(none declared)",
                "<span class='flag'>REVIEW</span>" if not accepted else "ok",
            )
        )

        texts = license_texts(crate_dir)
        body = []
        for filename, text in texts:
            body.append(f"<h4>{filename}</h4><pre>{escape(text)}</pre>")
        if not body:
            body.append("<p>(no license file found in the crate distribution)</p>")
        details.append(
            "<section><h3>{name} {version} — {license_id}</h3>"
            "<p class='meta'>repository: {repo}</p>{body}</section>".format(
                name=name,
                version=version,
                license_id=escape(license_id) or "(none declared)",
                repo=escape(repository) or "(none)",
                body="".join(body),
            )
        )

    html = TEMPLATE.format(
        crate_rows="".join(rows),
        crate_count=len(crates),
        details="".join(details),
        flagged=(
            "<p class='bad'><strong>Policy pass:</strong> "
            + "; ".join(f"{n} {v} [{l}]" for n, v, l in flagged)
            + "</p>"
            if flagged
            else "<p class='good'><strong>Policy pass:</strong> all "
            f"{len(crates)} third-party dependencies declare a permissive license.</p>"
        ),
    )
    Path(output).write_text(html, encoding="utf-8")

    print(f"third-party crates: {len(crates)}")
    if flagged:
        print("REVIEW REQUIRED:")
        for name, version, license_id in flagged:
            print(f"  {name} {version}: {license_id}")
        return 1
    print("policy pass: all dependencies declare a permissive license")
    return 0


def escape(text: str) -> str:
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
    )


TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Third-Party Licenses — Research Agent System v0.1.0</title>
<style>
  body {{ font-family: system-ui, sans-serif; margin: 2rem auto; max-width: 60rem; line-height: 1.5; }}
  table {{ border-collapse: collapse; width: 100%; }}
  th, td {{ border: 1px solid #ccc; padding: 0.3rem 0.5rem; text-align: left; font-size: 0.9rem; }}
  pre {{ white-space: pre-wrap; background: #f6f6f6; padding: 0.8rem; font-size: 0.85rem; }}
  .flag {{ color: #a00; font-weight: bold; }}
  .bad {{ color: #a00; font-weight: bold; }}
  .good {{ color: #060; font-weight: bold; }}
  .meta {{ color: #555; font-size: 0.85rem; }}
</style>
</head>
<body>
<h1>Third-Party Licenses</h1>
<p>Binary distribution: Research Agent System v0.1.0
(<code>x86_64-unknown-linux-gnu</code>). {crate_count} third-party Rust crates are
statically linked into or required by the shipped binaries. Full license texts follow
the summary table.</p>
{flagged}
<h2>Summary</h2>
<table>
<thead><tr><th>Crate</th><th>Version</th><th>License</th><th>Status</th></tr></thead>
<tbody>{crate_rows}</tbody>
</table>
<h2>License texts</h2>
{details}
<h2>Project license</h2>
<p>Research Agent System itself is licensed under Apache-2.0; see <code>LICENSE</code>
in the distribution.</p>
</body>
</html>
"""


if __name__ == "__main__":
    raise SystemExit(main())
