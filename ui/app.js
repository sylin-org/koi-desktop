// Koi workbench: Status + About + Discover.
// Transport rule (the Ghostlight rule): the webview holds no network. Every
// daemon byte crosses via Tauri commands; live events arrive as `mdns-event`.
// Pure layers hold no document; only this composition root touches the DOM.

const DAEMON_ORIGIN = "http://127.0.0.1:5641";

const invoke = window.__TAURI__?.core?.invoke;
// Browser mode: the same interface served by the daemon to LAN screens
// (ADR-035 mobile access). Read-only — mutations stay in the desktop app.
const BROWSER_MODE = !invoke;

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
let latestSnap = null; // the most recent daemon snapshot, for glance digest facts

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
  latestSnap = snap;
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
  feedNotify(); // glance digest mirrors daemon facts
}

if (BROWSER_MODE) {
  document.body.classList.add("readonly");
  const heading = document.getElementById("status-heading");
  if (heading && !heading.querySelector(".readonly-badge")) {
    const badge = document.createElement("span");
    badge.className = "readonly-badge";
    badge.textContent = "read-only view";
    heading.appendChild(document.createTextNode(" "));
    heading.appendChild(badge);
  }
  setInterval(refreshStatus, 5000);
}

async function refreshStatus() {
  let snap;
  if (BROWSER_MODE) {
    // Served by the daemon itself: same-origin GET, healthz decides "up".
    try {
      const health = await fetch("/healthz").then((r) => r.ok);
      if (!health) throw new Error("daemon down");
      snap = await fetch("/v1/status").then((r) => r.json());
      snap.up = true;
    } catch (error) {
      dlog(`status poll failed: ${error}`);
      snap = { up: false, version: null, posture: null };
    }
  } else {
    try {
      snap = await invoke("daemon_status");
    } catch (error) {
      dlog(`daemon_status failed: ${error}`);
      snap = { up: false, version: null, posture: null };
    }
  }
  let svc = { installed: false, running: false };
  if (!BROWSER_MODE) {
    try { svc = await invoke("service_status"); } catch {}
  }
  const signature = JSON.stringify([snap, svc]);
  if (signature === lastStatus) return;
  lastStatus = signature;
  applyStatus(snap, svc);
}

// ── Pond QR: publish the interface, then show the LAN URL as a QR ──
const qrModal = document.getElementById("qr-modal");
const qrNote = document.getElementById("qr-note");
const qrSvg = document.getElementById("qr-svg");
const qrUrl = document.getElementById("qr-url");

function openQrModal() { qrModal.hidden = false; }
function closeQrModal() { qrModal.hidden = true; }
document.getElementById("qr-close")?.addEventListener("click", closeQrModal);
qrModal?.addEventListener("click", (e) => { if (e.target === qrModal) closeQrModal(); });

document.getElementById("btn-phone")?.addEventListener("click", async () => {
  openQrModal();
  qrNote.textContent = "Publishing this interface to the daemon…";
  qrSvg.textContent = "";
  qrUrl.textContent = "";
  try {
    await invoke("pond_publish_ui");
    const url = await invoke("pond_qr_target");
    const svg = await invoke("pond_qr_svg", { url });
    qrSvg.innerHTML = svg;
    qrUrl.textContent = url;
    qrNote.textContent = "Scan to open this pond read-only on any screen on this network.";
  } catch (error) {
    qrNote.textContent = `QR failed: ${error}`;
  }
});

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
let discoverLens = "family"; // the curated pond by default; "all" is the raw water

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
  // Discover is the curated lens (the pond): the koi family plus starred
  // subjects. Everything else lives in the Browser pane (the water) —
  // a lens, never a silent absence.
  if (discoverLens === "family" && !isFamily(r) && !feed.watched.has("announcement:" + (r.name || r.instance_name || ""))) return false;
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
  watchedAlive("announcement:" + next.name);
  renderGroupFor(next);
  updateDiscoverTiles();
  refreshTypeDropdown();
  if (browserIsActive()) renderBrowser();
}

function markGone(evt) {
  const k = `${evt.service_type} ${evt.name}`;
  const r = instances.get(k);
  if (!r) return; // never seen here; nothing to remember
  r.gone = true;
  r.goneAt = Date.now();
  watchedFade("announcement:" + (evt.name ?? r.name), `${r.instance_name || r.name} went away.`);
  renderGroupFor(r);
  if (browserIsActive()) renderBrowser();
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

function starButton(r) {
  const subject = "announcement:" + (r.name || r.instance_name);
  const btn = document.createElement("button");
  btn.className = "row-star";
  btn.type = "button";
  const paint = () => {
    const on = feed.watched.has(subject);
    btn.textContent = on ? "★" : "☆";
    btn.title = on ? "Unwatch — no more fade notifications" : "Watch — notify me when it fades";
    btn.classList.toggle("pinned", on);
  };
  paint();
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    feed.watched.has(subject) ? feedUnpin(subject) : feedPin(subject);
    paint();
  });
  feed.listeners.add(paint);
  return btn;
}

