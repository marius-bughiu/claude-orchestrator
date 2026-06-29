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

/** Spawn `vite preview` and resolve once it actually serves HTTP. We poll the
 *  endpoint rather than sniff stdout — the banner's wording/buffering varies by
 *  Vite version and across `pnpm exec`, and a missed string would hang CI. */
async function startServer() {
  const proc = spawn(
    "pnpm",
    ["exec", "vite", "preview", "--port", String(port), "--strictPort"],
    { cwd: root, stdio: ["ignore", "pipe", "pipe"] },
  );
  proc.stdout.on("data", (b) => process.env.E2E_DEBUG && process.stdout.write(b));
  proc.stderr.on("data", (b) => process.env.E2E_DEBUG && process.stderr.write(b));
  let exited = null;
  proc.on("exit", (code) => { exited = code; });

  const deadline = Date.now() + 40_000;
  while (Date.now() < deadline) {
    if (exited !== null) throw new Error(`preview server exited early (code ${exited})`);
    try {
      const res = await fetch(base, { signal: AbortSignal.timeout(2000) });
      if (res.ok) return proc;
    } catch {
      // not up yet — keep polling
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  proc.kill("SIGTERM");
  throw new Error("preview server did not answer HTTP within 40s");
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
  await check("create-task modal exposes tags + dependencies", async () => {
    await goto("/#/tasks");
    await page.getByRole("button", { name: "New task" }).click();
    await seesText("Depends on");
    await page.getByPlaceholder("comma or space separated, e.g. docs, ci").fill("ci");
    await seesText("This task stays blocked until its prerequisites complete.");
  });
  await check("tag filter narrows the task list", async () => {
    await goto("/#/tasks");
    // The mock has a 'docs'-tagged task and a 'scheduled'-tagged one.
    const docsTask = "Generate API docs from the command surface";
    const otherTask = "Audit dependencies for vulnerabilities";
    await seesText(docsTask);
    await page.getByRole("button", { name: /^docs/ }).click();
    await page.waitForTimeout(200);
    await seesText(docsTask);
    assert.equal(await page.getByText(otherTask, { exact: false }).count(), 0, "non-docs task should be filtered out");
  });
  await check("command palette can launch a new task", async () => {
    await goto("/#/dashboard");
    await page.keyboard.press("Control+k");
    await page.getByRole("button", { name: /^New task/ }).click();
    await seesText("Instructions / acceptance criteria");
  });
  await check("task title is editable inline", async () => {
    await goto("/#/tasks/t1");
    const title = page.locator('input[title="Click to rename"]');
    await title.waitFor({ state: "visible" });
    assert.equal(await title.inputValue(), "Add partial-message streaming to the session view");
    await title.fill("Renamed task");
    assert.equal(await title.inputValue(), "Renamed task");
  });
  await check("settings diagnostics renders findings", async () => {
    await goto("/#/settings");
    await page.getByRole("button", { name: /Run diagnostics/ }).click();
    await seesText("writable");
    await seesText("allows codex but its CLI isn't installed");
  });
  await check("command palette runs diagnostics", async () => {
    await goto("/#/dashboard");
    await page.keyboard.press("Control+k");
    await page.getByRole("button", { name: /^Run diagnostics/ }).click();
    // Navigates to settings and auto-runs — findings appear without clicking.
    await seesText("available on PATH");
  });
  await check("tasks CSV export button is present", async () => {
    await goto("/#/tasks");
    await page.getByRole("button", { name: "CSV" }).waitFor({ state: "visible" });
    assert.equal(await page.getByRole("button", { name: "CSV" }).isEnabled(), true);
  });
  await check("session search returns matches with snippets", async () => {
    await goto("/#/search");
    await page.getByPlaceholder("Search session content…").fill("partials");
    await seesText("in transcript");
    await seesText(/matching "partials"/);
  });
  await check("create-task modal applies a template", async () => {
    await goto("/#/tasks");
    await page.getByRole("button", { name: "New task" }).click();
    await seesText("Start from a template");
    // Applying the "Bug fix" template prefills the title.
    await page.getByText("Start from a template").waitFor({ state: "visible" });
    // Scope to the modal overlay so we don't grab the page's project filter select.
    await page.locator(".fixed.z-50").getByRole("combobox").first().selectOption({ label: "Bug fix" });
    const titleVal = await page.locator('input[placeholder="Add user authentication"]').inputValue();
    assert.ok(titleVal.startsWith("Fix"), `expected title prefilled from template, got "${titleVal}"`);
  });
  await check("task detail offers a transcript export", async () => {
    await goto("/#/tasks/t1");
    await page.getByRole("button", { name: "Transcripts" }).waitFor({ state: "visible" });
  });
  await check("task detail has an editable notes field", async () => {
    await goto("/#/tasks/t1");
    await seesText("not sent to the agent");
    const notes = page.getByPlaceholder("Add context, links, or reminders for this task…");
    await notes.fill("call the user");
    assert.equal(await notes.inputValue(), "call the user");
  });
  await check("tasks view offers clear-completed", async () => {
    await goto("/#/tasks");
    // The mock has one completed and one cancelled-ish task; button shows a count.
    await page.getByRole("button", { name: /Clear completed/ }).waitFor({ state: "visible" });
  });
  await check("bulk selection exposes priority + tag actions", async () => {
    await goto("/#/tasks");
    await page.locator('tbody input[type="checkbox"]').first().check();
    await seesText("selected");
    await page.getByRole("combobox").filter({ hasText: "Set priority…" }).waitFor({ state: "visible" });
    await page.getByPlaceholder("add tag…").waitFor({ state: "visible" });
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
