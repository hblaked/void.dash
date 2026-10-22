"use strict";

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};
const GiB = 1024 ** 3;
const ui = {
  servers: [],
  states: {},
  selected: localStorage.getItem("void.selected"),
  demo: false,
  paused: false,
  logs: [],
  cutoff: 0,
  history: [],
  historyIndex: 0,
  suggestions: [],
  suggestionIndex: 0,
  charts: {},
  editing: null,
  pendingAction: false,
  discovery: new Set(),
  online: true,
};
const mapIcon =
  '<svg viewBox="0 0 24 24"><path d="m3 6 6-3 6 3 6-3v15l-6 3-6-3-6 3zm6-3v15m6-12v15"/><path d="m5 12 2-2m5 4 1 1m5-5 1-1"/></svg>';
const gridIcon =
  '<svg viewBox="0 0 24 24"><path d="M3 17V7l9-5 9 5v10l-9 5zM3 7l9 5 9-5M12 12v10"/></svg>';
const grassIcon =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" shape-rendering="crispEdges"><rect width="64" height="64" fill="#25281b"/><path d="m5 18 27-14 27 14-27 15z" fill="#8d9c55"/><path d="m5 18 27 15v27L5 45z" fill="#6c5633"/><path d="m32 33 27-15v27L32 60z" fill="#4d3d28"/><path d="m5 18 27 15v10l-6-3v-5l-7-4v6l-6-4v-5l-8-4z" fill="#7d8b47"/><path d="m32 33 27-15v9l-6 3v5l-7 4v-5l-8 4v6l-6 3z" fill="#586a36"/><path d="m14 17 5-3 7 4-5 3zm17-9 8 4-5 3-8-4zm6 14 8-4 6 3-8 4z" fill="#b6b96a"/><path d="m11 36 6 3v5l-6-3zm11 10 6 3v5l-6-3zm17-2 5-3v5l-5 3zm12-9 5-3v5l-5 3z" fill="#9b7750"/><rect x="1" y="1" width="62" height="62" fill="none" stroke="#594a2c" stroke-width="1"/></svg>';