function buildRow(r) {
  const node = document.createElement("div");
  node.className = rowClass(r) + " landing";
  node.innerHTML = rowMarkup(r);
  if (r.resolved && r.port) node.append(passageButton(r));
  node.append(starButton(r));
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

async function fetchDiscoverSnapshot() {
  // Browser mode: same-origin GET served by the daemon (read-only LAN view).
  if (BROWSER_MODE) {
    const r = await fetch("/v1/mdns/browser/snapshot");
    if (!r.ok) throw new Error("snapshot " + r.status);
    return await r.json();
  }
  return await invoke("discover_snapshot");
}

async function seedSnapshot() {
  try {
    const snap = await fetchDiscoverSnapshot();
    dlog(`snapshot ok: instances=${(snap.instances ?? []).length}`);
    latestSnapRaw = snap;
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
  renderBrowser();
  feedNotify(); // glance digest mirrors discovery facts
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
  if (browserIsActive()) renderBrowser();
  updateDiscoverTiles();
}, 5000);

document.getElementById("discover-filter")?.addEventListener("input", (e) => {
  discoverFilter = e.target.value.trim().toLowerCase();
  renderAllGroups();
});
document.getElementById("discover-lens")?.addEventListener("change", (e) => {
  discoverLens = e.target.value === "all" ? "all" : "family";
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

// ── The feed (cycle-1 WP0/WP2): one sentence stream, two surfaces ────
// Status renders it as the streaming hero; At a glance recounts it. The
// store holds no DOM — panes subscribe and render from it. Flapping stays
// ONE row + count per 90s window; the cap keeps the diary bounded.
const KS = window.KoiSentences;
const FLAP_MS = 90 * 1000;
const STREAM_CAP = 50;

const feed = {
  rows: [],          // newest first: {ts, line, tone, target, subject, kind}
  flap: new Map(),   // subject → {count, until, line, tone, target}
  watched: new Set(),
  degraded: [],      // attention lines, derived on every admit
  listeners: new Set(),
};
try { feed.watched = new Set(JSON.parse(localStorage.getItem("koi-watched") || "[]")); } catch (_) {}

function feedNotify() {
  feed.degraded = feed.rows
    .filter((r) => r.tone === "bad" || r.tone === "warn")
    .map((r) => r.line)
    .slice(0, 4);
  for (const fn of feed.listeners) {
    try { fn(); } catch (e) { dlog(`feed listener failed: ${e}`); }
  }
}

function feedAdmit(entry) {
  // Flapping: the same subject restarting inside the window is ONE row + count.
  const prior = feed.flap.get(entry.subject);
  const restart = entry.kind === "runtime.started" || entry.kind === "runtime.stopped";
  if (prior && Date.now() < prior.until && restart) {
    prior.count += 1;
    prior.until = Date.now() + FLAP_MS;
    const existing = feed.rows.find((r) => r.subject === entry.subject);
    if (existing) {
      existing.ts = Date.now();
      existing.line = prior.line + " — " + prior.count + " times in the last 90s";
      feedNotify();
      return;
    }
  }
  if (restart) {
    feed.flap.set(entry.subject, {
      count: 1, until: Date.now() + FLAP_MS,
      line: prior ? prior.line : entry.line, tone: entry.tone, target: entry.target,
    });
  }
  feed.rows.unshift({
    ts: Date.now(), line: entry.line, tone: entry.tone,
    target: entry.target, subject: entry.subject, kind: entry.kind,
  });
  if (feed.rows.length > STREAM_CAP) feed.rows.length = STREAM_CAP;
  if (entry.kind === "runtime.stopped") {
    watchedFade(entry.subject, entry.line + ".");
  } else if (entry.kind === "runtime.started" || entry.kind === "runtime.reconnected") {
    watchedAlive(entry.subject);
  }
  feedNotify();
}

function feedWatchedSave() {
  localStorage.setItem("koi-watched", JSON.stringify([...feed.watched]));
}

function feedPin(subject) {
  feed.watched.add(subject);
  feedWatchedSave();
  feedNotify();
}

function feedUnpin(subject) {
  feed.watched.delete(subject);
  feedWatchedSave();
  feedNotify();
}

// ── Care (WP8/H1): one OS notification when a watched subject fades ──
// Opt-in by starring; one notification per fade episode — returning to life
// re-arms it. The plugin refusal degrades to the debug sink, never a lie.
const notifiedFades = new Set();

function watchedFade(subject, line) {
  if (!feed.watched.has(subject) || notifiedFades.has(subject)) return;
  notifiedFades.add(subject);
  if (!invoke) { dlog(`fade (watched): ${line}`); return; }
  invoke("notify", { title: "Koi — a watched inhabitant faded", body: line })
    .catch((error) => dlog(`notification failed: ${error}`));
}

function watchedAlive(subject) {
  if (notifiedFades.has(subject)) notifiedFades.delete(subject);
}

function gotoView(view) {
  const tab = document.querySelector('.tab[data-view="' + view + '"]');
  if (tab) tab.click();
}

// ── At a glance (cycle-1 WP2): the quick read ────────────────────────
// Hero: attention → watched → quiet (B1). Happenings: the feed deduplicated,
// newest first (B2); every row click-throughs via the registry's target (B3).
// "Since you last looked" is honest diary depth: rows observed since the last
// time this pane was actually open (B4). No invented history across runs —
// until this session has happenings, the digest states the pond's present.
const glance = {};
for (const id of ["glance-word", "glance-detail", "glance-pins", "glance-count",
                  "glance-digest", "glance-happenings"]) {
  glance[id] = document.getElementById(id);
}

let glanceSince = Number(localStorage.getItem("koi-glance-seen") || 0);

function glanceIsActive() {
  return document.getElementById("view-glance")?.classList.contains("active");
}

function glanceMarkSeen() {
  glanceSince = Date.now();
  try { localStorage.setItem("koi-glance-seen", String(glanceSince)); } catch (_) {}
}

function glanceDedupe() {
  // Grouped + deduplicated: consecutive rows about the same subject collapse
  // into one row with a count. The feed iterates newest-first, so the kept
  // row already holds the latest sentence — older rows only add their count.
  const out = [];
  for (const row of feed.rows) {
    const prev = out[out.length - 1];
    if (prev && prev.subject === row.subject && prev.kind === row.kind) {
      prev.count += 1;
      continue;
    }
    out.push({ ...row, count: 1 });
  }
  return out;
}

function glanceDigest() {
  const all = [...instances.values()];
  const live = all.filter((r) => presence(r) !== "gone").length;
  const types = new Set(all.map((r) => r.service_type)).size;
  const up = latestSnap?.up === true;
  return (
    "The pond right now: " + live + " inhabitant" + (live === 1 ? "" : "s") +
    " across " + types + " type" + (types === 1 ? "" : "s") +
    " · daemon " + (up ? "up" : "down")
  );
}

function glanceRow(row) {
  const div = document.createElement("div");
  div.className = "stream-row";
  div.dataset.tone = row.tone;
  const ago = document.createElement("span");
  ago.className = "ago";
  ago.textContent = agoText(row.ts);
  const line = document.createElement("span");
  line.className = "line";
  line.textContent = row.count > 1 ? row.line + "  ×" + row.count : row.line;
  div.append(ago, line);
  div.addEventListener("click", () => gotoView(row.target));
  return div;
}

function renderGlance() {
  const happenings = glance["glance-happenings"];
  if (!happenings) return;

  // hero: attention → watched → quiet (B1)
  glance["glance-pins"].textContent = "";
  for (const subject of feed.watched) {
    const b = document.createElement("button");
    b.className = "hero-pin";
    b.type = "button";
    b.textContent = "📌 " + subject;
    b.addEventListener("click", () => feedUnpin(subject));
    glance["glance-pins"].appendChild(b);
  }
  const attention = feed.degraded;
  if (attention.length) {
    glance["glance-word"].textContent = "Needs you";
    glance["glance-detail"].textContent = attention.join(" · ");
    glance["glance-word"].closest(".strip")?.setAttribute("data-tone", "warn");
  } else if (feed.watched.size) {
    glance["glance-word"].textContent = "Pond is living";
    glance["glance-detail"].textContent =
      "watching " + feed.watched.size + " subject(s)";
    glance["glance-word"].closest(".strip")?.removeAttribute("data-tone");
  } else {
    glance["glance-word"].textContent = "All quiet";
    glance["glance-detail"].textContent = "nothing needs you right now";
    glance["glance-word"].closest(".strip")?.removeAttribute("data-tone");
  }
  glance["glance-count"].textContent = feed.rows.length + " this session";

  // digest (B4): only until the session has real happenings to show
  const digest = glance["glance-digest"];
  if (!feed.rows.length) {
    digest.hidden = false;
    digest.textContent = glanceDigest();
  } else {
    digest.hidden = true;
  }

  // happenings (B2), grouped since-last-visit vs. the rest of the session
  happenings.textContent = "";
  const since = [];
  const session = [];
  for (const row of glanceDedupe()) {
    (glanceSince && row.ts > glanceSince ? since : session).push(row);
  }
  for (const group of [{ label: "Since you last looked", rows: since },
                       { label: "This session", rows: session }]) {
    if (!group.rows.length) continue;
    const head = document.createElement("div");
    head.className = "hap-group-head";
    head.textContent = group.label;
    happenings.append(head);
    for (const row of group.rows) happenings.append(glanceRow(row));
  }
  if (!feed.rows.length) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "Nothing has happened yet — happenings collect here as the pond lives.";
    happenings.append(empty);
  }

  // Rows just rendered on an open pane are seen; the diary mark is honest.
  if (glanceIsActive()) glanceMarkSeen();
}
feed.listeners.add(renderGlance);

// ── Browser: the raw lens (cycle-1 WP3) ──────────────────────────────
// Discover is the pond (curated); the Browser is the water (raw): every
// announcement the daemon hears, koi or not, with its TXT records expanded
// on selection and the type dictionary shown. Everything here is a state
// the daemon declared — its snapshot's first/last seen are the diary, its
// removals are shown, never hidden.
const browser = {};
for (const id of ["browser-queue", "browser-count", "browser-types", "browser-instances",
                  "browser-filter", "browser-type", "browser-state", "browser-burst",
                  "browser-refresh", "browser-cache-age", "browser-verdict"]) {
  browser[id] = document.getElementById(id);
}
let browserFilter = "";
let browserTypeFilter = "";
let browserStateFilter = "";
let browserExpandedKey = null;
let latestSnapRaw = null;

function browserIsActive() {
  return document.getElementById("view-browser")?.classList.contains("active");
}

function seenTs(r, field) {
  const raw = r[field];
  if (raw) {
    const t = Date.parse(raw);
    if (!isNaN(t)) return t;
  }
  return null;
}

function shortTime(ts) {
  const d = new Date(ts);
  if (isNaN(d.getTime())) return "—";
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function isRemoved(r) {
  return r.gone === true || !!r.removed_at;
}

function browserPassesFilters(r) {
  if (browserTypeFilter && r.service_type !== browserTypeFilter) return false;
  if (browserStateFilter === "live" && isRemoved(r)) return false;
  if (browserStateFilter === "removed" && !isRemoved(r)) return false;
  if (browserFilter) {
    const hay = `${r.instance_name || ""} ${r.name || ""} ${r.service_type} ${r.host || ""} ${r.ip || ""}`.toLowerCase();
    if (!hay.includes(browserFilter)) return false;
  }
  return true;
}

function browserDetail(r) {
  const div = document.createElement("div");
  div.className = "row-detail";
  const meta = typeLabels.get(r.service_type);
  const lines = [];
  lines.push(`type ${r.service_type}${meta?.label ? " — " + meta.label : ""}`);
  if (meta?.description) lines.push(meta.description);
  if (r.host) lines.push(`host ${r.host.replace(/\.$/, "")}`);
  if (r.ip) lines.push(`address ${r.ip}`);
  if (r.port != null) lines.push(`port ${r.port}`);
  lines.push(`resolved: ${r.resolved ? "yes" : "address not resolved yet"}`);
  const first = seenTs(r, "first_seen");
  if (first != null) lines.push(`first seen ${shortTime(first)}`);
  if (r.removed_at) lines.push(`withdrawn ${r.removed_at}`);
  for (const [k, v] of Object.entries(r.txt ?? {})) lines.push(`TXT ${k} = ${v}`);
  if (!Object.keys(r.txt ?? {}).length) lines.push("no TXT records");
  div.textContent = lines.join("  ·  ");
  return div;
}

function browserRow(r) {
  const k = key(r);
  const removed = isRemoved(r);
  const node = document.createElement("div");
  node.className = "row browser" + (removed ? " removed" : "") + (isFamily(r) ? " family" : "");
  const lastTs = seenTs(r, "last_seen") ?? r.seenAt ?? Date.now();
  const firstTs = seenTs(r, "first_seen");
  node.innerHTML =
    `<div class="med-mini${isFamily(r) ? " family" : ""}">${isFamily(r) ? "◆" : "·"}</div>` +
    `<div class="row-tool">${escapeHtml(typeLabel(r.service_type))}</div>` +
    `<div class="row-activity">${escapeHtml(r.instance_name || r.name)}${removed ? ' <span class="sub">· withdrawn</span>' : ""}</div>` +
    `<div class="row-client mono">${escapeHtml(r.host ? r.host.replace(/\.$/, "") + (r.port ? ":" + r.port : "") : (r.ip || "—"))}</div>` +
    `<div class="row-dur" title="${firstTs != null ? escapeHtml(new Date(firstTs).toISOString()) : ""}">${firstTs != null ? escapeHtml(shortTime(firstTs)) : "—"}</div>` +
    `<div class="row-cap">${escapeHtml(agoText(lastTs))}</div>`;
  node.addEventListener("click", () => {
    browserExpandedKey = browserExpandedKey === k ? null : k;
    renderBrowser();
  });
  if (r.resolved && r.port) node.append(passageButton(r));
  node.append(starButton(r));
  if (!isFamily(r)) {
    const invite = document.createElement("button");
    invite.className = "row-remove trust-stranger";
    invite.type = "button";
    invite.textContent = "Trust…";
    invite.title = "This announcer holds no invite from the pond — open the grant ceremony";
    invite.addEventListener("click", (e) => {
      e.stopPropagation();
      trustStranger(r.instance_name || r.name || r.service_type);
    });
    node.append(invite);
  }
  if (browserExpandedKey === k) node.append(browserDetail(r));
  return node;
}

// Deaf-detection verdict (WP5 / D6): the daemon measures bursts vs answers;
// this turns those numbers into one honest sentence. A recent burst that
// heard nothing is the firewall verdict — announcing is fine, hearing is the
// half mDNS firewalls eat. A stale silent burst is judged inconclusive.
function deafVerdict(burst) {
  if (!burst || burst.bursts_sent === 0) {
    return { tone: "info", text: "No query burst has gone out yet — press Burst to ask the pond a question." };
  }
  const age = burst.last_burst_age_secs;
  if (burst.last_burst_answers > 0) {
    return { tone: "ok", text: `Hearing the pond: the last burst (${age ?? "?"}s ago) heard ${burst.last_burst_answers} answer${burst.last_burst_answers === 1 ? "" : "s"}.` };
  }
  if (age != null && age <= 120) {
    return { tone: "bad", text: `Deaf? The last burst (${age}s ago) heard nothing. Announcing without hearing usually means a firewall (mDNS udp 5353), not a quiet network.` };
  }
  return { tone: "warn", text: `The last burst (${age ?? "?"}s ago) heard nothing, but it is stale — burst again to re-test.` };
}

function renderBrowser() {
  const host = browser["browser-queue"];
  if (!host) return;
  const verdictNode = browser["browser-verdict"];
  if (verdictNode) {
    const v = deafVerdict(latestSnapRaw?.burst);
    verdictNode.hidden = false;
    verdictNode.textContent = v.text;
    verdictNode.className = "action-note verdict " + v.tone;
  }
  host.textContent = "";
  const all = [...instances.values()].filter(browserPassesFilters);
  all.sort((a, b) => {
    const la = seenTs(a, "last_seen") ?? a.seenAt ?? 0;
    const lb = seenTs(b, "last_seen") ?? b.seenAt ?? 0;
    return lb - la;
  });
  for (const r of all) host.append(browserRow(r));
  if (!all.length) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = streamState === "offline"
      ? "The daemon's browser cache is unreachable right now — nothing is invented here."
      : "Nothing heard yet — announcements appear here as the daemon hears them.";
    host.append(empty);
  }
  browser["browser-count"].textContent = `${all.length} announcement${all.length === 1 ? "" : "s"}`;
  browser["browser-types"].textContent = String(new Set(all.map((r) => r.service_type)).size);
  browser["browser-instances"].textContent = String(all.filter((r) => !isRemoved(r)).length);
  const age = latestSnapRaw?.cache_age_secs;
  browser["browser-cache-age"].textContent = age == null ? "" : `cache ${age}s old`;
  browserRefreshTypes();
}

function browserRefreshTypes() {
  const select = browser["browser-type"];
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
  if (select.value !== current) { select.value = ""; browserTypeFilter = ""; }
}

document.getElementById("browser-filter")?.addEventListener("input", (e) => {
  browserFilter = e.target.value.trim().toLowerCase();
  renderBrowser();
});
browser["browser-type"]?.addEventListener("change", (e) => {
  browserTypeFilter = e.target.value;
  renderBrowser();
});
browser["browser-state"]?.addEventListener("change", (e) => {
  browserStateFilter = e.target.value;
  renderBrowser();
});
browser["browser-refresh"]?.addEventListener("click", () => seedSnapshot());
browser["browser-burst"]?.addEventListener("click", async () => {
  setFact(el["t-stream"], "pinging…");
  if (!invoke) {
    browser["browser-count"].textContent = "burst needs the desktop workbench";
    return;
  }
  try {
    const result = await invoke("discover_ping");
    setFact(el["t-stream"], `burst across ${result.types_known ?? 0} type(s)`, "ok");
    setTimeout(seedSnapshot, 1500);
  } catch (error) {
    setFact(el["t-stream"], String(error), "down");
  }
});

// ── Cross-host diff (cycle-1 WP4) ────────────────────────────────────
// The unique capability: koi has daemons on every machine, so "why can't
// forge see the printer" has an answer — fetch two nodes' raw snapshots and
// show three buckets: what both see, what only A sees, what only B sees.
// A multicast partition, made visible. The diff compares daemon-declared
// state only; an unreachable node is an honest error, never half-data.
const diff = {};
for (const id of ["diff-a", "diff-b", "diff-go", "diff-note", "diff-results",
                  "diff-node-name", "diff-node-address", "diff-node-add"]) {
  diff[id] = document.getElementById(id);
}

function diffNodes() {
  let nodes = [];
  try { nodes = JSON.parse(localStorage.getItem("koi-diff-nodes") || "[]"); } catch (_) {}
  return Array.isArray(nodes) ? nodes : [];
}

function diffSaveNodes(nodes) {
  localStorage.setItem("koi-diff-nodes", JSON.stringify(nodes));
}

function diffRefreshSelects() {
  const nodes = diffNodes();
  for (const [id, extra] of [["diff-a", { value: "", textContent: "This machine" }],
                             ["diff-b", null]]) {
    const select = diff[id];
    if (!select) continue;
    const current = select.value;
    select.replaceChildren();
    if (extra) select.append(Object.assign(document.createElement("option"), extra));
    for (const n of nodes) {
      select.append(Object.assign(document.createElement("option"),
        { value: `${n.address}:${n.port}`, textContent: `${n.name} (${n.address}:${n.port})` }));
    }
    select.value = current;
    if (select.value !== current) select.value = select.firstElementChild?.value ?? "";
  }
}

// The pure core: three buckets over (type, name) keys. Both rows carry the
// side that saw them most recently so the pane can show last-heard.
function diffInstances(a, b) {
  const index = (list) => {
    const map = new Map();
    for (const r of list ?? []) {
      if (isRemoved(r)) continue; // withdrawn announcements are not "seen"
      map.set(key(r), r);
    }
    return map;
  };
  const ma = index(a);
  const mb = index(b);
  const both = [];
  const onlyA = [];
  const onlyB = [];
  for (const [k, r] of ma) {
    if (mb.has(k)) both.push([r, mb.get(k)]);
    else onlyA.push(r);
  }
  for (const [k, r] of mb) {
    if (!ma.has(k)) onlyB.push(r);
  }
  return { both, onlyA, onlyB };
}

function diffRow(r) {
  const div = document.createElement("div");
  div.className = "stream-row";
  const line = document.createElement("span");
  line.className = "line";
  const where = r.host ? r.host.replace(/\.$/, "") + (r.port ? ":" + r.port : "") : (r.ip || "");
  line.textContent = `${typeLabel(r.service_type)} — ${r.instance_name || r.name}` +
    (where ? ` at ${where}` : "");
  div.append(line);
  return div;
}

function diffBucket(host, label, rows) {
  if (!rows.length) return;
  const head = document.createElement("div");
  head.className = "hap-group-head";
  head.textContent = `${label} (${rows.length})`;
  host.append(head);
  for (const r of rows) host.append(diffRow(r));
}

async function diffFetchNode(selectValue) {
  if (selectValue === "" || selectValue == null) {
    const snap = await fetchDiscoverSnapshot();
    return snap;
  }
  const [address, portRaw] = String(selectValue).split(":");
  const port = Number(portRaw);
  if (!address || !Number.isInteger(port)) throw new Error("node looks wrong");
  if (!invoke) throw new Error("cross-host reads need the desktop workbench");
  return await invoke("daemon_get", { address, port, path: "/v1/mdns/browser/snapshot" });
}

async function runDiff() {
  const note = diff["diff-note"];
  const results = diff["diff-results"];
  if (!invoke) {
    note.textContent = "The cross-host diff reads sibling daemons directly — it needs the desktop workbench.";
    return;
  }
  diff["diff-go"].disabled = true;
  note.textContent = "Reading both ponds…";
  results.textContent = "";
  try {
    const [a, b] = await Promise.allSettled([
      diffFetchNode(diff["diff-a"].value),
      diffFetchNode(diff["diff-b"].value),
    ]);
    if (a.status === "rejected" || b.status === "rejected") {
      const errs = [];
      if (a.status === "rejected") errs.push(`node A: ${a.reason}`);
      if (b.status === "rejected") errs.push(`node B: ${b.reason}`);
      note.textContent = errs.join(" · ");
      return;
    }
    const out = diffInstances(a.value?.instances, b.value?.instances);
    const nameA = diff["diff-a"].selectedOptions[0]?.textContent ?? "A";
    const nameB = diff["diff-b"].selectedOptions[0]?.textContent ?? "B";
    note.textContent = `${nameA} vs ${nameB} — both: ${out.both.length}, only A: ${out.onlyA.length}, only B: ${out.onlyB.length}`;
    diffBucket(results, "Seen by both", out.both.map(([r]) => r));
    diffBucket(results, "Only " + nameA, out.onlyA);
    diffBucket(results, "Only " + nameB, out.onlyB);
    if (!out.both.length && !out.onlyA.length && !out.onlyB.length) {
      const empty = document.createElement("div");
      empty.className = "empty";
      empty.textContent = "Neither node has any active announcements.";
      results.append(empty);
    }
  } finally {
    diff["diff-go"].disabled = false;
  }
}

diff["diff-go"]?.addEventListener("click", runDiff);
diff["diff-node-add"]?.addEventListener("click", () => {
  const name = diff["diff-node-name"].value.trim();
  const addrRaw = diff["diff-node-address"].value.trim();
  const [address, portRaw] = addrRaw.split(":");
  const port = Number(portRaw);
  if (!name || !address || !Number.isInteger(port) || port <= 0) {
    diff["diff-note"].textContent = "Add a node as name + address:port (e.g. brook · 192.168.1.44:5641).";
    return;
  }
  const nodes = diffNodes().filter((n) => n.address !== address || n.port !== port);
  nodes.push({ name, address, port });
  diffSaveNodes(nodes);
  diff["diff-node-name"].value = "";
  diff["diff-node-address"].value = "";
  diffRefreshSelects();
  diff["diff-note"].textContent = `${name} added.`;
});

diffRefreshSelects();

// ── Passage (cycle-1 WP7): one click to the working endpoint ─────────
// Composed from what the daemon resolved — never from raw TXT alone. Only
// http(s) is offered and the Rust side refuses anything else, so an
// announcement can open a page but never execute anything.
function composeUrl(r) {
  const host = String(r.host || r.ip || "").replace(/\.$/, "").trim();
  const port = Number(r.port);
  if (!host || !Number.isInteger(port) || port <= 0 || port > 65535) return null;
  if (!/^[a-zA-Z0-9.\-:]+$/.test(host)) return null;
  const scheme = (r.txt?.scheme || (r.txt?.tls === "1" ? "https" : "http")).toLowerCase();
  if (scheme !== "http" && scheme !== "https") return null;
  return `${scheme}://${host}:${port}/`;
}

function openEndpoint(r) {
  const url = composeUrl(r);
  if (!url) { note("No usable endpoint on this inhabitant yet.", true); return; }
  if (!invoke) { note(`Endpoint: ${url}`, false); return; }
  invoke("open_url", { url })
    .then(() => note(`Opened ${url} in your browser.`))
    .catch((error) => note(String(error), true));
}

function passageButton(r) {
  const btn = document.createElement("button");
  btn.className = "row-open";
  btn.type = "button";
  btn.textContent = "Open";
  btn.title = "Open the working endpoint in your browser";
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    openEndpoint(r);
  });
  return btn;
}

