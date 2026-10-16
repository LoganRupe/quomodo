// Records the readme demo against a running `KEEP=1 scripts/demo.sh`:
//   1-3. a one-line change is committed and pushed, the post-receive webhook
//        runs DeployStackIfChanged, only gitea restarts. Every command shown
//        runs for real.
//   4.   the Servers page groups agents by host: root and rootless side by side.
//   5.   the Stacks page, zooming onto the shield that marks a root agent.
// Writes one video per page and marks.json to $OUT; assemble.py cuts them.
//
// Usage: PLAYWRIGHT=/path/to/node_modules/playwright/index.mjs node record.mjs
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const { chromium } = await import(process.env.PLAYWRIGHT ?? "playwright");
const HERE = path.dirname(new URL(import.meta.url).pathname);
const ROOT = path.resolve(HERE, "../..");
const OUT = process.env.OUT ?? "/tmp/quomodo-demo-rec";
const WORK = process.env.WORK ?? "/tmp/quomodo-demo";
const UI = "http://127.0.0.1:9142";
const HOMELAB = `${WORK}/homelab`;
const FROM = process.env.FROM ?? "1.22.4";
const TO = process.env.TO ?? "1.22.6";
const SIZE = { width: 1280, height: 720 };
fs.rmSync(OUT, { recursive: true, force: true });
fs.mkdirSync(OUT, { recursive: true });
for (const f of ["term.html", "slide.html"]) fs.copyFileSync(`${HERE}/${f}`, `${OUT}/${f}`);
fs.copyFileSync(`${ROOT}/ui/node_modules/@xterm/xterm/lib/xterm.js`, `${OUT}/xterm.js`);
fs.copyFileSync(`${ROOT}/ui/node_modules/@xterm/xterm/css/xterm.css`, `${OUT}/xterm.css`);
fs.copyFileSync(`${ROOT}/ui/public/quomodo-app-icon.svg`, `${OUT}/quomodo-app-icon.svg`);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sh = (cmd, cwd) =>
  execSync(cmd, { cwd, encoding: "utf8", shell: "/bin/bash", env: { ...process.env, GIT_PAGER: "cat" } });
// demo.sh's api.py, without its command line: evaluates `expr` with api() in scope.
const apiEval = (expr) =>
  JSON.parse(
    execSync("python3 -", {
      encoding: "utf8",
      input:
        `import sys, json; sys.argv = ["x", "x"]\n` +
        `exec(open("${WORK}/api.py").read().split("cmd = sys.argv[1]")[0])\n` +
        `print(json.dumps(${expr}))\n`,
    }),
  );

// The one-line change, made off camera; the camera sees git diff.
sh(`sed -i 's|gitea/gitea:${FROM}|gitea/gitea:${TO}|' gitea/gitea.container`, HOMELAB);

const browser = await chromium.launch();
const ctx = await browser.newContext({
  viewport: SIZE,
  colorScheme: "dark",
  recordVideo: { dir: OUT, size: SIZE },
});
const marks = {};
const born = {};
const now = () => Date.now() / 1000;
const mark = (page, name) => (marks[name] = { page, t: now() - born[page] });

async function newPage(name) {
  const p = await ctx.newPage();
  born[name] = now();
  return p;
}

// Captions hang off <html>, not <body>, so scene 5's zoom of <body> leaves them be.
const CAPTION_CSS = `position:fixed;left:50%;bottom:22px;transform:translateX(-50%);z-index:99999;
  background:#1f6feb;color:#fff;font:600 20px system-ui,sans-serif;padding:10px 18px;border-radius:8px;
  box-shadow:0 6px 24px #0009;white-space:nowrap`;
const caption = (p, text, extra = "") =>
  p.evaluate(
    ([text, css]) => {
      let d = document.getElementById("demo-caption");
      if (!d) {
        d = document.createElement("div");
        d.id = "demo-caption";
        document.documentElement.appendChild(d);
      }
      d.style.cssText = css;
      d.textContent = text;
    },
    [text, CAPTION_CSS + extra],
  );
const LEFT = ";left:272px;transform:none";

