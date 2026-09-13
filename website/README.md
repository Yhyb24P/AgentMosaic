# AgentMosaic website

Static HTML/CSS/JS deployed with Cloudflare Workers Static Assets.

## Architecture

- `src/index.ts` — the only Worker logic: two redirect endpoints. Everything else is
  handed to `env.ASSETS`.
- `public/` — the static site. `wrangler.jsonc` declares the `ASSETS` binding that makes
  `env.ASSETS.fetch()` work, plus `html_handling` and `not_found_handling`.
- No framework, no npm runtime dependency, no external font, no analytics, no cookies.

Do not re-implement asset routing or the 404 in the Worker; those are `wrangler.jsonc`
settings.

## Check

```bash
npm ci
npm run verify   # static contract: files, HTML/SEO, content, dependencies, size budget
npm run check    # verify + wrangler deploy --dry-run
npm run smoke    # boots wrangler dev and asserts real HTTP behavior
```

## Routes

```text
/             English homepage
/zh/          Chinese homepage
/install.sh   302 -> latest cargo-dist installer
/release      302 -> GitHub latest release
```

Any other path is served from `public/`; an unmatched path returns `public/404.html` with
a real 404 status.

## Deploy

```bash
npx wrangler deploy
```

Deployment is manual. No Cloudflare credential belongs in this repository or in CI.

## Content source of truth

`README.md` and `AGENTS.md` at the repository root define product truth, together with the
public `am` CLI. This site summarizes them and must not introduce a product claim those
documents do not support.