// ── Trust pane (cycle-1 WP6): ceremony + audit ───────────────────────
// Friction is the feature: the grant ceremony places BOTH fingerprints side
// by side and the invite stays locked until the operator states they compared
// them character by character. Revocation is armed (two clicks), labeled
// grantor → grantee, and every action lands in the daemon's audit trail.
const trust = {};
for (const id of ["trust-note", "trust-role", "trust-detail", "trust-members-count",
                  "trust-members", "invite-host", "invite-ttl", "invite-mint",
                  "ceremony", "grant-label", "grant-consequence", "fp-ca", "fp-invite",
                  "fp-compared", "invite-token", "invite-expires", "invite-note",
                  "audit-log", "audit-refresh", "trust-refresh"]) {
  trust[id] = document.getElementById(id);
}
let lastCertmeshStatus = null;

// The invite code is `<secret>.<ca_fingerprint>` (ADR-017 F3) — the part
// after the first separator is the fingerprint the joiner pins. Pure + tested.
function fpPinOf(inviteToken) {
  const s = String(inviteToken ?? "");
  const i = s.indexOf(".");
  return i >= 0 && i < s.length - 1 ? s.slice(i + 1) : null;
}

// The honest role this machine plays in the pond's trust.
function trustRoleOf(status) {
  if (!status) return { role: "unknown", detail: "The daemon did not answer." };
  if (!status.ca_initialized) {
    return {
      role: "open node",
      detail: "This machine holds no trust role yet — it has no CA and no identity. " +
        "Create one with `koi certmesh create`, or join an existing CA with `koi certmesh join`.",
    };
  }
  if (status.ca_locked) {
    return { role: "CA (locked)", detail: "The CA is locked — unlock with `koi certmesh unlock` to mint invites." };
  }
  return {
    role: "CA — the grantor",
    detail: "This machine signs the pond's identities. Enrollment is " +
      (status.enrollment_open ? "OPEN (new members can join)" : "closed (invites only)") + ".",
  };
}

