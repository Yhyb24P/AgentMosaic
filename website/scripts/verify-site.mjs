#!/usr/bin/env node
// Static contract verification for the AgentMosaic site.
//
// Node standard library only: this runs credential-free in CI and must not depend on
// wrangler, a browser, or a network. It checks the files, the HTML/SEO contract, the
// product-content contract, forbidden claims, the no-external-dependency contract and
// the size budget. Anything it cannot check statically is covered by smoke-site.mjs.

import { readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const EN_PAGE = "public/index.html";
const ZH_PAGE = "public/zh/index.html";

let checks = 0;
const failures = [];

function pass(message) {
  checks += 1;
  console.log(`  ok    ${message}`);
}

function fail(message) {
  checks += 1;
  failures.push(message);
  console.log(`  FAIL  ${message}`);
}

function check(condition, message) {
  if (condition) pass(message);
  else fail(message);
}

function section(title) {
  console.log(`\n${title}`);
}

function read(relative) {
  return readFileSync(join(root, relative), "utf8");
}

function bytes(relative) {
  return statSync(join(root, relative)).size;
}

function matchAll(source, pattern) {
  const out = [];
  const re = new RegExp(pattern.source, pattern.flags.includes("g") ? pattern.flags : `${pattern.flags}g`);
  let m;
  while ((m = re.exec(source)) !== null) out.push(m);
  return out;
}

// --------------------------------------------------------------- files

section("files");
const REQUIRED_FILES = [
  "public/index.html",
  "public/zh/index.html",
  "public/styles.css",
  "public/site.js",
  "public/favicon.svg",
  "public/404.html",
  "public/zh/404.html",
  "public/robots.txt",
  "public/sitemap.xml",
  "public/_headers",
];
for (const file of REQUIRED_FILES) {
  let exists = true;
  try {
    statSync(join(root, file));
  } catch {
    exists = false;
  }
  check(exists, `exists: ${file}`);
}

// -------------------------------------------------------- HTML contract

const pages = [
  { name: "English", path: EN_PAGE, lang: "en", html: read(EN_PAGE) },
  { name: "Chinese", path: ZH_PAGE, lang: "zh-CN", html: read(ZH_PAGE) },
];

const HTML_CONTRACT = [
  [/^<!doctype html>/i, "doctype"],
  [/<html\b[^>]*\blang=/i, "lang attribute"],
  [/<title>[^<]+<\/title>/i, "title"],
  [/<meta\b[^>]*name="description"[^>]*content="[^"]+"/i, "meta description"],
  [/<meta\b[^>]*name="viewport"[^>]*content="[^"]+"/i, "viewport"],
  [/<link\b[^>]*rel="canonical"[^>]*href="https:\/\/am\.yhshyp\.xyz/i, "canonical"],
  [/<header\b/i, "header"],
  [/<nav\b/i, "nav"],
  [/<main\b[^>]*\bid="main"/i, "main#main"],
  [/<footer\b/i, "footer"],
  [/<meta\b[^>]*name="theme-color"[^>]*content="[^"]+"/i, "theme-color"],
  [/<link\b[^>]*rel="icon"[^>]*href="\/favicon\.svg"/i, "favicon"],
  [/<meta\b[^>]*property="og:title"[^>]*content="[^"]+"/i, "og:title"],
  [/<meta\b[^>]*property="og:description"[^>]*content="[^"]+"/i, "og:description"],
  [/<meta\b[^>]*property="og:type"[^>]*content="[^"]+"/i, "og:type"],
  [/<meta\b[^>]*property="og:url"[^>]*content="[^"]+"/i, "og:url"],
  [/<meta\b[^>]*property="og:site_name"[^>]*content="[^"]+"/i, "og:site_name"],
  [/<meta\b[^>]*property="og:locale"[^>]*content="[^"]+"/i, "og:locale"],
  [/<a\b[^>]*class="skip-link"[^>]*href="#main"/i, "skip link"],
];

for (const page of pages) {
  section(`HTML contract: ${page.name} (${page.path})`);
  check(new RegExp(`<html\\b[^>]*\\blang="${page.lang}"`, "i").test(page.html), `lang="${page.lang}"`);
  for (const [pattern, label] of HTML_CONTRACT) {
    check(pattern.test(page.html), label);
  }

  const h1s = matchAll(page.html, /<h1[\s>]/i);
  check(h1s.length === 1, `exactly one h1 (found ${h1s.length})`);

  const headings = matchAll(page.html, /<h([1-6])[\s>]/i).map((m) => Number(m[1]));
  let coherent = headings.length > 0 && headings[0] === 1;
  for (let i = 1; i < headings.length; i += 1) {
    if (headings[i] > headings[i - 1] + 1) coherent = false;
  }
  check(coherent, `heading levels coherent (${headings.join(",")})`);

  const hreflangs = matchAll(page.html, /<link\b[^>]*rel="alternate"[^>]*hreflang="([^"]+)"/i).map((m) => m[1]);
  for (const value of ["en", "zh-CN", "x-default"]) {
    check(hreflangs.includes(value), `hreflang ${value}`);
  }

  check(
    !/<meta\b[^>]*name="keywords"/i.test(page.html),
    "no meta keywords",
  );
  check(!/<meta\b[^>]*property="og:image"/i.test(page.html), "no og:image while no asset exists");
}

// ----------------------------------------------------- content contract

const CONTENT_CONTRACT = [
  ["https://am.yhshyp.xyz/install.sh", "install command"],
  ["am init", "am init"],
  ["--role reasoner", "reasoner registration"],
  ["--adapter codex-app-server", "codex-app-server adapter"],
  ["--role worker", "worker registration"],
  ["--adapter acp", "acp adapter"],
  ["am doctor", "am doctor"],
  ["am run ", "am run"],
  ["Linux x86_64", "Linux x86_64 limitation"],
];

for (const page of pages) {
  section(`content contract: ${page.name}`);
  for (const [needle, label] of CONTENT_CONTRACT) {
    check(page.html.includes(needle), label);
  }
}

section("content contract: homepage only");
for (const page of pages) {
  for (const forbidden of ["register <", "run-team", "resume-team", "submit ", "auth_method", "mcp_command"]) {
    check(!page.html.includes(forbidden), `no "${forbidden}" on ${page.name} homepage`);
  }
}

// ------------------------------------------------------ forbidden claims

section("forbidden claims");
// Scoped to the public pages only. Historical documents elsewhere in the repository
// are allowed to discuss the retired product lines.
const FORBIDDEN_CLAIMS = [
  [/\bA2A\b/i, "A2A"],
  [/\bPython\b/i, "Python"],
  [/production[-\s]?ready/i, "production-ready"],
  [/enterprise[-\s]?grade/i, "enterprise-grade"],
  [/\bWindows\b/i, "Windows"],
  [/\bmacOS\b/i, "macOS"],
];
for (const page of pages) {
  for (const [pattern, label] of FORBIDDEN_CLAIMS) {
    check(!pattern.test(page.html), `no "${label}" claim on ${page.name}`);
  }
}

// --------------------------------------------------- dependency contract

section("dependency contract");
for (const page of pages) {
  const stylesheets = matchAll(page.html, /<link\b[^>]*rel="stylesheet"[^>]*>/i);
  check(stylesheets.length > 0, `${page.name} loads a stylesheet`);
  for (const [tag] of stylesheets) {
    const href = (tag.match(/href="([^"]*)"/i) || [])[1] || "";
    check(href.startsWith("/"), `stylesheet is site-relative (${href})`);
  }

  const scripts = matchAll(page.html, /<script\b[^>]*>/i);
  for (const [tag] of scripts) {
    const src = (tag.match(/src="([^"]*)"/i) || [])[1];
    check(Boolean(src), `script has src (no inline script): ${tag.trim()}`);
    if (src) check(src.startsWith("/"), `script is site-relative (${src})`);
  }

  check(matchAll(page.html, /<style[\s>]/i).length === 0, `${page.name} has no inline <style>`);
  check(matchAll(page.html, /\sstyle="/i).length === 0, `${page.name} has no style attribute`);
  check(matchAll(page.html, /\son[a-z]+\s*=/i).length === 0, `${page.name} has no inline event handler`);
  check(!/fonts\.googleapis|fonts\.gstatic/i.test(page.html), `${page.name} has no Google Fonts`);
  check(!/https?:\/\/[^"]*\.js["']/i.test(page.html), `${page.name} loads no CDN JavaScript`);

  const copyButtons = matchAll(page.html, /<button\b[^>]*data-copy="[^"]+"[^>]*>/i);
  check(copyButtons.length > 0, `${page.name} has copy buttons`);
  check(/aria-live="polite"/i.test(page.html), `${page.name} has an aria-live status region`);
}

section("stylesheet contract");
const css = read("public/styles.css");
check(!/@import\b/i.test(css), "no @import");
check(!/@font-face\b/i.test(css), "no @font-face");
check(!/fonts\.googleapis|fonts\.gstatic/i.test(css), "no Google Fonts reference");
check(!/outline\s*:\s*none/i.test(css), "does not remove focus outline");
check(/:focus-visible/i.test(css), "defines a :focus-visible style");
check(/prefers-reduced-motion/i.test(css), "respects prefers-reduced-motion");

section("404 contract");
// Both 404 pages must stay script-free and must not pretend to be the homepage.
const NOT_FOUND_PAGES = [
  { file: "public/404.html", home: /href="\/"/i, label: "root" },
  { file: "public/zh/404.html", home: /href="\/zh\/"/i, label: "Chinese" },
];
for (const page of NOT_FOUND_PAGES) {
  const notFound = read(page.file);
  check(/\b404\b/.test(notFound), `${page.label} 404 page mentions 404`);
  check(!/<script\b/i.test(notFound), `${page.label} 404 page loads no JavaScript`);
  check(page.home.test(notFound), `${page.label} 404 page links back to its homepage`);
}

section("headers contract");
const headers = read("public/_headers");
check(/Content-Security-Policy:/i.test(headers), "Content-Security-Policy");
check(/Referrer-Policy:/i.test(headers), "Referrer-Policy");
check(/X-Content-Type-Options:\s*nosniff/i.test(headers), "X-Content-Type-Options");
check(/Permissions-Policy:/i.test(headers), "Permissions-Policy");
check(!/'unsafe-inline'/i.test(headers), "CSP has no 'unsafe-inline'");
check(!/'unsafe-eval'/i.test(headers), "CSP has no 'unsafe-eval'");

section("robots and sitemap");
check(/Sitemap:\s*https:\/\/am\.yhshyp\.xyz\/sitemap\.xml/i.test(read("public/robots.txt")), "robots.txt points at the sitemap");
const sitemap = read("public/sitemap.xml");
check(sitemap.includes("<loc>https://am.yhshyp.xyz/</loc>"), "sitemap lists /");
check(sitemap.includes("<loc>https://am.yhshyp.xyz/zh/</loc>"), "sitemap lists /zh/");

// ------------------------------------------------------------ size budget

section("size budget");
const KIB = 1024;
const BUDGET = [
  [EN_PAGE, 32 * KIB],
  [ZH_PAGE, 32 * KIB],
  ["public/styles.css", 24 * KIB],
  ["public/site.js", 8 * KIB],
];
let payload = 0;
for (const [file, limit] of BUDGET) {
  const size = bytes(file);
  if (["public/index.html", "public/zh/index.html", "public/styles.css", "public/site.js"].includes(file)) {
    payload += size;
  }
  check(size <= limit, `${file} ${size} B <= ${limit} B`);
}
payload += bytes("public/favicon.svg");
const payloadLimit = 100 * KIB;
check(payload <= payloadLimit, `critical first-party payload ${payload} B <= ${payloadLimit} B`);

// ------------------------------------------------------------------ result

console.log(`\n${checks - failures.length}/${checks} checks passed`);
if (failures.length > 0) {
  console.error(`\nverify-site: FAILED (${failures.length})`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("verify-site: PASS");
