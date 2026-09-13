#!/usr/bin/env node
// Local HTTP smoke test for the AgentMosaic site.
//
// `wrangler deploy --dry-run` only validates configuration. This script boots a real
// local Worker through `wrangler dev` and talks to it over HTTP, so it is the check that
// actually covers the ASSETS binding, the 404 path, the Worker redirects and the
// trailing-slash asset routing.
//
// Node standard library only. The child process is terminated as a process group so no
// orphaned workerd is left behind.

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const port = Number(process.env.SITE_PORT || 8797);
const origin = `http://127.0.0.1:${port}`;
const READY_TIMEOUT_MS = Number(process.env.SITE_READY_TIMEOUT_MS || 120000);

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

function delay(ms) {
  return new Promise((done) => setTimeout(done, ms));
}

function wranglerBin() {
  const name = process.platform === "win32" ? "wrangler.cmd" : "wrangler";
  const local = join(root, "node_modules", ".bin", name);
  return existsSync(local) ? local : null;
}

const bin = wranglerBin();
if (!bin) {
  console.error("smoke-site: local wrangler not installed; run `npm ci` first.");
  process.exit(1);
}

console.log(`smoke-site: starting wrangler dev on ${origin}`);

const child = spawn(bin, ["dev", "--ip", "127.0.0.1", "--port", String(port)], {
  cwd: root,
  stdio: ["ignore", "pipe", "pipe"],
  detached: process.platform !== "win32",
});

let serverLog = "";
child.stdout.on("data", (chunk) => {
  serverLog += chunk.toString();
});
child.stderr.on("data", (chunk) => {
  serverLog += chunk.toString();
});

let childExited = false;
child.on("exit", () => {
  childExited = true;
});

function signalChild(signal) {
  if (childExited || child.pid === undefined) return;
  try {
    if (process.platform === "win32") child.kill(signal);
    // Negative pid targets the whole process group, which includes workerd.
    else process.kill(-child.pid, signal);
  } catch {
    /* already gone */
  }
}

async function stopChild() {
  if (childExited) return;
  signalChild("SIGTERM");
  for (let i = 0; i < 50 && !childExited; i += 1) await delay(100);
  if (!childExited) {
    signalChild("SIGKILL");
    for (let i = 0; i < 20 && !childExited; i += 1) await delay(100);
  }
}

let stopped = false;
async function cleanup() {
  if (stopped) return;
  stopped = true;
  await stopChild();
}

process.on("SIGINT", async () => {
  await cleanup();
  process.exit(130);
});
process.on("SIGTERM", async () => {
  await cleanup();
  process.exit(143);
});

async function waitForReady() {
  const deadline = Date.now() + READY_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (childExited) return false;
    try {
      await fetch(`${origin}/`, { redirect: "manual" });
      return true;
    } catch {
      await delay(500);
    }
  }
  return false;
}

function reportServerLog() {
  if (!serverLog.trim()) return;
  console.error("\n--- wrangler dev output ---");
  console.error(serverLog.trim().split("\n").slice(-40).join("\n"));
  console.error("--- end wrangler dev output ---\n");
}

const CASES = [
  { path: "/", status: 200, contains: "Run heterogeneous coding agents as one durable team" },
  { path: "/zh/", status: 200, contains: "把异构 coding Agent" },
  { path: "/styles.css", status: 200, contains: "--accent" },
  { path: "/site.js", status: 200, contains: "navigator.clipboard" },
  { path: "/favicon.svg", status: 200, contains: "<svg" },
];

const REDIRECTS = [
  {
    path: "/release",
    status: 302,
    location: /^https:\/\/github\.com\/Yhyb24P\/AgentMosaic\/releases\/latest$/,
    label: "redirects to the GitHub latest release",
  },
  {
    path: "/install.sh",
    status: 302,
    location: /^https:\/\/github\.com\/Yhyb24P\/AgentMosaic\/releases\/latest\/download\/agentmosaic-cli-installer\.sh$/,
    label: "redirects to the latest cargo-dist installer",
  },
];