function trustNote(text, isError) {
  if (!trust["trust-note"]) return;
  trust["trust-note"].textContent = text || "";
  trust["trust-note"].className = "action-note" + (isError ? " error" : "");
}

function shortFp(fp) {
  const s = String(fp ?? "");
  return s.length > 20 ? s.slice(0, 10) + "…" + s.slice(-8) : s;
}

function renderTrustMembers(status) {
  const host = trust["trust-members"];
  host.textContent = "";
  const members = status?.members ?? [];
  trust["trust-members-count"].textContent = `${members.length} member${members.length === 1 ? "" : "s"}`;
  if (!status?.ca_initialized) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "No CA on this machine — the roster would live here.";
    host.append(empty);
    return;
  }
  if (!members.length) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "No members yet — mint an invite below to grant the first one.";
    host.append(empty);
    return;
  }
  for (const m of members) {
    const node = document.createElement("div");
    node.className = "row trust-member";
    node.innerHTML =
      `<div class="med-mini family">◆</div>` +
      `<div class="row-tool">${escapeHtml(m.hostname)}</div>` +
      `<div class="row-activity">${escapeHtml(m.role)} · ${escapeHtml(m.status)}` +
      `<span class="sub"> · cert ${escapeHtml(shortFp(m.cert_fingerprint))}</span></div>` +
      `<div class="row-client">${escapeHtml(String(m.cert_expires ?? "").slice(0, 10))}</div>` +
      `<div class="row-cap"></div>`;
    // The danger action is a real element (two-step armed), not innerHTML.
    const revoke = document.createElement("button");
    revoke.className = "row-remove danger-button";
    revoke.type = "button";
    revoke.textContent = "Revoke";
    node.append(revoke);
    revoke.addEventListener("click", async () => {
      if (!invoke) { trustNote("Revocation needs the desktop workbench.", true); return; }
      if (revoke.dataset.armed !== "yes") {
        revoke.dataset.armed = "yes";
        revoke.textContent = `Really revoke ${m.hostname}?`;
        trustNote(`Revoking ${m.hostname} stops it from speaking our TLS. Click again to confirm.`, true);
        setTimeout(() => { revoke.dataset.armed = ""; revoke.textContent = "Revoke"; }, 6000);
        return;
      }
      try {
        const out = await invoke("certmesh_revoke", { hostname: m.hostname,
          reason: "revoked from the workbench" });
        trustNote(out?.revoked
          ? `${m.hostname} revoked — the pond retracts its trust.`
          : `${m.hostname} could not be revoked.`, !out?.revoked);
        refreshTrust();
      } catch (error) {
        trustNote(String(error), true);
      }
    });
    host.append(node);
  }
}

