/* Optional end-to-end check. Requires Playwright, Chromium, Python 3, and a built Rust binary.
   PLAYWRIGHT_MODULE=/path/to/playwright CHROMIUM_PATH=/usr/bin/chromium node tests/browser-smoke.cjs
   No npm packages are required by the application itself. */
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || "playwright");
const { spawn } = require("node:child_process");
const { mkdtemp, rm, mkdir, writeFile } = require("node:fs/promises");
const { tmpdir } = require("node:os");
const { join, resolve } = require("node:path");
const net = require("node:net");
const http = require("node:http");
const assert = require("node:assert/strict");

async function freePort() {
  return new Promise((resolve) => {
    const socket = net.createServer();
    socket.listen(0, "127.0.0.1", () => {
      const port = socket.address().port;
      socket.close(() => resolve(port));
    });
  });
}
(async () => {
  const directory = await mkdtemp(join(tmpdir(), "void-browser-"));
  const appPort = await freePort(),
    gamePort = await freePort();
  const telemetryToken = "test-only-telemetry-token-32-characters";
  const telemetryStarted = Date.now();
  const viewer = http.createServer((request, response) => {
    if (request.url === "/v1/telemetry") {
      if (request.headers.authorization !== `Bearer ${telemetryToken}`) {
        response.writeHead(401);
        response.end();
        return;
      }
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(
        JSON.stringify({
          heap_used: 128 * 1024 ** 2,
          heap_max: 512 * 1024 ** 2,
          uptime_seconds:
            3600 + Math.floor((Date.now() - telemetryStarted) / 1000),
          sampled_at_ms: Date.now(),
          players: [
            {
              name: "Alex",
              uuid: "ec561538-f3fd-461d-aff5-086b22154bce",
              ping_ms: 42,
            },
          ],
        }),
      );
      return;
    }
    response.writeHead(200, { "Content-Type": "text/html" });
    response.end("<p>Test plugin viewer</p>");
  });
  await new Promise((resolve) => viewer.listen(0, "127.0.0.1", resolve));
  const viewerUrl = `http://127.0.0.1:${viewer.address().port}/`;
  const pluginDirectory = join(
    directory,
    "minecraft",
    "plugins",
    "void-dash-bridge",
  );
  await mkdir(pluginDirectory, { recursive: true });
  await writeFile(
    join(pluginDirectory, "config.yml"),
    `bind: 127.0.0.1\nport: ${viewer.address().port}\ntoken: '${telemetryToken}'\n`,
  );
  const application = spawn(
    process.env.VOID_DASH_BINARY || resolve("target/debug/void-dash"),
    [
      "--no-open",
      "--port",
      String(appPort),
      "--config",
      join(directory, "config.json"),
    ],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  let startup = "";
  application.stdout.on("data", (chunk) => (startup += chunk));
  application.stderr.on("data", (chunk) => (startup += chunk));
  const browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_PATH || "/usr/bin/chromium",
    headless: true,
    args: ["--no-sandbox"],
  });
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1080 },
  });
  const errors = [];
  let externalServer;
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  // Block external avatar requests to verify the built-in skin fallback without external networking.
  await page.route("https://mc-heads.net/**", (route) =>
    route.fulfill({
      status: 200,
      contentType: "image/png",
      body: Buffer.from("invalid image"),
    }),
  );
  const checks = [];
  async function waitForApp() {
    for (let i = 0; i < 100; i++) {
      try {
        const result = await fetch(`http://127.0.0.1:${appPort}`);
        if (result.ok) return;
      } catch {}
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error(`App did not start: ${startup}`);
  }
  try {
    await waitForApp();
    await mkdir("data/screenshots", { recursive: true });
    await page.goto(`http://127.0.0.1:${appPort}`);
    await page.waitForSelector(".log-line");
    assert.equal(await page.locator(".server-tab").count(), 3);
    assert.equal(await page.locator(".brand,.rail,.page-heading").count(), 0);
    assert.equal(
      await page
        .getByText("Built for the worlds worth keeping.", { exact: false })
        .count(),
      0,
    );
    assert.ok(
      await page
        .locator("#memory-total")
        .textContent()
        .then((s) => s.includes("4.00 GB max")),
    );
    assert.ok(
      await page
        .locator("#host-memory")
        .textContent()
        .then((s) => s.includes("16.00 GB")),
    );
    await page.screenshot({
      path: "data/screenshots/desktop.png",
      fullPage: true,
    });
    await page.getByRole("button", { name: "creative", exact: true }).click();
    assert.equal(await page.locator("#server-name").textContent(), "creative");
    await page.getByRole("button", { name: "evergreen", exact: true }).click();
    await page.locator("#command-input").fill("whi");
    await page.keyboard.press("Tab");
    assert.equal(
      await page.locator("#command-input").inputValue(),
      "whitelist ",
    );
    await page.locator("#command-input").fill("list");
    await page.keyboard.press("Enter");
    await page
      .getByText("Preview: 12 players online", { exact: false })
      .waitFor();
    await page.locator("#log-filter").fill("squaremap");
    assert.equal(await page.locator(".log-line").count(), 2);
    await page.locator("#log-filter").fill("");
    checks.push(
      "Preview tabs, server commands, Tab completion, console filtering",
    );
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    );
    await page.screenshot({
      path: "data/screenshots/mobile.png",
      fullPage: true,
    });
    checks.push("390px layout has no horizontal overflow");
    await page.setViewportSize({ width: 1440, height: 1080 });
    // Reproduce an already-running server started in a separate .bat/terminal.
    externalServer = spawn(process.env.PYTHON || "python3", [
      "-u",
      resolve("tests/fixture_server.py"),
      "--port",
      String(gamePort),
      "--directory",
      join(directory, "minecraft"),
    ]);
    let externalOutput = "";
    externalServer.stdout.on("data", (chunk) => (externalOutput += chunk));
    externalServer.stderr.on("data", (chunk) => (externalOutput += chunk));
    for (let i = 0; i < 100 && !externalOutput.includes("Fixture ready"); i++) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    assert.ok(externalOutput.includes("Fixture ready"), externalOutput);
    await page.locator("#add-server").click();
    await page.locator("[name=name]").fill("Browser test world");
    await page.locator("[name=port]").fill(String(gamePort));
    await page.locator("[name=directory]").fill(join(directory, "minecraft"));
    await page
      .locator("[name=executable]")
      .fill(process.env.PYTHON || "python3");
    await page
      .locator("[name=args]")
      .fill(
        JSON.stringify([
          "-u",
          resolve("tests/fixture_server.py"),
          "--port",
          String(gamePort),
          "--directory",
          join(directory, "minecraft"),
        ]),
      );
    await page.locator("#save-server").click();
    await page.waitForSelector("#server-dialog", { state: "hidden" });
    assert.equal(await page.locator("#demo-banner").isVisible(), false);
    assert.equal(
      await page.locator("#server-name").textContent(),
      "Browser test world",
    );
    await page
      .locator("#management-notice")
      .waitFor({ state: "visible", timeout: 15000 });
    assert.equal(await page.locator("#restart-button").isDisabled(), true);
    assert.equal(await page.locator("#stop-button").isDisabled(), true);
    const inventory = await (
      await page.request.get(`http://127.0.0.1:${appPort}/api/servers`)
    ).json();
    assert.equal(JSON.stringify(inventory).includes(telemetryToken), false);
    const response = await page.request.post(
      `http://127.0.0.1:${appPort}/api/servers/${inventory.servers[0].id}/action/restart`,
    );
    const refusedRestart = {
      status: response.status(),
      body: await response.json(),
    };
    assert.equal(refusedRestart.status, 400);
    assert.ok(
      refusedRestart.body.error.includes(
        "not started by this void.dash session",
      ),
    );
    assert.ok(
      refusedRestart.body.error.includes(
        "Local process control does not require RCON",
      ),
    );
    const externalExit = new Promise((resolve) =>
      externalServer.once("exit", resolve),
    );
    externalServer.stdin.write("stop\n");
    await externalExit;
    await page.waitForFunction(
      () => !document.querySelector("#start-button").disabled,
      {},
      { timeout: 15000 },
    );
    assert.equal(await page.locator("#management-notice").isVisible(), false);
    checks.push(
      "External local server has clear ownership notice, disabled Restart, and an actionable API error; taking control needs no RCON",
    );
    await page.locator("#start-button").click();
    await page
      .getByText("Browser-tested world", { exact: false })
      .waitFor({ timeout: 15000 });
    await page.getByText("Fixture ready.", { exact: false }).waitFor();
    await page.locator("#refresh-commands").click();
    await page.getByText("4 server commands", { exact: true }).waitFor();
    await page.locator("#command-input").fill("sa");
    await page.keyboard.press("Tab");
    assert.equal(await page.locator("#command-input").inputValue(), "say ");
    await page.locator("#command-input").fill("say browser check");
    await page.keyboard.press("Enter");
    await page
      .getByText("Executed: say browser check", { exact: true })
      .waitFor({ timeout: 10000 });
    await page.getByText("Alex", { exact: true }).waitFor();
    await page.getByText("42 ms", { exact: true }).waitFor();
    assert.equal(
      await page.locator("#memory-total").textContent(),
      "/ 0.50 GB max",
    );
    assert.equal(await page.locator("#memory-used").textContent(), "0.13 GB");
    assert.ok(
      (await page.locator("#server-uptime").textContent()).startsWith(
        "Uptime 0d 01h",
      ),
    );
    await page
      .getByText("File-only connection: Alex logged in", { exact: true })
      .first()
      .waitFor();
    assert.equal(
      await page
        .getByText("There are 0 of a max of 20 players online:", {
          exact: false,
        })
        .count(),
      0,
    );
    await page.locator("#command-input").fill("tp @a ~ ~2 ~");
    await page.locator("#coordinate-hint").waitFor({ state: "visible" });
    assert.equal(
      await page.locator("#command-input").inputValue(),
      "tp @a ~ ~2 ~",
    );
    await page.keyboard.press("Enter");
    await page.getByText("Executed: tp @a ~ ~2 ~", { exact: true }).waitFor();
    await page.locator("#command-input").fill("tp @a ~ ~2 ~");
    await page.locator("#use-player-coordinates").click();
    assert.equal(
      await page.locator("#command-input").inputValue(),
      "execute as @a at @s run tp @s ~ ~2 ~",
    );
    await page.keyboard.press("Enter");
    await page
      .getByText("Executed: execute as @a at @s run tp @s ~ ~2 ~", {
        exact: true,
      })
      .waitFor();
    checks.push(
      "Removed branding/sidebar, separate host panel, JVM maximum instead of host RAM, player ping, server uptime, full file logs, no zero-list spam, unchanged console commands and explicit player-relative teleport helper",
    );
    checks.push(
      "Real Rust API, persisted server, process launch, Minecraft status, live logs, players, server-derived completion, command execution",
    );
    await page.locator("#add-viewer").click();
    await page.locator("#viewer-url").fill(viewerUrl);
    await page.locator("#viewer-form button[type=submit]").click();
    await page.waitForSelector("#viewer-dialog", { state: "hidden" });
    await page.locator(".viewer-open").click();
    await page.waitForSelector("#webview-dialog[open]");
    assert.equal(
      await page.locator("#plugin-frame").getAttribute("src"),
      viewerUrl,
    );
    await page
      .frameLocator("#plugin-frame")
      .getByText("Test plugin viewer")
      .waitFor();
    await page.locator("#webview-dialog .close-modal").click();
    await page.reload();
    await page.locator(".viewer-card").waitFor();
    checks.push(
      "Plugin viewer save while running, embed dialog, persisted configuration after reload",
    );
    await page.locator("#restart-button").click();
    await page.locator("#confirm-yes").click();
    await page
      .getByText(
        "Server restarted. Watch the console as it comes back online.",
        { exact: true },
      )
      .waitFor({ timeout: 15000 });
    await page.getByText("Browser-tested world", { exact: false }).waitFor();
    await page.locator("#stop-button").click();
    await page.locator("#confirm-yes").click();
    await page
      .getByText("Stop completed. Your world has been saved.", { exact: true })
      .waitFor({ timeout: 15000 });
    checks.push("Confirmed graceful restart and stop through the UI");
    assert.deepEqual(errors, []);
    console.log(
      JSON.stringify({ passed: checks, browserErrors: errors }, null, 2),
    );
  } finally {
    if (externalServer && externalServer.exitCode === null)
      externalServer.stdin.end("stop\n");
    // Ensure a failed test does not leave a fixture process behind.
    try {
      const root = await fetch(`http://127.0.0.1:${appPort}`);
      const cookie = root.headers.get("set-cookie")?.split(";")[0];
      const response = await fetch(`http://127.0.0.1:${appPort}/api/servers`, {
        headers: { Cookie: cookie },
      });
      const data = await response.json();
      for (const server of data.servers || [])
        await fetch(
          `http://127.0.0.1:${appPort}/api/servers/${server.id}/action/stop`,
          { method: "POST", headers: { Cookie: cookie } },
        );
    } catch {}
    await browser.close();
    application.kill("SIGINT");
    viewer.close();
    await rm(directory, { recursive: true, force: true });
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