// Headless chromium draws no pointer: this one follows the mouse events.
const CURSOR_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 24 24"><path d="M4 2l16 10.5-7 1.2 4.2 7.6-3 1.6-4.2-7.7L4 20z" fill="#fff" stroke="#000" stroke-width="1.4" stroke-linejoin="round"/></svg>`;
const addCursor = (p) =>
  p.evaluate((svg) => {
    const c = document.createElement("div");
    c.innerHTML = svg;
    c.style.cssText =
      "position:fixed;left:-40px;top:-40px;z-index:100000;pointer-events:none;filter:drop-shadow(0 2px 3px #0008)";
    document.documentElement.appendChild(c);
    document.addEventListener(
      "mousemove",
      (e) => {
        c.style.left = e.clientX - 4 + "px";
        c.style.top = e.clientY - 2 + "px";
      },
      true,
    );
  }, CURSOR_SVG);
async function center(locator) {
  const b = await locator.boundingBox();
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
}
async function glide(p, locator, steps = 28) {
  const { x, y } = await center(locator);
  await p.mouse.move(x, y, { steps });
  return { x, y };
}

// Log in off camera.
const login = await newPage("login");
await login.goto(UI + "/login");
await login.getByRole("textbox", { name: "Username" }).fill("demo");
await login.getByRole("textbox", { name: "Password" }).fill("quomodo-demo");
await login.getByRole("button", { name: "Log In" }).click();
await login.waitForURL((u) => !u.pathname.startsWith("/login"));
const giteaId = apiEval(`[s["id"] for s in api("read", "ListStacks", {}) if s["name"] == "gitea"][0]`);

// Scene 2's page loads first so it is ready when the push lands.
const ui = await newPage("ui");
await ui.goto(`${UI}/stacks/${giteaId}`);
await ui.getByRole("tab", { name: "Services" }).click();
await ui.waitForTimeout(2000);
// Stack header, the Updates list and the units in one frame.
await ui.evaluate(() => window.scrollTo(0, 0));
await ui.waitForTimeout(1000);

async function termPage(name, title) {
  const p = await newPage(name);
  await p.goto(`file://${OUT}/term.html`);
  await p.evaluate((t) => window.setTitle(t), title);
  return p;
}
const write = (p, s) => p.evaluate((s) => window.term.write(s), s);
async function type(p, prompt, cmd) {
  await write(p, prompt);
  await sleep(300);
  for (const ch of cmd) {
    await write(p, ch);
    await sleep(22);
  }
  await sleep(250);
  await write(p, "\r\n");
}

// Scene 1: the diff, committed and pushed.
const PROMPT = "\x1b[32mdemo@laptop\x1b[0m:\x1b[34m~/homelab\x1b[0m$ ";
const s1 = await termPage("s1", "demo@laptop: ~/homelab");
await caption(s1, "1. Change one line in a quadlet file and push");
await sleep(300);
mark("s1", "s1-start");
await sleep(600);
await type(s1, PROMPT, "git diff");
await write(s1, sh("git -c color.ui=always diff", HOMELAB));
await sleep(2200);
const commitCmd = `git commit -qam "gitea: ${TO}" && git push -q`;
await type(s1, PROMPT, commitCmd);
mark("ui", "push");
sh(commitCmd + " 2>&1", HOMELAB);
await write(s1, PROMPT);
await sleep(1200);
mark("s1", "s1-end");

// Scene 2: the push webhook deploys; watch it land on the Stack page.
await caption(ui, "2. The push webhook runs DeployStackIfChanged", LEFT);
const t2 = now();
for (;;) {
  const u = apiEval(
    `[{"status": x["status"], "start": x["start_ts"]} for x in api("read", "ListUpdates", {"query": {"target.id": "${giteaId}"}})["updates"] if x["operation"] == "DeployStack"][0]`,
  );
  if (u.status === "Complete" && u.start > (t2 - 30) * 1000) break;
  if (now() - t2 > 120) throw new Error("deploy did not complete");
  await sleep(500);
}
mark("ui", "deployed");
await ui.waitForTimeout(1500);
await ui.getByText("Deploy Stack").first().click();
await ui.waitForTimeout(1200);
await caption(ui, "Only the changed unit restarts", ";top:14px;bottom:auto;left:auto;right:60px;transform:none");
mark("ui", "dialog");
await ui
  .getByText("Deploy Plan", { exact: true })
  .locator("visible=true")
  .first()
  .evaluate((e) => e.scrollIntoView({ behavior: "smooth", block: "start" }));
await ui.waitForTimeout(4000);
mark("ui", "ui-end");

