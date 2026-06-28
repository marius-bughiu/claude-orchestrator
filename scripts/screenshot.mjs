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

await browser.close();
console.log("done");
