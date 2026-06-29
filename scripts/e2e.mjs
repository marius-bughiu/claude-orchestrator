// Headless end-to-end smoke suite for the React frontend.
//
// Boots the production bundle under `vite preview`, injects the same mocked
// Tauri IPC the screenshot harness uses (scripts/screenshot-mock.js), then
// drives the real UI in Chromium and asserts each route renders representative
// data and the core interactions work. Any uncaught page exception fails the
// run, so this catches the TS/store/render regressions that `pnpm build`
// (a typecheck only) cannot.
//
// Usage:  pnpm e2e            (builds nothing — run `pnpm build` first)
//         PW_CHROMIUM=/path/to/chrome pnpm e2e
import { chromium } from "playwright";
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");
const mock = readFileSync(join(__dirname, "screenshot-mock.js"), "utf8");
const port = Number(process.env.PORT || 4178);
const base = `http://localhost:${port}`;

/** Spawn `vite preview` and resolve once it answers, or reject on timeout. */
function startServer() {
  const proc = spawn(
    "pnpm",
    ["exec", "vite", "preview", "--port", String(port), "--strictPort"],
    { cwd: root, stdio: ["ignore", "pipe", "pipe"] },
  );
  return new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error("preview server did not start in 30s")), 30_000);
    const onData = (b) => {
      if (b.toString().includes(`localhost:${port}`)) {
        clearTimeout(deadline);
        proc.stdout.off("data", onData);
        resolve(proc);
      }
    };
    proc.stdout.on("data", onData);
    proc.stderr.on("data", (b) => process.env.E2E_DEBUG && process.stderr.write(b));
    proc.on("exit", (code) => reject(new Error(`preview server exited early (code ${code})`)));
  });
}

let passed = 0;
const failures = [];
async function check(name, fn) {
  try {
    await fn();
    passed++;
    console.log(`  ✓ ${name}`);
  } catch (err) {
    failures.push({ name, err });
    console.log(`  ✗ ${name}\n      ${err.message.split("\n")[0]}`);
  }
}

const server = await startServer();
const browser = await chromium.launch({ executablePath: process.env.PW_CHROMIUM || undefined });
const ctx = await browser.newContext({ viewport: { width: 1320, height: 900 } });
await ctx.addInitScript(mock);
const page = await ctx.newPage();

// Any uncaught exception in the app is a hard failure.
const pageErrors = [];
page.on("pageerror", (e) => pageErrors.push(e));

/** Navigate to a hash route and wait for the app shell to settle. Routes are
 *  hash-based, so going to the same `/#/...` twice would NOT reload the SPA and
 *  React state (open overlays, etc.) would leak between checks — bounce through
 *  about:blank to guarantee a fresh document each time. */
async function goto(route) {
  await page.goto("about:blank");
  await page.goto(base + route, { waitUntil: "networkidle" });
  await page.waitForTimeout(300);
}
/** Assert visible text is present somewhere on the page. */
async function seesText(text) {
  await page.getByText(text, { exact: false }).first().waitFor({ state: "visible", timeout: 5000 });
}

try {
  // --- Each route renders its representative, IPC-fed content. ---
  await check("dashboard renders usage stats + console", async () => {
    await goto("/#/dashboard");
    await seesText("Total cost");
    await seesText("Console");
  });
  await check("projects list shows all mocked projects", async () => {
    await goto("/#/projects");
    await seesText("claude-orchestrator");
    await seesText("web-dashboard");
    await seesText("api-gateway");
  });
  await check("project detail shows settings incl. MCP config", async () => {
    await goto("/#/projects/p1");
    await seesText("Roadmap loop");
    await seesText("MCP config");
  });
  await check("tasks view lists tasks", async () => {
    await goto("/#/tasks");
    await seesText("Persist usage windows across restarts");
  });
  await check("board view renders columns", async () => {
    await goto("/#/board");
    await seesText("In progress");
  });
  await check("graph view renders", async () => {
    await goto("/#/graph");
    await seesText("Audit dependencies for vulnerabilities");
  });
  await check("pull requests view lists PRs", async () => {
    await goto("/#/pulls");
    await seesText("Add partial-message streaming to the session view");
  });
  await check("scheduled view lists scheduled tasks", async () => {
    await goto("/#/scheduled");
    await seesText("Weekly dependency audit");
  });
  await check("timeline view renders sessions", async () => {
    await goto("/#/timeline");
    await seesText("claude-orchestrator");
  });
  await check("activity view renders the feed", async () => {
    await goto("/#/activity");
    await seesText("Merged PR #42");
  });
  await check("settings view renders incl. scheduled backups", async () => {
    await goto("/#/settings");
    await seesText("Scheduled backups");
  });
  await check("session detail renders the transcript", async () => {
    await goto("/#/sessions/s1");
    await seesText("orchestrator/add-streaming-1a2b3c4d");
  });

  // --- Interactions. ---
  await check("dashboard granularity toggle re-renders", async () => {
    await goto("/#/dashboard");
    await page.getByRole("button", { name: "Monthly" }).click();
    await page.waitForTimeout(300);
    await seesText("Total cost");
  });
  await check("log console expands to show empty state", async () => {
    await goto("/#/dashboard");
    await page.getByText("Console", { exact: false }).first().click();
    await seesText("No log output yet.");
  });
  await check("command palette opens on Ctrl+K", async () => {
    await goto("/#/dashboard");
    await page.keyboard.press("Control+k");
    await page.waitForTimeout(300);
    await seesText("Dashboard");
  });
  await check("sidebar navigation switches views", async () => {
    await goto("/#/dashboard");
    await page.getByRole("link", { name: "Settings" }).click();
    await seesText("Scheduled backups");
  });
  await check("MCP config input is editable", async () => {
    await goto("/#/projects/p1");
    const input = page.getByPlaceholder("e.g. .mcp.json or /abs/path/mcp.json");
    await input.fill(".mcp.json");
    assert.equal(await input.inputValue(), ".mcp.json");
  });

  await check("no uncaught page exceptions across the run", async () => {
    assert.equal(pageErrors.length, 0, `page errors:\n${pageErrors.map((e) => e.stack || e.message).join("\n---\n")}`);
  });
} finally {
  await browser.close();
  server.kill("SIGTERM");
}

console.log(`\n${passed} passed, ${failures.length} failed`);
if (failures.length > 0) process.exit(1);
