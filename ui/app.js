// Koi workbench: Status + About + Discover.
// Transport rule (the Ghostlight rule): the webview holds no network. Every
// daemon byte crosses via Tauri commands; live events arrive as `mdns-event`.
// Pure layers hold no document; only this composition root touches the DOM.

const DAEMON_ORIGIN = "http://127.0.0.1:5641";

const invoke = window.__TAURI__?.core?.invoke;

// ── debug sink: milestones + every failure path, to disk via Rust ────
function dlog(message) {
  try { invoke?.("debug_log", { message }); } catch {}
}
window.addEventListener("error", (e) => dlog(`JS error: ${e.message} @ ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => dlog(`unhandled rejection: ${e.reason}`));
window.addEventListener("securitypolicyviolation", (e) =>
  dlog(`CSP blocked: ${e.violatedDirective} ${e.blockedURI} (source ${e.sourceFile ?? ""}:${e.lineNumber ?? ""})`));
dlog("workbench booted");

function postureWord(level) {
  switch (level) {
    case "open": return "open";
    case "authenticated": return "authenticated";
    case "confidential": return "confidential";
    default: return "unknown";
  }
}

function escapeHtml(text) {
  return String(text ?? "").replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}

// ── composition root ─────────────────────────────────────────────────
const el = {};
for (const id of [
  "lamp", "state-word", "state-facts",
  "service-state", "service-detail", "btn-start", "btn-run-once", "btn-stop", "action-note",
  "autostart-toggle",
  "t-http", "t-posture", "t-version",
  "t-stream", "t-types", "t-instances", "discover-queue", "discover-count",
  "koi-card", "card-version", "f-daemon", "f-posture", "f-host", "f-version",
]) el[id] = document.getElementById(id);

let lastSignature = "";

function setFact(node, text, tone) {
  if (!node) return;
  node.textContent = text;
  node.className = tone || "";
}

function note(text, isError) {
  if (!el["action-note"]) return;
  el["action-note"].textContent = text || "";
  el["action-note"].className = "action-note" + (isError ? " error" : "");
}

// ── Status + About facts — event-driven via the daemon's /v1/events ──
let lastStatus = null;

function applyStatus(snap, svc) {
  const up = snap.up === true;
  const level = snap.posture ? postureWord(snap.posture) : "—";
  const version = snap.version ? String(snap.version) : "—";

  // header lamp — Ghostlight drives states through body classes
  document.body.classList.toggle("runtime-offline", !up);
  el["state-word"].textContent = up ? "Calm waters" : "Quiet pond";
  el["state-facts"].textContent = up
    ? "http 127.0.0.1:5641 · posture " + level
    : "no daemon on this machine";

  // service strip
  el["service-state"].textContent = svc.running
    ? "Service running"
    : svc.installed ? "Service installed but stopped" : "Service not running";
  el["service-detail"].textContent = svc.running
    ? "The Koi daemon is serving on standard ports."
    : svc.installed
      ? "Installed as a system service; start it below."
      : "Not installed as a service yet — run it once on demand, or install with `koi install`.";
  el["btn-start"].disabled = !svc.installed || svc.running;
  el["btn-stop"].disabled = !svc.running;
  el["btn-run-once"].disabled = false;

  // diagnostic tiles
  setFact(el["t-http"], up ? "serving at http://127.0.0.1:5641" : "no listener on 5641", up ? "ok" : "down");
  setFact(el["t-posture"], level, up ? "" : "down");
  setFact(el["t-version"], version);

  // about facts mirror
  setFact(el["f-daemon"], up ? "running" : "not running", up ? "ok" : "down");
  setFact(el["f-posture"], level, up ? "ok" : "down");
  setFact(el["f-version"], version);
  el["card-version"].textContent = version !== "—" ? version.split(".").slice(0, 2).join(".") : "—";
}

async function refreshStatus() {
  let snap;
  try {
    snap = await invoke("daemon_status");
  } catch (error) {
    dlog(`daemon_status failed: ${error}`);
    snap = { up: false, version: null, posture: null };
  }
  let svc = { installed: false, running: false };
  try { svc = await invoke("service_status"); } catch {}
  const signature = JSON.stringify([snap, svc]);
  if (signature === lastStatus) return;
  lastStatus = signature;
  applyStatus(snap, svc);
}

// ── service actions ──────────────────────────────────────────────────
async function act(name) {
  if (!invoke) { note("Desktop commands unavailable in this build.", true); return; }
  note("Working…");
  for (const id of ["btn-start", "btn-run-once", "btn-stop"]) el[id].disabled = true;
  try {
    const result = await invoke(name);
    note(result && result.message ? result.message : "Done.");
  } catch (error) {
    note(String(error), true);
  }
  lastStatus = "";
  refreshStatus();
}

el["btn-start"]?.addEventListener("click", () => act("service_start"));
el["btn-stop"]?.addEventListener("click", () => act("service_stop"));
el["btn-run-once"]?.addEventListener("click", () => act("daemon_run_once"));

// ── autostart (login launch, minimized to tray) ─────────────────────────────
// The plugin degrades honestly: if the platform refuses (unsupported session,
// policy), the toggle reports the failure instead of pretending.
const autostartApi = window.__TAURI__?.autostart;
async function refreshAutostart() {
  const toggle = el["autostart-toggle"];
  if (!toggle || !autostartApi?.isEnabled) return;
  try {
    toggle.checked = await autostartApi.isEnabled();
  } catch (error) {
    dlog(`autostart isEnabled failed: ${error}`);
    toggle.disabled = true;
    note(`Autostart is unavailable here: ${error}`, true);
  }
}
el["autostart-toggle"]?.addEventListener("change", async (event) => {
  if (!autostartApi) return;
  const want = event.target.checked;
  try {
    if (want) { await autostartApi.enable(); } else { await autostartApi.disable(); }
    note(want
      ? "Koi will start (minimized to the tray) when you log in."
      : "Koi will no longer start at login.");
  } catch (error) {
    dlog(`autostart change failed: ${error}`);
    note(`Autostart could not be changed: ${error}`, true);
  }
  refreshAutostart();
});
refreshAutostart();
document.getElementById("refresh-status")?.addEventListener("click", () => { lastStatus = ""; refreshStatus(); });

// ── tabs ─────────────────────────────────────────────────────────────
for (const tab of document.querySelectorAll(".tab")) {
  tab.addEventListener("click", () => {
    for (const t of document.querySelectorAll(".tab")) {
      t.classList.toggle("active", t === tab);
      if (t === tab) t.setAttribute("aria-current", "page"); else t.removeAttribute("aria-current");
    }
    for (const view of document.querySelectorAll(".view")) {
      view.classList.toggle("active", view.dataset.page === tab.dataset.view);
    }
  });
}

// ── Discover: inhabitants of the network ─────────────────────────────
// The workbench is the memory: the daemon's browser cache evicts, we don't.
// An instance is live (fresh announcement), fading (aging), or gone (kept,
// dimmed, reviving the moment it re-announces).

const instances = new Map(); // key → record {type,name,instance,host,ip,port,txt,resolved,seenAt,lastAnnounce,gone}
const groupNodes = new Map(); // groupKey → {header, rows: Map(rowKey → node)}
const typeLabels = new Map(); // service_type → {label, description}
let streamState = "connecting";
let discoverFilter = "";
let typeFilter = "";
let stateFilter = "";

const LIVE_MS = 90 * 1000;
const FADING_MS = 10 * 60 * 1000;
const FAMILY = /(koi|moss|zen-?garden|ghostlight|sylin|koan)/i;

function key(record) {
  return `${record.service_type} ${record.name}`;
}

function friendlyName(r) {
  const txt = r.txt ?? {};
  return txt.fn || txt.ty || txt.md || txt.model || txt.am || "";
}

function deviceOf(r) {
  const host = (r.host || "").replace(/\.$/, "");
  if (host) return host;
  // instance base: strip the trailing .<type>.local. tail the daemon may carry
  return String(r.instance_name || r.name || "unknown").split(".")[0] || "unknown";
}

function presence(r) {
  if (r.gone) return "gone";
  const age = Date.now() - (r.seenAt ?? 0);
  if (age < LIVE_MS) return "live";
  if (age < FADING_MS) return "fading";
  return "gone";
}

function isFamily(r) {
  return FAMILY.test(`${r.instance_name || ""} ${r.name} ${r.service_type} ${r.host || ""}`);
}

function typeLabel(t) {
  const meta = typeLabels.get(t);
  return meta?.label || shortType(t);
}

function shortType(t) {
  return String(t ?? "").replace(/^_/, "").replace(/\._tcp$/, "").replace(/\._udp$/, "");
}

function agoText(ts) {
  const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
  if (s < 5) return "now";
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

function passesFilters(r) {
  if (typeFilter && r.service_type !== typeFilter) return false;
  const p = presence(r);
  if (stateFilter === "live" && p !== "live") return false;
  if (stateFilter === "gone" && p !== "gone") return false;
  if (discoverFilter) {
    const hay = `${r.instance_name || ""} ${r.name} ${r.service_type} ${r.host || ""} ${r.ip || ""} ${friendlyName(r)}`.toLowerCase();
    if (!hay.includes(discoverFilter)) return false;
  }
  return true;
}

function upsertInstance(record) {
  const k = key(record);
  const existing = instances.get(k);
  const next = {
    ...existing,
    ...record,
    txt: { ...(existing?.txt ?? {}), ...(record.txt ?? {}) },
    seenAt: Date.now(),
    gone: false,
  };
  instances.set(k, next);
  renderGroupFor(next);
  updateDiscoverTiles();
  refreshTypeDropdown();
}

function markGone(evt) {
  const k = `${evt.service_type} ${evt.name}`;
  const r = instances.get(k);
  if (!r) return; // never seen here; nothing to remember
  r.gone = true;
  r.goneAt = Date.now();
  renderGroupFor(r);
}

function instanceRowKey(r) {
  return `${deviceOf(r)} :: ${key(r)}`;
}

function rowMarkup(r) {
  const p = presence(r);
  const endpoint = r.resolved
    ? `${(r.host || r.ip || "?").replace(/\.$/, "")}:${r.port ?? ""}`
    : `${(r.ip || "").trim() || "—"}`;
  const friendly = friendlyName(r);
  return (
    `<div class="med-mini${isFamily(r) ? " family" : ""}">◆</div>` +
    `<div class="row-tool">${escapeHtml(typeLabel(r.service_type))}</div>` +
    `<div class="row-activity">${escapeHtml(r.instance_name || r.name)}${friendly ? ` <span class="sub">· ${escapeHtml(friendly)}</span>` : ""}</div>` +
    `<div class="row-client">${escapeHtml(endpoint)}</div>` +
    `<div class="row-dur">${escapeHtml(agoText(r.seenAt ?? Date.now()))}</div>` +
    `<div class="row-cap">${escapeHtml(p)}</div>`
  );
}

function rowClass(r) {
  const p = presence(r);
  let cls = `row discover ${p}`;
  if (isFamily(r)) cls += " family";
  return cls;
}

function buildRow(r) {
  const node = document.createElement("div");
  node.className = rowClass(r) + " landing";
  node.innerHTML = rowMarkup(r);
  node.addEventListener("animationend", () => node.classList.remove("landing"), { once: true });
  return node;
}

function updateRowNode(node, r) {
  node.className = rowClass(r);
  node.innerHTML = rowMarkup(r);
}

function groupKeyOf(r) {
  return `${isFamily(r) ? "0" : "1"}|${deviceOf(r).toLowerCase()}`;
}

function renderGroupFor(r) {
  const queue = el["discover-queue"];
  if (!queue) return;
  const gk = groupKeyOf(r);
  let group = groupNodes.get(gk);
  if (!group) {
    if (!passesFilters(r)) return;
    const header = document.createElement("div");
    header.className = "group-header" + (isFamily(r) ? " family" : "");
    const rows = document.createElement("div");
    rows.className = "group-rows";
    queue.append(header, rows);
    group = { header, rows, nodes: new Map() };
    groupNodes.set(gk, group);
  }
  // header
  const members = [...instances.values()].filter((x) => groupKeyOf(x) === gk);
  const latest = members.reduce((a, b) => ((a.seenAt ?? 0) > (b.seenAt ?? 0) ? a : b));
  group.header.innerHTML =
    `<span class="g-name">${escapeHtml(deviceOf(latest))}</span>` +
    `<span class="g-meta">${members.filter((m) => presence(m) !== "gone").length} live · ${members.length} services</span>`;
  // rows
  const rk = instanceRowKey(r);
  let node = group.nodes.get(rk);
  if (!passesFilters(r)) {
    if (node) { node.remove(); group.nodes.delete(rk); }
    return;
  }
  if (!node) {
    node = buildRow(r);
    group.nodes.set(rk, node);
    group.rows.append(node);
  } else {
    updateRowNode(node, r);
  }
  sortGroups();
}

function sortGroups() {
  const queue = el["discover-queue"];
  if (!queue) return;
  const order = [...groupNodes.entries()]
    .map(([gk, g]) => {
      const members = [...instances.values()].filter((x) => groupKeyOf(x) === gk);
      const latest = members.reduce((a, b) => ((a.seenAt ?? 0) > (b.seenAt ?? 0) ? a : b), {});
      return { gk, at: latest.seenAt ?? 0 };
    })
    .sort((a, b) => b.at - a.at);
  for (const { gk } of order) {
    const g = groupNodes.get(gk);
    if (g) queue.append(g.header, g.rows);
  }
  const visible = order.filter(({ gk }) => {
    const g = groupNodes.get(gk);
    return g && g.rows.childElementCount > 0;
  });
  const count = el["discover-count"];
  if (count) count.textContent = `${visible.length} device${visible.length === 1 ? "" : "s"}`;
}

function renderAllGroups() {
  for (const r of instances.values()) renderGroupFor(r);
  // drop empty groups
  for (const [gk, g] of [...groupNodes.entries()]) {
    if (g.rows.childElementCount === 0) {
      g.header.remove();
      g.rows.remove();
      groupNodes.delete(gk);
    }
  }
  sortGroups();
}

function updateDiscoverTiles() {
  const all = [...instances.values()];
  const live = all.filter((r) => presence(r) === "live");
  const types = new Set(all.map((r) => r.service_type));
  setFact(el["t-stream"], streamState, streamState === "live" ? "ok" : streamState === "connecting" ? "" : "down");
  setFact(el["t-live"], String(live.length), live.length ? "ok" : "down");
  setFact(el["t-types"], String(types.size));
  setFact(el["t-remembered"], String(all.length));
}

function refreshTypeDropdown() {
  const select = el["discover-type"];
  if (!select) return;
  const types = [...new Set([...instances.values()].map((r) => r.service_type))].sort();
  const current = select.value;
  select.replaceChildren(
    Object.assign(document.createElement("option"), { value: "", textContent: "All types" })
  );
  for (const t of types) {
    select.append(Object.assign(document.createElement("option"), { value: t, textContent: typeLabel(t) }));
  }
  select.value = current;
  if (select.value !== current) { select.value = ""; typeFilter = ""; }
}

async function seedSnapshot() {
  try {
    const snap = await invoke("discover_snapshot");
    dlog(`snapshot ok: instances=${(snap.instances ?? []).length}`);
    for (const meta of snap.service_types ?? []) {
      if (meta.service_type) {
        typeLabels.set(meta.service_type, { label: meta.label, description: meta.description });
      }
    }
    for (const r of snap.instances ?? []) upsertInstance(r);
  } catch (error) {
    streamState = "offline";
    dlog(`snapshot unavailable: ${error}`);
  }
  updateDiscoverTiles();
  refreshTypeDropdown();
}

async function startDiscover() {
  try { await invoke("discover_start"); } catch (error) { dlog(`discover_start failed: ${error}`); }
  await seedSnapshot();
}

if (window.__TAURI__?.event?.listen) {
  window.__TAURI__.event.listen("mdns-event", (event) => {
    const payload = event.payload ?? {};
    switch (payload.kind) {
      case "resolved": {
        if (payload.data) {
          dlog(`resolved: ${payload.data.service_type} ${payload.data.name}`);
          upsertInstance(payload.data);
        }
        break;
      }
      case "removed":
        markGone(payload.data ?? {});
        break;
      case "type_found":
        seedSnapshot(); // picks up the new type's label + any cached instances
        break;
      default:
        break;
    }
  });
}

// Presence and ages tick in place; rows never rebuild, so nothing pops.
setInterval(() => {
  let changedPresence = false;
  for (const [k, r] of instances) {
    const g = groupNodes.get(groupKeyOf(r));
    const node = g?.nodes.get(instanceRowKey(r));
    if (!node) continue;
    const before = node.className;
    const dur = node.querySelector(".row-dur");
    if (dur) dur.textContent = agoText(r.seenAt ?? Date.now());
    updateRowNode(node, r);
    if (node.className !== before) changedPresence = true;
  }
  if (changedPresence) { renderAllGroups(); }
  updateDiscoverTiles();
}, 5000);

document.getElementById("discover-filter")?.addEventListener("input", (e) => {
  discoverFilter = e.target.value.trim().toLowerCase();
  renderAllGroups();
});
document.getElementById("discover-type")?.addEventListener("change", (e) => {
  typeFilter = e.target.value;
  renderAllGroups();
});
document.getElementById("discover-state")?.addEventListener("change", (e) => {
  stateFilter = e.target.value;
  renderAllGroups();
});
document.getElementById("refresh-discover")?.addEventListener("click", () => startDiscover());
document.getElementById("ping-pond")?.addEventListener("click", async () => {
  setFact(el["t-stream"], "pinging…");
  try {
    const result = await invoke("discover_ping");
    dlog(`ping: ${JSON.stringify(result)}`);
    setFact(el["t-stream"], `burst across ${result.types_known ?? 0} type(s)`, "ok");
    // Answers arrive as pushed events; refresh the seed shortly anyway.
    setTimeout(seedSnapshot, 1500);
  } catch (error) {
    dlog(`ping failed: ${error}`);
    setFact(el["t-stream"], String(error), "down");
  }
});

function updateDiscoverTiles() {
  const types = new Set([...instances.values()].map((r) => r.service_type));
  setFact(el["t-types"], String(types.size), types.size ? "ok" : "down");
  setFact(el["t-instances"], String(instances.size), instances.size ? "ok" : "");
  setFact(el["t-stream"], streamState, streamState === "live" ? "ok" : streamState === "connecting" ? "" : "down");
}

async function seedSnapshot() {
  try {
    const snap = await invoke("discover_snapshot");
    dlog(`snapshot ok: instances=${(snap.instances ?? []).length}`);
    for (const r of snap.instances ?? []) upsertInstance(r);
  } catch (error) {
    streamState = "offline";
    dlog(`snapshot unavailable: ${error}`);
  }
  updateDiscoverTiles();
}

async function startDiscover() {
  try { await invoke("discover_start"); } catch (error) { dlog(`discover_start failed: ${error}`); }
  await seedSnapshot();
}

if (window.__TAURI__?.event?.listen) {
  window.__TAURI__.event.listen("mdns-event", (event) => {
    const payload = event.payload ?? {};
    switch (payload.kind) {
      case "resolved":
        if (payload.data) { upsertInstance(payload.data); }
        break;
      case "removed":
        dropInstance(payload.data ?? {});
        break;
      case "type_found":
        updateDiscoverTiles();
        break;
      default:
        break;
    }
  });
}

// The card's sheen and foil follow the pointer, and let go when it leaves.
// Two numbers per move; the compositor does the rest (Ghostlight's port).
function armCard() {
  const card = el["koi-card"];
  if (!card) return;
  card.addEventListener("pointermove", (event) => {
    const box = card.getBoundingClientRect();
    const x = ((event.clientX - box.left) / box.width) * 100;
    const y = ((event.clientY - box.top) / box.height) * 100;
    card.style.setProperty("--mx", `${x.toFixed(1)}%`);
    card.style.setProperty("--my", `${y.toFixed(1)}%`);
    card.style.setProperty("--gx", `${((50 - x) / 6).toFixed(1)}px`);
    card.style.setProperty("--gy", `${((50 - y) / 6).toFixed(1)}px`);
    card.style.setProperty("--holo", "1");
  });
  card.addEventListener("pointerleave", () => card.style.setProperty("--holo", "0"));
}

if (window.__TAURI__?.event?.listen) {
  // The Rust reader owns the real stream state; the UI never invents "live".
  window.__TAURI__.event.listen("discover-stream", (event) => {
    streamState = String(event.payload ?? "connecting");
    updateDiscoverTiles();
  });
  // The lamp: pushed by the Rust reader on /v1/events connect + heartbeats.
  window.__TAURI__.event.listen("daemon-status", (event) => {
    let svc = { installed: false, running: false };
    // SCM state is cheap and local; refresh alongside each push.
    invoke("service_status").then((s) => { svc = s ?? svc; }).catch(() => {})
      .finally(() => applyStatus(event.payload ?? { up: false }, svc));
  });
  // ── Status streaming hero (cycle-1 WP1) ──
  // Sentences stream newest-first; watched subjects pin; a restart storm is
  // ONE flapping row, not N; a settled window leaves the designed quiet state.
  const KS = window.KoiSentences;
  const streamRows = [];            // newest first: {ts, line, tone, target, subject}
  const flap = new Map();           // subject → {count, until, line, tone, target}
  const FLAP_MS = 90 * 1000;
  const STREAM_CAP = 50;
  let watched = new Set();
  try { watched = new Set(JSON.parse(localStorage.getItem("koi-watched") || "[]")); } catch (_) {}

  const heroWord = document.getElementById("hero-word");
  const heroDetail = document.getElementById("hero-detail");
  const heroPins = document.getElementById("hero-pins");
  const heroAttention = document.getElementById("hero-attention");
  const heroNote = document.getElementById("hero-note");
  const streamNode = document.getElementById("status-stream");

  function renderHero(degraded) {
    const pins = [...watched];
    const attention = degraded.slice(0, 4);
    heroPins.textContent = "";
    for (const subject of pins) {
      const b = document.createElement("button");
      b.className = "hero-pin";
      b.type = "button";
      b.textContent = "📌 " + subject;
      b.addEventListener("click", () => unpin(subject));
      heroPins.appendChild(b);
    }
    heroAttention.textContent = attention.join(" · ");
    if (attention.length) {
      heroWord.textContent = "Needs you";
      heroDetail.textContent = attention[0];
      document.getElementById("status-hero").dataset.tone = "warn";
    } else if (streamRows.length || pins.length) {
      heroWord.textContent = "Pond is living";
      heroDetail.textContent = pins.length
        ? "watching " + pins.length + " subject(s); everything else flows past"
        : "events stream as they happen";
      delete document.getElementById("status-hero").dataset.tone;
    } else {
      heroWord.textContent = "Watching the pond";
      heroDetail.textContent = "events land here as they happen, newest first";
      delete document.getElementById("status-hero").dataset.tone;
    }
  }

  function renderStream() {
    streamNode.textContent = "";
    const now = Date.now();
    for (const row of streamRows) {
      const div = document.createElement("div");
      div.className = "stream-row";
      div.dataset.tone = row.tone;
      const ago = document.createElement("span");
      ago.className = "ago";
      ago.textContent = agoText(row.ts);
      const line = document.createElement("span");
      line.className = "line";
      line.textContent = row.line;
      const pin = document.createElement("button");
      pin.className = "pin" + (watched.has(row.subject) ? " pinned" : "");
      pin.type = "button";
      pin.title = watched.has(row.subject) ? "unpin" : "pin to the hero";
      pin.textContent = watched.has(row.subject) ? "★" : "☆";
      pin.addEventListener("click", (e) => {
        e.stopPropagation();
        watched.has(row.subject) ? unpin(row.subject) : pin(row.subject);
      });
      div.append(ago, line, pin);
      div.addEventListener("click", () => gotoView(KoiSentences.targetOf(row.target)));
      streamNode.appendChild(div);
    }
    if (!streamRows.length && !streamNode.querySelector(".stream-row")) {
      // :empty ::after renders the quiet copy; nothing to do
    }
    renderHero(collectDegraded());
  }

  function collectDegraded() {
    const out = [];
    for (const row of streamRows.slice(0, 20)) {
      if (row.tone === "bad" || row.tone === "warn") out.push(row.line);
      if (out.length >= 4) break;
    }
    return out;
  }

  function unpin(subject) {
    watched.delete(subject);
    saveWatched();
    renderStream();
  }

  function pin(subject) {
    watched.add(subject);
    localStorage.setItem("koi-watched", JSON.stringify([...watched]));
    renderStream();
  }

  function saveWatched() {
    localStorage.setItem("koi-watched", JSON.stringify([...watched]));
  }

  function admitToStream(entry) {
    // Flapping: the same subject restarting inside the window is ONE row + count.
    const prior = flap.get(entry.subject);
    if (prior && Date.now() < prior.until &&
        (entry.kind === "runtime.started" || entry.kind === "runtime.stopped")) {
      prior.count += 1;
      prior.until = Date.now() + FLAP_MS;
      const existing = streamRows.find((r) => r.flapKey === entry.subject);
      if (existing) {
        existing.ts = Date.now();
        existing.line = prior.line + " — " + prior.count + " times in the last 90s";
        renderStream();
        return;
      }
    }
    if (entry.kind === "runtime.started" || entry.kind === "runtime.stopped") {
      flap.set(entry.subject, {
        count: 1, until: Date.now() + FLAP_MS,
        line: prior ? prior.line : entry.line, tone: entry.tone, target: entry.target,
      });
    }
    streamRows.unshift({
      ts: Date.now(), line: entry.line, tone: entry.tone,
      target: entry.target, subject: entry.subject,
      kind: entry.kind, flapKey: entry.subject,
    });
    if (streamRows.length > STREAM_CAP) streamRows.length = STREAM_CAP;
    renderStream();
  }

  function gotoView(view) {
    const tab = document.querySelector('.tab[data-view="' + view + '"]');
    if (tab) tab.click();
  }

  // Domain events: forwarded wire events (dns.*, certmesh.*, mdns.* …).
  // A loopback poke (127.0.0.1:5640/poke) means something local just changed
  // the daemon's world (Run once, install, a script) — re-read everything now.
  window.__TAURI__.event.listen("ui-poked", () => {
    dlog("poked: re-reading the pond");
    lastStatus = "";
    refreshStatus();
    seedSnapshot();
  });
  window.__TAURI__.event.listen("daemon-event", (event) => {
    const payload = event.payload ?? {};
    const kind = String(payload.kind ?? "");
    if (!kind) return;
    // A posture/certmesh event may have changed the level — pull once.
    if (/certmesh|posture/.test(kind)) { lastStatus = ""; refreshStatus(); }
    // DNS changes push: the table re-reads instead of polling.
    if (kind.startsWith("dns.")) loadDns();
    const s = KS.sentenceFor(kind, payload.data);
    admitToStream({ kind, line: s.line, tone: s.tone, target: s.target,
                    subject: KS.subjectOf(kind, payload.data) });
  });

  // Hero admission: watched subjects render even with an empty stream.
  renderStream();
} else {
  dlog("tauri event API unavailable — falling back to reconcile-only");
}

// ── DNS pane: static records + ephemeral TXT ─────────────────────────
const el2 = {};
for (const id of [
  "dns-name", "dns-ip", "dns-ttl", "dns-add", "dns-note", "dns-queue", "dns-count",
  "txt-name", "txt-value", "txt-set", "txt-clear", "txt-note", "dns-refresh",
]) el2[id] = document.getElementById(id);

function dnsNote(node, text, isError) {
  if (!node) return;
  node.textContent = text || "";
  node.className = "action-note" + (isError ? " error" : "");
}

async function dnsAct(noteEl, fn) {
  dnsNote(noteEl, "Working…");
  try {
    await fn();
    dnsNote(noteEl, "Done.");
    await loadDns();
  } catch (error) {
    dnsNote(noteEl, String(error), true);
  }
}

async function loadDns() {
  let entries = [];
  try {
    const snap = await invoke("dns_entries");
    entries = snap.entries ?? [];
  } catch (error) {
    dnsNote(el2["dns-note"], String(error), true);
  }
  const queue = el2["dns-queue"];
  if (!queue) return;
  queue.replaceChildren();
  for (const e of entries) {
    const node = document.createElement("div");
    node.className = "row dns";
    node.innerHTML =
      `<div class="row-tool">${escapeHtml(e.name)}</div>` +
      `<div class="row-client mono">${escapeHtml(e.ip)}</div>` +
      `<div class="row-dur">${e.ttl != null ? escapeHtml(String(e.ttl)) : "default"}</div>` +
      `<button class="row-remove" type="button">Remove</button>`;
    node.querySelector(".row-remove")?.addEventListener("click", () => {
      dnsAct(el2["dns-note"], () => invoke("dns_remove", { name: e.name }));
    });
    queue.append(node);
  }
  if (!entries.length) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "No static records — add one above, or let discovery and the mesh derive names.";
    queue.append(empty);
  }
  if (el2["dns-count"]) el2["dns-count"].textContent = `${entries.length} record${entries.length === 1 ? "" : "s"}`;
}

el2["dns-add"]?.addEventListener("click", () => {
  const ttlRaw = el2["dns-ttl"].value.trim();
  const ttl = ttlRaw === "" ? null : Number(ttlRaw);
  if (ttlRaw !== "" && (!Number.isFinite(ttl) || ttl < 0)) {
    dnsNote(el2["dns-note"], "TTL must be a non-negative number of seconds.", true);
    return;
  }
  dnsAct(el2["dns-note"], async () => {
    await invoke("dns_add", { name: el2["dns-name"].value, ip: el2["dns-ip"].value, ttl });
    el2["dns-name"].value = "";
    el2["dns-ip"].value = "";
    el2["dns-ttl"].value = "";
  });
});
el2["txt-set"]?.addEventListener("click", () => {
  dnsAct(el2["txt-note"], () =>
    invoke("dns_txt_set", { name: el2["txt-name"].value, value: el2["txt-value"].value }));
});
el2["txt-clear"]?.addEventListener("click", () => {
  dnsAct(el2["txt-note"], () =>
    invoke("dns_txt_clear", { name: el2["txt-name"].value, value: el2["txt-value"].value }));
});
el2["dns-refresh"]?.addEventListener("click", loadDns);

armCard();
startDiscover();
invoke?.("status_events_start");
refreshStatus();
loadDns();
// Safety net only: the stream is the driver; this catches missed pushes.
setInterval(refreshStatus, 60000);