function renderTrust(status, diagnose) {
  const role = trustRoleOf(status);
  trust["trust-role"].textContent = role.role;
  trust["trust-detail"].textContent = role.detail;
  lastCertmeshStatus = status;
  renderTrustMembers(status);
}

async function refreshTrust() {
  if (!invoke) {
    trust["trust-role"].textContent = "Trust lives in the desktop workbench";
    trust["trust-detail"].textContent =
      "The phone view can read the pond's public state; the ceremony and audit stay local.";
    return;
  }
  let status = null;
  try {
    status = await invoke("certmesh_status");
  } catch (error) {
    trust["trust-role"].textContent = "The daemon did not answer";
    trust["trust-detail"].textContent = String(error);
    return;
  }
  let diagnose = null;
  try { diagnose = await invoke("certmesh_diagnose"); } catch (_) {}
  renderTrust(status, diagnose);
}

function closeCeremony() {
  trust["ceremony"].hidden = true;
  trust["fp-compared"].checked = false;
}

async function mintInvite() {
  if (!invoke) { trustNote("The ceremony needs the desktop workbench.", true); return; }
  const hostname = trust["invite-host"].value.trim();
  if (!hostname) { trustNote("Name the member you are granting to.", true); return; }
  const ttlRaw = trust["invite-ttl"].value.trim();
  const ttl = ttlRaw === "" ? null : Number(ttlRaw);
  if (ttl !== null && (!Number.isFinite(ttl) || ttl <= 0)) {
    trustNote("Invite TTL must be a positive number of minutes.", true);
    return;
  }
  trustNote("Minting…");
  try {
    const out = await invoke("certmesh_invite", { hostname, ttl_mins: ttl });
    const tokenStr = String(out?.token ?? "");
    const pin = fpPinOf(tokenStr);
    const caFp = lastCertmeshStatus?.ca_fingerprint ?? "";
    trust["grant-label"].textContent = `grant: this CA (${shortFp(caFp)}) → ${hostname}`;
    trust["grant-consequence"].textContent =
      `Signing gives ${hostname} a pond identity — valid TLS among members, ` +
      "revocable anytime from this pane. The invite is single-use and expires.";
    trust["fp-ca"].textContent = caFp || "(the daemon did not report a CA fingerprint)";
    trust["fp-invite"].textContent = pin ?? "(no fingerprint pin found in the invite)";
    trust["invite-token"].textContent = tokenStr;
    trust["invite-expires"].textContent = String(out?.expires_at ?? "");
    trust["ceremony"].hidden = false;
    trustNote("Invite minted — but the ceremony is not done until the fingerprints are compared.");
    refreshTrust();
  } catch (error) {
    trustNote(String(error), true);
  }
}