// Scene 3: on atlas, everything else kept running.
const ATLAS = "[root@atlas ~]# ";
const s3 = await termPage("s3", "root@atlas");
await caption(s3, "3. Every other unit keeps its uptime");
mark("s3", "s3-start");
await sleep(400);
const ps = `podman ps --format "table {{.Names}}\\t{{.Image}}\\t{{.Status}}"`;
await type(s3, ATLAS, ps);
await write(s3, execSync(`docker exec quomodo-demo-atlas ${ps}`, { encoding: "utf8", shell: "/bin/bash" }));
await write(s3, ATLAS);
await sleep(3500);
mark("s3", "s3-end");

// Scene 4: Servers, grouped by host; hermes and minerva each run a rootless
// agent beside the root one.
const servers = await newPage("servers");
await servers.goto(`${UI}/servers`);
await servers.getByText("hermes", { exact: true }).first().waitFor();
await servers.waitForTimeout(1500);
await addCursor(servers);
await servers.mouse.move(640, 600);
await caption(servers, "4. Every host, with its root and rootless agents", LEFT);
mark("servers", "s4-start");
await servers.waitForTimeout(1200);
for (const host of ["hermes", "minerva"]) {
  const row = servers.getByText(host, { exact: true }).first();
  await glide(servers, row);
  await servers.waitForTimeout(250);
  await row.click();
  await servers.waitForTimeout(1300);
}
// Rest on a rootless agent's badge.
await glide(servers, servers.getByText("monitor", { exact: true }).first());
await servers.waitForTimeout(2600);
mark("servers", "s4-end");

// Scene 5: Stacks. Root agents carry a shield; zoom onto the one beside
// hermes, whose rootless twin hermes-monitor runs uptime-kuma.
const stacks = await newPage("stacks");
await stacks.goto(`${UI}/stacks`);
const row = stacks.locator("tr", { hasText: "vaultwarden" }).first();
await row.waitFor();
await stacks.waitForTimeout(1500);
await addCursor(stacks);
await stacks.mouse.move(900, 620);
await caption(stacks, "5. Rootless by default. Root agents carry a shield", LEFT);
mark("stacks", "s5-start");
await stacks.waitForTimeout(1600);
const shield = row.locator("svg.lucide-shield-alert").first();
const at = await glide(stacks, shield, 34);
await stacks.getByText("Root agent", { exact: true }).waitFor();
mark("stacks", "s5-hover");
await stacks.waitForTimeout(900);
// Zoom <body> so the region from (left, top) fills the frame: the hermes-monitor
// row above, the shield and its card. The pointer moves with the shield each
// step, so the card never sees the mouse leave.
const ZOOM = 2;
const card = await stacks.getByText("Root agent", { exact: true }).boundingBox();
const left = Math.min(at.x - 150, SIZE.width - SIZE.width / ZOOM);
const top = Math.min(at.y - 110, card.y + 150 - SIZE.height / ZOOM);
const origin = { x: (ZOOM * left) / (ZOOM - 1), y: (ZOOM * top) / (ZOOM - 1) };
await stacks.evaluate((o) => (document.body.style.transformOrigin = `${o.x}px ${o.y}px`), origin);
const STEPS = 36;
for (let i = 1; i <= STEPS; i++) {
  const k = i / STEPS;
  const s = 1 + (ZOOM - 1) * (k < 0.5 ? 2 * k * k : 1 - (-2 * k + 2) ** 2 / 2);
  await stacks.evaluate((s) => (document.body.style.transform = `scale(${s})`), s);
  await stacks.mouse.move(origin.x + s * (at.x - origin.x), origin.y + s * (at.y - origin.y));
  await sleep(30);
}
mark("stacks", "s5-zoomed");
await stacks.waitForTimeout(3600);
mark("stacks", "s5-end");

const videos = {};
for (const [name, p] of Object.entries({ login, ui, s1, s3, servers, stacks })) {
  await p.close();
  videos[name] = await p.video().path();
}
await ctx.close();
// The end slide, as a still for assemble.py to fade into.
const slide = await browser.newPage({ viewport: SIZE });
await slide.goto(`file://${OUT}/slide.html`);
await slide.waitForTimeout(500);
await slide.screenshot({ path: `${OUT}/slide.png` });
await browser.close();
fs.writeFileSync(OUT + "/marks.json", JSON.stringify({ marks, videos }, null, 2));
console.log(JSON.stringify({ marks, videos }, null, 2));
