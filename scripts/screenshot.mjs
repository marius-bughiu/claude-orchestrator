import { chromium } from "playwright";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const mock = readFileSync(join(__dirname, "screenshot-mock.js"), "utf8");
const base = process.env.BASE || "http://localhost:4173";
const outDir = process.env.OUT || "/tmp/shots";

const routes = [
  ["dashboard", "/#/dashboard"],
  ["projects", "/#/projects"],
  ["project-detail", "/#/projects/p1"],
  ["tasks", "/#/tasks"],
  ["task-detail", "/#/tasks/t3"],
  ["board", "/#/board"],
  ["graph", "/#/graph"],
  ["pulls", "/#/pulls"],
  ["scheduled", "/#/scheduled"],
  ["timeline", "/#/timeline"],
  ["settings", "/#/settings"],
  ["session-detail", "/#/sessions/s1"],
];

const browser = await chromium.launch({
  executablePath: process.env.PW_CHROMIUM || undefined,
});

async function capture(theme, names) {
  const ctx = await browser.newContext({
    viewport: { width: 1320, height: 860 },
    deviceScaleFactor: 2,
    colorScheme: theme === "light" ? "light" : "dark",
  });
  await ctx.addInitScript(mock);
  if (theme === "light") {
    await ctx.addInitScript(() => localStorage.setItem("orchestrator.theme", "light"));
  }
  const page = await ctx.newPage();
  for (const [name, route] of routes) {
    if (names && !names.includes(name)) continue;
    await page.goto(base + route, { waitUntil: "networkidle" });
    await page.waitForTimeout(800);
    const suffix = theme === "light" ? "-light" : "";
    await page.screenshot({ path: join(outDir, `${name}${suffix}.png`) });
    console.log("shot:", `${name}${suffix}`);
  }
  await ctx.close();
}

await capture("dark");
await capture("light", ["dashboard", "projects"]);

// Tasks with a bulk selection.
{
  const ctx = await browser.newContext({ viewport: { width: 1320, height: 860 }, deviceScaleFactor: 2, colorScheme: "dark" });
  await ctx.addInitScript(mock);
  const page = await ctx.newPage();
  await page.goto(base + "/#/tasks", { waitUntil: "networkidle" });
  await page.waitForTimeout(700);
  const boxes = page.locator('tbody input[type="checkbox"]');
  await boxes.nth(0).check();
  await boxes.nth(1).check();
  await page.waitForTimeout(200);
  await page.screenshot({ path: join(outDir, "tasks-bulk.png") });
  console.log("shot: tasks-bulk");
  await ctx.close();
}

// Command palette (Cmd/Ctrl+K).
{
  const ctx = await browser.newContext({ viewport: { width: 1320, height: 860 }, deviceScaleFactor: 2, colorScheme: "dark" });
  await ctx.addInitScript(mock);
  const page = await ctx.newPage();
  await page.goto(base + "/#/dashboard", { waitUntil: "networkidle" });
  await page.waitForTimeout(600);
  await page.keyboard.press("Control+k");
  await page.waitForTimeout(400);
  await page.screenshot({ path: join(outDir, "command-palette.png") });
  console.log("shot: command-palette");
  await ctx.close();
}

// First-run onboarding overlay (no projects yet).
{
  const ctx = await browser.newContext({ viewport: { width: 1320, height: 860 }, deviceScaleFactor: 2, colorScheme: "dark" });
  await ctx.addInitScript(mock);
  await ctx.addInitScript(() => {
    localStorage.removeItem("orchestrator.onboarded");
    window.__SHOT_MOCK__.projects = [];
  });
  const page = await ctx.newPage();
  await page.goto(base + "/#/dashboard", { waitUntil: "networkidle" });
  await page.waitForTimeout(600);
  await page.screenshot({ path: join(outDir, "onboarding-welcome.png") });
  await page.getByText("Get started").click();
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(outDir, "onboarding-agents.png") });
  console.log("shot: onboarding");
  await ctx.close();
}

// Session detail with the Changes (diff) panel expanded.
{
  const ctx = await browser.newContext({ viewport: { width: 1320, height: 980 }, deviceScaleFactor: 2, colorScheme: "dark" });
  await ctx.addInitScript(mock);
  const page = await ctx.newPage();
  await page.goto(base + "/#/sessions/s1", { waitUntil: "networkidle" });
  await page.waitForTimeout(500);
  await page.getByText("Changes", { exact: false }).first().click();
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(outDir, "session-diff.png") });
  console.log("shot: session-diff");
  await ctx.close();
}

await browser.close();
console.log("done");