let exitCode = 0;
try {
  const ready = await waitForReady();
  if (!ready) {
    console.error(`smoke-site: server did not become ready within ${READY_TIMEOUT_MS} ms`);
    reportServerLog();
    await cleanup();
    process.exit(1);
  }
  pass("wrangler dev is serving");

  console.log("\nstatic routes");
  for (const item of CASES) {
    const response = await fetch(`${origin}${item.path}`, { redirect: "manual" });
    const body = await response.text();
    check(response.status === item.status, `GET ${item.path} -> ${response.status} (want ${item.status})`);
    if (item.contains) {
      check(body.includes(item.contains), `GET ${item.path} body contains expected content`);
    }
  }

  console.log("\n404 handling");
  const missing = await fetch(`${origin}/definitely-missing`, { redirect: "manual" });
  const missingBody = await missing.text();
  check(missing.status === 404, `GET /definitely-missing -> ${missing.status} (want 404)`);
  check(
    !missingBody.includes("Run heterogeneous coding agents as one durable team"),
    "unknown path does not fall back to the homepage",
  );
  check(missingBody.includes("404"), "unknown path serves the 404 page");

  console.log("\nzh nearest 404");
  const zhMissing = await fetch(`${origin}/zh/definitely-missing`, { redirect: "manual" });
  const zhMissingBody = await zhMissing.text();
  check(zhMissing.status === 404, `GET /zh/definitely-missing -> ${zhMissing.status} (want 404)`);
  check(
    zhMissingBody.includes("页面不存在"),
    "a path under /zh/ serves the Chinese 404 page (nearest-404)",
  );

  console.log("\ntrailing-slash routing");
  const zhNoSlash = await fetch(`${origin}/zh`, { redirect: "manual" });
  // Cloudflare's auto-trailing-slash normalization answers with a 307 Temporary
  // Redirect, so accept the whole 3xx class here rather than one specific code.
  check(
    zhNoSlash.status >= 300 && zhNoSlash.status < 400,
    `GET /zh -> ${zhNoSlash.status} (want a 3xx redirect)`,
  );
  const zhLocation = zhNoSlash.headers.get("location") || "";
  check(/\/zh\/$/.test(zhLocation), `GET /zh redirects to a trailing-slash URL (${zhLocation})`);

  console.log("\nWorker redirects");
  for (const item of REDIRECTS) {
    const response = await fetch(`${origin}${item.path}`, { redirect: "manual" });
    const location = response.headers.get("location") || "";
    check(response.status === item.status, `GET ${item.path} -> ${response.status} (want ${item.status})`);
    check(item.location.test(location), `GET ${item.path} ${item.label}`);
  }

  console.log("\nsecurity headers");
  const home = await fetch(`${origin}/`, { redirect: "manual" });
  const csp = home.headers.get("content-security-policy") || "";
  check(csp.length > 0, "Content-Security-Policy applied to the homepage");
  check(csp.includes("default-src 'self'"), "CSP restricts default-src to 'self'");
  check(!csp.includes("unsafe-inline"), "CSP does not allow unsafe-inline");
  check(
    (home.headers.get("x-content-type-options") || "").toLowerCase() === "nosniff",
    "X-Content-Type-Options: nosniff",
  );
  check(
    (home.headers.get("referrer-policy") || "") === "strict-origin-when-cross-origin",
    "Referrer-Policy applied",
  );
} catch (error) {
  fail(`unexpected error: ${error && error.message ? error.message : error}`);
} finally {
  await cleanup();
}

if (failures.length > 0) {
  reportServerLog();
  console.error(`\nsmoke-site: FAILED (${failures.length} of ${checks})`);
  for (const failure of failures) console.error(`  - ${failure}`);
  exitCode = 1;
} else {
  console.log(`\n${checks}/${checks} checks passed`);
  console.log("smoke-site: PASS");
}

process.exit(exitCode);
