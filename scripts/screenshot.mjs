import { chromium } from "playwright";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const mock = readFileSync(join(__dirname, "screenshot-mock.js"), "utf8");
const base = process.env.BASE || "http://localhost:4173";
const outDir = process.env.OUT || "/tmp/shots";

const routes = [
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
const ctx = await browser.newContext({
  viewport: { width: 1320, height: 860 },
  deviceScaleFactor: 2,
  colorScheme: "dark",
});
await ctx.addInitScript(mock);
const page = await ctx.newPage();

for (const [name, route] of routes) {
  await page.goto(base + route, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);
  await page.screenshot({ path: join(outDir, `${name}.png`) });
  console.log("shot:", name);
}

await browser.close();
console.log("done");