function faceSvg(name) {
  let hash = 0;
  for (const ch of name) hash = ((hash << 5) - hash + ch.charCodeAt(0)) | 0;
  const hues = [
    "#805c3a",
    "#4f624c",
    "#a6844d",
    "#726056",
    "#4c5d68",
    "#806047",
  ];
  const hair = hues[Math.abs(hash) % hues.length];
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8" shape-rendering="crispEdges"><rect width="8" height="8" fill="#bd926d"/><path d="M0 0h8v3H6V2H2v2H0z" fill="${hair}"/><path d="M1 4h2v1H1zm4 0h2v1H5z" fill="#e7debd"/><path d="M2 4h1v1H2zm3 0h1v1H5z" fill="#405452"/><path d="M3 5h2v1H3z" fill="#a37656"/><path d="M2 7V6h1v1h2V6h1v2H2z" fill="${hair}"/></svg>`;
}
const previewNames = [
  "MossyOak",
  "blocksmith_",
  "lunara",
  "NotQuiteSteve",
  "pixelpioneer",
  "enderbloom",
];
const demoServers = [
  {
    id: "demo-evergreen",
    name: "evergreen",
    host: "192.168.1.42",
    port: 25565,
    mode: "local",
    viewers: [
      { name: "squaremap", url: "http://192.168.1.42:8080" },
      { name: "Plan analytics", url: "http://192.168.1.42:8804" },
    ],
  },
  {
    id: "demo-creative",
    name: "creative",
    host: "192.168.1.42",
    port: 25566,
    mode: "local",
    viewers: [{ name: "BlueMap", url: "http://192.168.1.42:8100" }],
  },
  {
    id: "demo-deep",
    name: "the deep",
    host: "192.168.1.43",
    port: 25565,
    mode: "agent",
    viewers: [],
  },
];
function demoState(index) {
  return {
    online: index !== 2,
    managed: index !== 2,
    motd:
      index === 0
        ? "a place to build something good.\nSurvival · Friends · A very questionable spawn."
        : index === 1
          ? "big ideas. infinite blocks.\nCreative · WorldEdit · Make something impossible."
          : "an adventure waiting to happen.\nA fresh world, whenever you are.",
    icon: null,
    version: "Paper 1.21.4",
    players_online: index === 0 ? 12 : index === 1 ? 3 : 0,
    players_max: 40,
    latency_ms: 3,
    players: (index === 0
      ? previewNames
      : index === 1
        ? previewNames.slice(0, 3)
        : []
    ).map((name) => ({ name, uuid: "" })),
    player_list_complete: false,
    resources: {
      cpu: 18.4,
      cpu_count: 8,
      memory_used: 6.14 * GiB,
      memory_total: 8 * GiB,
      process_memory: 4.28 * GiB,
      uptime: 187409,
      disk_free: 182.6 * GiB,
      disk_total: 512 * GiB,
    },
    commands: [
      "ban",
      "banlist",
      "clear",
      "deop",
      "difficulty",
      "effect",
      "enchant",
      "gamemode",
      "gamerule",
      "give",
      "help",
      "kick",
      "list",
      "op",
      "pardon",
      "plugins",
      "save-all",
      "say",
      "seed",
      "setworldspawn",
      "spawnpoint",
      "stop",
      "summon",
      "teleport",
      "time",
      "title",
      "tp",
      "weather",
      "whitelist",
    ],
    command_source: "preview",
  };
}
function demoLogs() {
  const lines = [
    ["info", "[Server thread/INFO]: Starting minecraft server version 1.21.4"],
    ["info", "[Server thread/INFO]: Loading properties"],
    [
      "info",
      "[Server thread/INFO]: This server is running Paper version 1.21.4",
    ],
    ["info", '[Server thread/INFO]: Preparing level "world"'],
    [
      "info",
      "[Server thread/INFO]: Preparing start region for dimension overworld",
    ],
    ["info", "[Server thread/INFO]: [squaremap] Enabling squaremap v1.3.3"],
    [
      "info",
      "[Server thread/INFO]: [squaremap] Web server listening on port 8080",
    ],
    ["info", '[Server thread/INFO]: Done (4.281s)! For help, type "help"'],
    ["info", "[Server thread/INFO]: MossyOak joined the game"],
    ["info", "[Server thread/INFO]: lunara joined the game"],
    [
      "info",
      "[Server thread/INFO]: <blocksmith_> anyone seen my diamond pick?",
    ],
    ["info", "[Server thread/INFO]: <MossyOak> check the chest by spawn :)"],
    [
      "info",
      "[Server thread/INFO]: pixelpioneer has made the advancement [Stone Age]",
    ],
    ["info", "[Server thread/INFO]: Saving chunks for level 'world'/overworld"],
    ["info", "[Server thread/INFO]: Saved the game"],
  ];
  return lines.map(([level, text], i) => ({
    id: i + 1,
    time: `14:${String(22 + Math.floor(i / 6)).padStart(2, "0")}:${String(3 + ((i * 3) % 57)).padStart(2, "0")}`,
    level,
    text,
  }));
}
function selectedServer() {
  return ui.servers.find((s) => s.id === ui.selected);
}
function selectedState() {
  return (
    ui.states[ui.selected] || {
      online: false,
      players: [],
      commands: [],
      resources: null,
    }
  );
}
function text(selector, value) {
  $(selector).textContent = value;
}
let toastTimer;
function toast(message, error = false) {
  const node = $("#toast");
  node.textContent = message;
  node.className = `toast${error ? " error" : ""}`;
  node.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(
    () => {
      node.hidden = true;
    },
    error ? 7000 : 4500,
  );
}
async function api(path, options = {}) {
  const response = await fetch(path, {
    ...options,
    headers: { "Content-Type": "application/json", ...options.headers },
  });
  const type = response.headers.get("content-type") || "";
  if (!type.includes("application/json"))
    throw new Error(
      `Dashboard returned HTTP ${response.status}. Reopen the application and try again.`,
    );
  const data = await response.json();
  if (!response.ok)
    throw new Error(data.error || `Request failed (${response.status})`);
  return data;
}
function path(suffix = "") {
  return `/api/servers/${encodeURIComponent(ui.selected)}${suffix}`;
}
function setDemo() {
  ui.demo = true;
  ui.servers = structuredClone(demoServers);
  demoServers.forEach((s, index) => {
    ui.states[s.id] = demoState(index);
  });
  if (!ui.servers.some((s) => s.id === ui.selected))
    ui.selected = ui.servers[0].id;
  ui.logs = demoLogs();
}
async function load(initial = false) {
  try {
    const data = await api("/api/servers");
    ui.online = true;
    if (!data.servers.length) {
      if (!ui.demo) setDemo();
    } else {
      ui.demo = false;
      ui.servers = data.servers;
      ui.states = data.states;
      if (!ui.servers.some((s) => s.id === ui.selected))
        ui.selected = ui.servers[0].id;
    }
    $("#error-banner").hidden = true;
    render();
    await loadLogs();
    if (
      initial &&
      !ui.demo &&
      selectedState().online &&
      !selectedState().commands?.length
    )
      discoverCommands(true);
  } catch (error) {
    ui.online = false;
    $("#error-banner").hidden = false;
    text("#error-banner", error.message);
    text("#connection-status", "● DASHBOARD DISCONNECTED");
    if (initial) toast(error.message, true);
  }
}
async function loadLogs() {
  if (ui.paused || !ui.selected) return;
  if (ui.demo) {
    renderLogs();
    return;
  }
  const id = ui.selected;
  try {
    const data = await api(`/api/servers/${encodeURIComponent(id)}/logs`);
    if (id !== ui.selected) return;
    ui.logs = data.logs;
    renderLogs();
  } catch (error) {
    if (id === ui.selected) toast(error.message, true);
  }
}
function renderTabs() {
  const nav = $("#server-tabs");
  nav.replaceChildren();
  ui.servers.forEach((server) => {
    const button = el(
      "button",
      `server-tab${server.id === ui.selected ? " active" : ""}`,
    );
    const dot = el("i", ui.states[server.id]?.online ? "online" : "");
    button.append(dot, el("span", "", server.name));
    if (server.id === ui.selected)
      button.append(
        el(
          "span",
          "tab-count",
          String(ui.states[server.id]?.players_online || 0),
        ),
      );
    button.setAttribute(
      "aria-current",
      server.id === ui.selected ? "page" : "false",
    );
    button.addEventListener("click", () => selectServer(server.id));
    nav.append(button);
  });
}
function selectServer(id) {
  ui.selected = id;
  localStorage.setItem("void.selected", id);
  ui.cutoff = 0;
  ui.logs = ui.demo ? demoLogs() : [];
  ui.paused = false;
  ui.historyIndex = ui.history.length;
  $("#command-input").value = "";
  $("#autocomplete").hidden = true;
  text("#pause-logs", "Ⅱ");
  render();
  renderLogs();
  loadLogs();
  if (!ui.demo && selectedState().online && !selectedState().commands?.length)
    discoverCommands(true);
}
function render() {
  const server = selectedServer(),
    state = selectedState();
  if (!server) return;
  renderTabs();
  $("#demo-banner").hidden = !ui.demo;
  text(
    "#connection-status",
    ui.demo ? "● PREVIEW WORKSPACE" : "● LOCAL CONNECTION",
  );
  text("#server-name", server.name);
  text("#server-motd", state.motd || "Waiting for server status…");
  const icon = $("#server-icon");
  if (state.icon && state.icon.startsWith("data:image/png;base64,")) {
    const img = el("img");
    img.src = state.icon;
    img.alt = `${server.name} server icon`;
    icon.replaceChildren(img);
  } else {
    icon.innerHTML = grassIcon;
    icon.setAttribute("aria-label", "Default server icon");
  }
  const onlineBadge = $("#online-badge");
  onlineBadge.className = `online-badge${state.online ? "" : " offline"}`;
  onlineBadge.replaceChildren(
    el("i"),
    document.createTextNode(
      state.online ? "ONLINE" : state.managed ? "STARTING" : "OFFLINE",
    ),
  );
  text("#game-version", state.version || "Awaiting status");
  text(
    "#identity-players",
    `${state.players_online || 0} / ${state.players_max || "—"} players`,
  );
  text(
    "#latency",
    state.latency_ms != null && state.online ? `${state.latency_ms} ms` : "—",
  );
  text(
    "#process-state",
    ui.pendingAction
      ? "WORKING…"
      : ui.demo
        ? "PREVIEW CONTROLS"
        : state.managed
          ? "PROCESS MANAGED"
          : server.mode === "rcon"
            ? "RCON CONNECTION"
            : "SERVER CONTROLS",
  );
  $("#start-button").disabled =
    ui.pendingAction || server.mode === "rcon" || state.online || state.managed;
  $("#restart-button").disabled =
    ui.pendingAction ||
    server.mode === "rcon" ||
    !(state.online || state.managed);
  $("#stop-button").disabled =
    ui.pendingAction || !(state.online || state.managed);
  $("#start-button").title =
    server.mode === "rcon"
      ? "An agent is required to start remote servers."
      : "Start the configured server process";
  $("#restart-button").title =
    server.mode === "rcon"
      ? "An agent is required to restart remote servers."
      : "Save, stop, then start the server";
  text(
    "#console-host",
    `${server.host}:${server.port} / ${server.mode === "rcon" ? "RCON responses" : "stdout + stderr"}`,
  );
  text(
    "#console-connection",
    ui.demo
      ? "Preview session"
      : server.mode === "agent"
        ? "Remote agent"
        : state.managed
          ? "Local process"
          : server.mode === "rcon"
            ? "RCON"
            : "Awaiting process",
  );
  text(
    "#command-source",
    state.commands?.length
      ? `${state.commands.length} ${ui.demo ? "sample" : "server"} commands`
      : "Discover commands",
  );
  $("#command-input").disabled = ui.pendingAction || !ui.online;
  renderResources(state.resources);
  renderPlayers(state);
  renderViewers(server);
  if (!ui.demo && state.error) {
    $("#error-banner").hidden = false;
    text(
      "#error-banner",
      `${state.online ? "Connection notice" : state.managed ? "Server is starting or not answering status" : "Server unavailable"}: ${state.error}`,
    );
  }
}
function renderResources(resource) {
  const setMeter = (selector, percent) => {
    $(selector).style.width = `${Math.max(0, Math.min(100, percent || 0))}%`;
  };
  if (!resource) {
    [
      "#memory-used",
      "#cpu-value",
      "#host-memory",
      "#disk-free",
      "#uptime",
      "#memory-percent",
    ].forEach((s) => text(s, "—"));
    text("#memory-total", "/ — GB");
    text("#cpu-cores", "— cores");
    text("#memory-label", "HOST TELEMETRY");
    text(
      "#resource-note",
      selectedServer()?.mode === "rcon"
        ? "Connect a remote agent for host metrics and full process controls."
        : "Waiting for host telemetry.",
    );
    $("#chart-area").setAttribute("d", "");
    $("#chart-line").setAttribute("d", "");
    setMeter("#cpu-meter", 0);
    setMeter("#host-memory-meter", 0);
    return;
  }
  const processMemory = resource.process_memory != null;
  const memory = processMemory ? resource.process_memory : resource.memory_used;
  const total = resource.memory_total || 1;
  const used = memory / GiB;
  text(
    "#memory-label",
    processMemory
      ? "SERVER PROCESS · RESIDENT MEMORY"
      : "DASHBOARD HOST · MEMORY",
  );
  text("#memory-used", `${used.toFixed(2)} GB`);
  text("#memory-total", `/ ${(total / GiB).toFixed(0)} GB host`);
  text("#memory-percent", `${Math.round((memory / total) * 100)}%`);
  text("#cpu-value", `${resource.cpu.toFixed(1)}%`);
  text("#cpu-cores", `${resource.cpu_count} cores`);
  text(
    "#host-memory",
    `${(resource.memory_used / GiB).toFixed(2)} / ${(total / GiB).toFixed(0)} GB`,
  );
  text("#disk-free", `${(resource.disk_free / GiB).toFixed(1)} GB`);
  const seconds = resource.uptime || 0;
  text(
    "#uptime",
    seconds
      ? `${Math.floor(seconds / 86400)}d ${String(Math.floor((seconds % 86400) / 3600)).padStart(2, "0")}h ${String(Math.floor((seconds % 3600) / 60)).padStart(2, "0")}m`
      : "Not managed",
  );
  text(
    "#resource-note",
    ui.demo
      ? "Sample telemetry · a little breathing room."
      : "Host CPU & disks · process RSS is not JVM heap usage.",
  );
  setMeter("#cpu-meter", resource.cpu);
  setMeter("#host-memory-meter", (resource.memory_used / total) * 100);
  if (!ui.charts[ui.selected])
    ui.charts[ui.selected] = ui.demo
      ? [
          43, 42, 44, 44, 43, 47, 46, 49, 47, 48, 47, 49, 51, 50, 52, 50, 54,
          52, 53, 53,
        ]
      : [];
  const history = ui.charts[ui.selected];
  const now = Date.now();
  if (!history.lastAt || now - history.lastAt >= 2500) {
    history.push((memory / total) * 100);
    if (history.length > 21) history.shift();
    history.lastAt = now;
  }
  const points = history.map(
    (v, i) =>
      `${360 - (history.length - 1 - i) * 18},${100 - Math.min(v, 100)}`,
  );
  const line = `M${points.join(" L")}`;
  $("#chart-line").setAttribute("d", line);
  $("#chart-area").setAttribute(
    "d",
    `${line} L360,100 L${360 - (history.length - 1) * 18},100 Z`,
  );
}
function renderPlayers(state) {
  const list = $("#players-list");
  list.replaceChildren();
  const players = state.players || [];
  text("#player-count", `${state.players_online || 0} PLAYERS ONLINE`);
  text(
    "#players-slots",
    state.players_max
      ? `${Math.max(0, state.players_max - (state.players_online || 0))} available slots`
      : "Awaiting server status",
  );
  text(
    "#players-caption",
    ui.demo
      ? `Preview · showing ${players.length} sample players`
      : state.player_list_complete
        ? "Everyone currently in your world."
        : "Status sample · configure RCON for the full list.",
  );
  if (!players.length) {
    list.append(
      el(
        "div",
        "empty-players",
        state.online
          ? state.players_online
            ? "The server hides its player list. Configure RCON to fetch it."
            : "A quiet moment. The next adventure is a login away."
          : "Your players will appear here when the server is online.",
      ),
    );
    return;
  }
  players.forEach((player) => {
    const node = el("div", "player"),
      face = el("div", "player-face");
    face.innerHTML = faceSvg(player.name);
    if (!ui.demo) {
      const img = el("img");
      img.alt = `${player.name}'s Minecraft skin`;
      img.loading = "lazy";
      img.referrerPolicy = "no-referrer";
      img.src = `https://mc-heads.net/avatar/${encodeURIComponent(player.uuid || player.name)}/32`;
      img.addEventListener(
        "error",
        () => {
          face.innerHTML = faceSvg(player.name);
        },
        { once: true },
      );
      face.replaceChildren(img);
    } else face.setAttribute("aria-label", `${player.name} sample skin face`);
    const info = el("div", "player-info");
    const label = el("span", "player-label");
    label.append(el("i"), document.createTextNode("In the overworld"));
    if (!ui.demo)
      label.replaceChildren(el("i"), document.createTextNode("Connected"));
    info.append(el("span", "player-name", player.name), label);
    node.append(face, info);
    list.append(node);
  });
}
function renderViewers(server) {
  const list = $("#viewer-list");
  list.replaceChildren();
  if (!server.viewers?.length) {
    list.append(
      el(
        "div",
        "viewer-empty",
        "Maps. Analytics. Your world from another angle.\nAdd a plugin’s website to get started.",
      ),
    );
    return;
  }
  server.viewers.forEach((viewer, index) => {
    const card = el("div", "viewer-card"),
      icon = el("div", "viewer-icon");
    icon.innerHTML = viewer.name.toLowerCase().includes("plan")
      ? gridIcon
      : mapIcon;
    const copy = el("div", "viewer-copy");
    copy.append(
      el("strong", "", viewer.name),
      el(
        "span",
        "",
        viewer.name.toLowerCase().includes("plan")
          ? "Player insights & server analytics"
          : /map/i.test(viewer.name)
            ? "Your world, from a different perspective."
            : viewer.url,
      ),
    );
    const open = el("button", "viewer-open", "↗");
    open.title = `Open ${viewer.name}`;
    open.setAttribute("aria-label", `Open ${viewer.name}`);
    open.addEventListener("click", () => openViewer(viewer));
    const remove = el("button", "viewer-remove", "×");
    remove.title = `Remove ${viewer.name}`;
    remove.setAttribute("aria-label", `Remove ${viewer.name}`);
    remove.addEventListener("click", () =>
      confirmAction(
        "Remove viewer?",
        `Remove ${viewer.name} from this dashboard? The plugin itself will stay on the server.`,
        async () => {
          const viewers = server.viewers.filter((_, i) => i !== index);
          await saveViewers(viewers);
        },
      ),
    );
    card.append(icon, copy, open, remove);
    list.append(card);
  });
}
function renderLogs() {
  const output = $("#console-output");
  const nearBottom =
    output.scrollTop + output.clientHeight >= output.scrollHeight - 45;
  const filter = $("#log-filter").value.toLowerCase();
  const lines = ui.logs.filter(
    (l) =>
      l.id > ui.cutoff &&
      (!filter ||
        l.text.toLowerCase().includes(filter) ||
        l.level.includes(filter)),
  );
  text("#line-count", `${lines.length} lines${ui.paused ? " · paused" : ""}`);
  output.replaceChildren();
  if (!lines.length) {
    const empty = el("div", "log-empty");
    empty.append(
      el("span", "", "/>"),
      el(
        "p",
        "",
        filter
          ? "No output matches this filter."
          : ui.cutoff
            ? "Console cleared. New output will appear here."
            : selectedServer()?.mode === "rcon"
              ? "RCON shows command responses here. Connect an agent to stream full server logs."
              : "Your console is ready. Start the server here to stream its logs.",
      ),
    );
    output.append(empty);
    return;
  }
  const fragment = document.createDocumentFragment();
  lines.forEach((line) => {
    const cls = line.text.includes("[void.dash] >")
      ? "command"
      : line.text.includes("Done (")
        ? "highlight"
        : line.level;
    const node = el("div", `log-line ${cls}`);
    let message = line.text.replace(/^\[\d{2}:\d{2}:\d{2}\] ?/, "");
    message = message.replace(/^\[Server thread\/(INFO|WARN|ERROR)\]:? ?/, "");
    node.append(
      el("span", "log-time", line.time),
      el("span", "log-level", line.level.toUpperCase()),
      el("span", "log-text", message),
    );
    fragment.append(node);
  });
  output.append(fragment);
  if (nearBottom) output.scrollTop = output.scrollHeight;
}
function suggestions() {
  const value = $("#command-input").value.replace(/^\//, "");
  if (!value || value.includes(" ")) {
    $("#autocomplete").hidden = true;
    ui.suggestions = [];
    return;
  }
  ui.suggestions = (selectedState().commands || [])
    .filter((c) => c.startsWith(value))
    .slice(0, 8);
  ui.suggestionIndex = 0;
  renderSuggestions();
}
function renderSuggestions() {
  const list = $("#autocomplete");
  list.replaceChildren();
  list.hidden = !ui.suggestions.length;
  ui.suggestions.forEach((command, index) => {
    const button = el(
      "button",
      index === ui.suggestionIndex ? "selected" : "",
      `/${command}`,
    );
    button.type = "button";
    button.setAttribute("role", "option");
    button.setAttribute("aria-selected", String(index === ui.suggestionIndex));
    button.append(
      el("span", "", ui.demo ? "PREVIEW COMMAND" : "SERVER COMMAND"),
    );
    button.addEventListener("mousedown", (event) => {
      event.preventDefault();
      completeCommand(index);
    });
    list.append(button);
  });
}
function completeCommand(index = ui.suggestionIndex) {
  if (!ui.suggestions[index]) return;
  $("#command-input").value = `${ui.suggestions[index]} `;
  $("#autocomplete").hidden = true;
  $("#command-input").focus();
}
async function discoverCommands(quiet = false) {
  if (ui.demo) {
    toast(
      "Preview: sample commands loaded. Connected servers use their own help output.",
    );
    return;
  }
  const id = ui.selected;
  if (ui.discovery.has(id)) return;
  ui.discovery.add(id);
  $("#refresh-commands").disabled = true;
  try {
    const result = await api(
      `/api/servers/${encodeURIComponent(id)}/commands/refresh`,
      { method: "POST" },
    );
    if (ui.states[id]) ui.states[id].commands = result.commands;
    if (id === ui.selected) {
      text("#command-source", `${result.commands.length} server commands`);
      suggestions();
    }
    if (!quiet)
      toast(`Discovered ${result.commands.length} commands from the server.`);
  } catch (error) {
    if (!quiet) toast(error.message, true);
  } finally {
    ui.discovery.delete(id);
    $("#refresh-commands").disabled = false;
  }
}
async function sendCommand(event) {
  event.preventDefault();
  const input = $("#command-input");
  const command = input.value.trim().replace(/^\//, "");
  if (!command) return;
  input.disabled = true;
  $("#autocomplete").hidden = true;
  try {
    if (ui.demo) {
      const next = Math.max(0, ...ui.logs.map((l) => l.id)) + 1;
      ui.logs.push({
        id: next,
        time: new Date().toLocaleTimeString("en-GB"),
        level: "info",
        text: `[void.dash] > ${command}`,
      });
      const response =
        command === "list"
          ? "Preview: 12 players online — MossyOak, blocksmith_, lunara…"
          : command === "help"
            ? `Preview commands: ${selectedState().commands.join(", ")}`
            : `Preview: “${command}” would be sent to your connected server.`;
      ui.logs.push({
        id: next + 1,
        time: new Date().toLocaleTimeString("en-GB"),
        level: "info",
        text: response,
      });
      renderLogs();
    } else {
      await api(path("/command"), {
        method: "POST",
        body: JSON.stringify({ command }),
      });
      await loadLogs();
    }
    ui.history.push(command);
    if (ui.history.length > 100) ui.history.shift();
    ui.historyIndex = ui.history.length;
    input.value = "";
  } catch (error) {
    toast(error.message, true);
  } finally {
    input.disabled = false;
    input.focus();
  }
}
async function runAction(action) {
  ui.pendingAction = true;
  render();
  try {
    if (ui.demo) {
      const state = selectedState();
      state.online = action !== "stop";
      state.managed = state.online;
      if (action === "stop") {
        state.players_online = 0;
        state.players = [];
      }
      if (action === "start") {
        state.players_online = 12;
        state.players = previewNames.map((name) => ({ name, uuid: "" }));
      }
      ui.logs.push({
        id: Math.max(...ui.logs.map((l) => l.id), 0) + 1,
        time: new Date().toLocaleTimeString("en-GB"),
        level: "info",
        text: `[void.dash] Preview: server ${action} simulated.`,
      });
      toast(`Preview: ${action} simulated. No real server was affected.`);
      renderLogs();
    } else {
      await api(path(`/action/${action}`), { method: "POST" });
      toast(
        action === "start"
          ? "Server process launched. Watch the console for startup progress."
          : action === "restart"
            ? "Server restarted. Watch the console as it comes back online."
            : "Stop completed. Your world has been saved.",
      );
      await load();
    }
  } catch (error) {
    toast(error.message, true);
  } finally {
    ui.pendingAction = false;
    render();
  }
}
function confirmAction(title, description, callback) {
  text("#confirm-title", title);
  text("#confirm-description", description);
  const dialog = $("#confirm-dialog");
  $("#confirm-yes").onclick = async () => {
    dialog.close();
    try {
      await callback();
    } catch (error) {
      toast(error.message, true);
    }
  };
  dialog.showModal();
}
function modeFields() {
  const mode = $("#server-mode").value;
  $$(".local-field").forEach((n) => (n.hidden = mode !== "local"));
  $$(".rcon-field").forEach((n) => (n.hidden = mode === "agent"));
  $$(".agent-field").forEach((n) => (n.hidden = mode !== "agent"));
  $$(".local-optional").forEach((n) => (n.hidden = mode !== "local"));
  $("#server-form [name=directory]").required = mode === "local";
  $("#server-form [name=agent_url]").required = mode === "agent";
  $("#server-form [name=agent_server_id]").required = mode === "agent";
}
function openServerDialog(edit = false) {
  const form = $("#server-form");
  form.reset();
  ui.editing = edit && !ui.demo ? selectedServer() : null;
  text("#server-form-error", "");
  text(
    "#server-dialog-title",
    ui.editing ? "Server configuration" : "Connect a server",
  );
  text("#save-server", ui.editing ? "Save changes ↗" : "Connect server ↗");
  $("#delete-server").hidden = !ui.editing;
  if (ui.editing) {
    for (const [name, value] of Object.entries(ui.editing)) {
      const input = form.elements.namedItem(name);
      if (input) input.value = name === "args" ? JSON.stringify(value) : value;
    }
    form.elements.rcon_password.placeholder = ui.editing.has_rcon_password
      ? "Leave blank to keep existing password"
      : "RCON password";
    form.elements.agent_token.placeholder = ui.editing.has_agent_token
      ? "Leave blank to keep existing token"
      : "Agent token";
  }
  modeFields();
  $("#server-dialog").showModal();
}
async function saveServer(event) {
  event.preventDefault();
  text("#server-form-error", "");
  const form = event.currentTarget;
  const fields = new FormData(form);
  const data = Object.fromEntries(fields);
  try {
    data.args = JSON.parse(data.args || "[]");
    if (
      !Array.isArray(data.args) ||
      data.args.some((a) => typeof a !== "string")
    )
      throw new Error("Arguments must be a JSON array of strings.");
    data.port = Number(data.port);
    data.rcon_port = Number(data.rcon_port);
    data.viewers = ui.editing?.viewers || [];
    $("#save-server").disabled = true;
    const result = await api(
      ui.editing
        ? `/api/servers/${encodeURIComponent(ui.editing.id)}`
        : "/api/servers",
      { method: ui.editing ? "PUT" : "POST", body: JSON.stringify(data) },
    );
    ui.selected = result.id;
    ui.logs = [];
    ui.cutoff = 0;
    localStorage.setItem("void.selected", result.id);
    $("#server-dialog").close();
    await load();
    toast("Server configuration saved on this computer.");
  } catch (error) {
    text("#server-form-error", error.message);
  } finally {
    $("#save-server").disabled = false;
  }
}
async function saveViewers(viewers) {
  if (ui.demo) {
    selectedServer().viewers = viewers;
    renderViewers(selectedServer());
    toast("Preview viewer updated for this session.");
    return;
  }
  await api(path("/viewers"), {
    method: "PUT",
    body: JSON.stringify({ viewers }),
  });
  await load();
  toast("Plugin viewers saved.");
}
function openViewer(viewer) {
  if (ui.demo) {
    toast(
      "This is a sample viewer. Connect your server and add its plugin’s website URL to view your world.",
    );
    return;
  }
  if (!/^https?:\/\//i.test(viewer.url)) {
    toast("This viewer needs an HTTP(S) URL.", true);
    return;
  }
  text("#webview-title", viewer.name);
  $("#webview-external").href = viewer.url;
  $("#plugin-frame").src = viewer.url;
  $("#webview-dialog").showModal();
}
function showViewerDialog() {
  $("#viewer-form").reset();
  text("#viewer-form-error", "");
  const host = selectedServer()?.host || "127.0.0.1";
  $("#viewer-url").value =
    `http://${host.includes(":") ? `[${host}]` : host}:8080`;
  $("#viewer-dialog").showModal();
}

$("#add-server").addEventListener("click", () => openServerDialog());
$("#connect-first").addEventListener("click", () => openServerDialog());
$("#settings-button").addEventListener("click", () => openServerDialog(true));
$("#configure-tab").addEventListener("click", () => openServerDialog(true));
$("#help-button").addEventListener("click", () =>
  $("#help-dialog").showModal(),
);
$("#overview-button").addEventListener("click", () =>
  window.scrollTo({ top: 0, behavior: "smooth" }),
);
$("#overview-tab").addEventListener("click", () =>
  $("#console-panel").scrollIntoView({ behavior: "smooth", block: "center" }),
);
$("#focus-console").addEventListener("click", () => {
  $("#console-panel").scrollIntoView({ behavior: "smooth", block: "center" });
  $("#command-input").focus({ preventScroll: true });
});
$("#focus-viewers").addEventListener("click", () =>
  $("#viewers-panel").scrollIntoView({ behavior: "smooth", block: "center" }),
);
$$(".close-modal,.cancel-modal").forEach((button) =>
  button.addEventListener("click", () => button.closest("dialog").close()),
);
$$("dialog").forEach((dialog) => {
  dialog.addEventListener("click", (event) => {
    const r = dialog.getBoundingClientRect();
    if (
      event.target === dialog &&
      (event.clientX < r.left ||
        event.clientX > r.right ||
        event.clientY < r.top ||
        event.clientY > r.bottom)
    )
      dialog.close();
  });
});
$("#webview-dialog").addEventListener("close", () =>
  $("#plugin-frame").removeAttribute("src"),
);
$("#server-mode").addEventListener("change", modeFields);
$("#server-form").addEventListener("submit", saveServer);
$("#delete-server").addEventListener("click", () => {
  const server = ui.editing;
  confirmAction(
    "Remove server?",
    `Remove ${server.name} from void.dash? Your Minecraft files will stay on disk.`,
    async () => {
      await api(`/api/servers/${encodeURIComponent(server.id)}`, {
        method: "DELETE",
      });
      $("#server-dialog").close();
      ui.selected = null;
      await load();
      toast("Server removed from the dashboard.");
    },
  );
});
$("#add-viewer").addEventListener("click", showViewerDialog);
$("#viewer-preset").addEventListener("change", (event) => {
  const presets = {
    squaremap: ["squaremap", 8080],
    bluemap: ["BlueMap", 8100],
    dynmap: ["Dynmap", 8123],
    plan: ["Plan analytics", 8804],
    custom: ["My viewer", 8080],
  };
  const [name, port] = presets[event.target.value];
  $("#viewer-name").value = name;
  const host = selectedServer()?.host || "127.0.0.1";
  $("#viewer-url").value =
    `http://${host.includes(":") ? `[${host}]` : host}:${port}`;
});
$("#viewer-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  text("#viewer-form-error", "");
  try {
    const url = new URL($("#viewer-url").value);
    if (!["http:", "https:"].includes(url.protocol))
      throw new Error("Use an HTTP(S) website URL.");
    await saveViewers([
      ...(selectedServer().viewers || []),
      { name: $("#viewer-name").value.trim(), url: url.href },
    ]);
    $("#viewer-dialog").close();
  } catch (error) {
    text("#viewer-form-error", error.message);
  }
});
$("#start-button").addEventListener("click", () => runAction("start"));
$("#stop-button").addEventListener("click", () =>
  confirmAction(
    "Give your world a breather?",
    `Stop ${selectedServer().name}? Connected players will be disconnected. The server will save before it shuts down.`,
    () => runAction("stop"),
  ),
);
$("#restart-button").addEventListener("click", () =>
  confirmAction(
    "A fresh start?",
    `Restart ${selectedServer().name}? Connected players will be disconnected while the server saves and starts again.`,
    () => runAction("restart"),
  ),
);
$("#log-filter").addEventListener("input", renderLogs);
$("#pause-logs").addEventListener("click", () => {
  ui.paused = !ui.paused;
  text("#pause-logs", ui.paused ? "▷" : "Ⅱ");
  $("#pause-logs").title = ui.paused
    ? "Resume log updates"
    : "Pause log updates";
  $("#pause-logs").setAttribute("aria-label", $("#pause-logs").title);
  if (!ui.paused) loadLogs();
  renderLogs();
});
$("#clear-logs").addEventListener("click", () => {
  ui.cutoff = Math.max(...ui.logs.map((l) => l.id), 0);
  renderLogs();
});
$("#export-logs").addEventListener("click", () => {
  const blob = new Blob(
    [
      ui.logs
        .map((l) => `[${l.time}] [${l.level.toUpperCase()}] ${l.text}`)
        .join("\n"),
    ],
    { type: "text/plain" },
  );
  const url = URL.createObjectURL(blob);
  const anchor = el("a");
  anchor.href = url;
  anchor.download = `void-dash-${selectedServer().name.replace(/[^a-z0-9_-]/gi, "_")}-${new Date().toISOString().slice(0, 10)}.log`;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
});
$("#command-form").addEventListener("submit", sendCommand);
$("#command-input").addEventListener("input", suggestions);
$("#refresh-commands").addEventListener("click", () => discoverCommands());
$("#command-input").addEventListener("keydown", (event) => {
  if (event.key === "Tab" && !$("#autocomplete").hidden) {
    event.preventDefault();
    completeCommand();
  } else if (["ArrowUp", "ArrowDown"].includes(event.key)) {
    event.preventDefault();
    if (!$("#autocomplete").hidden && ui.suggestions.length) {
      ui.suggestionIndex =
        (ui.suggestionIndex +
          (event.key === "ArrowDown" ? 1 : -1) +
          ui.suggestions.length) %
        ui.suggestions.length;
      renderSuggestions();
    } else {
      ui.historyIndex = Math.max(
        0,
        Math.min(
          ui.history.length,
          ui.historyIndex + (event.key === "ArrowUp" ? -1 : 1),
        ),
      );
      event.target.value = ui.history[ui.historyIndex] || "";
    }
  } else if (event.key === "Escape") $("#autocomplete").hidden = true;
});
document.addEventListener("click", (event) => {
  if (!event.target.closest(".console-input-wrap"))
    $("#autocomplete").hidden = true;
});
function clock() {
  text("#clock", new Date().toLocaleTimeString("en-GB"));
}
clock();
setInterval(clock, 1000);
load(true);
let refreshing = false;
setInterval(async () => {
  if (refreshing || document.hidden) return;
  refreshing = true;
  try {
    await load();
  } finally {
    refreshing = false;
  }
}, 3000);