trust["invite-mint"]?.addEventListener("click", mintInvite);
trust["refresh"] = trust["trust-refresh"];
trust["trust-refresh"]?.addEventListener("click", refreshTrust);
trust["fp-compared"]?.addEventListener("change", (e) => {
  // The invite is visible either way (hiding it would fake security) — the
  // checkbox gates the *guidance*, acknowledging the comparison happened.
  trust["invite-note"].textContent = e.target.checked
    ? "Compared. Hand the invite to the member; on their machine: koi certmesh join <ca-endpoint> --invite <token>"
    : "";
});

async function refreshAudit() {
  if (!invoke) {
    trust["audit-log"].textContent = "The audit trail stays in the desktop workbench.";
    return;
  }
  try {
    const out = await invoke("certmesh_log");
    const entries = String(out?.entries ?? "");
    trust["audit-log"].textContent = entries || "No audit entries yet — the diary starts with the first grant.";
  } catch (error) {
    trust["audit-log"].textContent = "Audit unavailable: " + error;
  }
}
trust["audit-refresh"]?.addEventListener("click", refreshAudit);

// Stranger flow (E6): an unknown announcer in the Browser pane arrives here
// with its name prefilled. Nothing is granted without the full ceremony.
function trustStranger(name) {
  if (!trust["invite-host"]) return;
  trust["invite-host"].value = String(name ?? "");
  trustNote(`${name} announced itself and holds no invite from this pond. If it is yours, ` +
    "mint it an invite; if it is not, doing nothing grants it nothing.");
  gotoView("trust");
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
  // The feed is shared (At a glance recounts it); this pane renders the
  // streaming hero: sentences newest-first, watched pins, flapping as ONE
  // row + count, and the designed quiet state when nothing needs anyone.
  const heroWord = document.getElementById("hero-word");
  const heroDetail = document.getElementById("hero-detail");
  const heroPins = document.getElementById("hero-pins");
  const heroAttention = document.getElementById("hero-attention");
  const streamNode = document.getElementById("status-stream");
  const statusHero = document.getElementById("status-hero");

  function renderStatusHero() {
    heroPins.textContent = "";
    for (const subject of feed.watched) {
      const b = document.createElement("button");
      b.className = "hero-pin";
      b.type = "button";
      b.textContent = "📌 " + subject;
      b.addEventListener("click", () => feedUnpin(subject));
      heroPins.appendChild(b);
    }
    const attention = feed.degraded;
    heroAttention.textContent = attention.join(" · ");
    if (attention.length) {
      heroWord.textContent = "Needs you";
      heroDetail.textContent = attention[0];
      statusHero.dataset.tone = "warn";
    } else if (feed.rows.length || feed.watched.size) {
      heroWord.textContent = "Pond is living";
      heroDetail.textContent = feed.watched.size
        ? "watching " + feed.watched.size + " subject(s); everything else flows past"
        : "events stream as they happen";
      delete statusHero.dataset.tone;
    } else {
      heroWord.textContent = "All quiet";
      heroDetail.textContent = "nothing needs you";
      delete statusHero.dataset.tone;
    }
  }

  function renderStatusStream() {
    streamNode.textContent = "";
    for (const row of feed.rows) {
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
      pin.className = "pin" + (feed.watched.has(row.subject) ? " pinned" : "");
      pin.type = "button";
      pin.title = feed.watched.has(row.subject) ? "unpin" : "pin to the hero";
      pin.textContent = feed.watched.has(row.subject) ? "★" : "☆";
      pin.addEventListener("click", (e) => {
        e.stopPropagation();
        feed.watched.has(row.subject) ? feedUnpin(row.subject) : feedPin(row.subject);
      });
      div.append(ago, line, pin);
      // Click-through is the view registry's target (B3): the sentence
      // already carries the pane that owns this subject.
      div.addEventListener("click", () => gotoView(row.target));
      streamNode.appendChild(div);
    }
    renderStatusHero();
  }

  feed.listeners.add(renderStatusStream);
  renderStatusStream();

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
    feedAdmit({ kind, line: s.line, tone: s.tone, target: s.target,
                subject: KS.subjectOf(kind, payload.data) });
  });
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
    const snap = BROWSER_MODE
      ? await fetch("/v1/dns/entries").then((r) => {
          if (!r.ok) throw new Error(String(r.status));
          return r.json();
        })
      : await invoke("dns_entries");
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
