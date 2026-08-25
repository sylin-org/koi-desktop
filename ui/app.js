// Koi workbench: Status + About. Pure layers hold no document; only this
// composition root touches the DOM.

const DAEMON_ORIGIN = "http://127.0.0.1:5641";
const POLL_MS = 5000;

const invoke = window.__TAURI__?.core?.invoke;

// ── transport ────────────────────────────────────────────────────────
async function getJson(path) {
  const response = await fetch(DAEMON_ORIGIN + path, { cache: "no-store" });
  if (!response.ok) throw new Error("http " + response.status);
  return response.json();
}

function postureWord(level) {
  switch (level) {
    case "open": return "open";
    case "authenticated": return "authenticated";
    case "confidential": return "confidential";
    default: return "unknown";
  }
}

// ── composition root ─────────────────────────────────────────────────
const el = {};
for (const id of [
  "lamp", "state-word", "state-facts",
  "service-state", "service-detail", "btn-start", "btn-run-once", "btn-stop", "action-note",
  "t-http", "t-posture", "t-version",
  "koi-card", "card-version", "f-daemon", "f-posture", "f-host", "f-version",
]) el[id] = document.getElementById(id);

let lastSignature = "";
let lastService = { installed: false, running: false };

function setFact(node, text, tone) {
  node.textContent = text;
  node.className = tone || "";
}

function note(text, isError) {
  el["action-note"].textContent = text || "";
  el["action-note"].className = "action-note" + (isError ? " error" : "");
}

async function tick() {
  let service = null;
  if (invoke) {
    try { service = await invoke("service_status"); } catch { service = null; }
  }
  const svc = service || { installed: false, running: false, detail: "" };
  lastService = svc;

  let snapshot;
  try {
    const [status, posture] = await Promise.all([
      getJson("/v1/status").catch(() => null),
      getJson("/v1/certmesh/posture").catch(() => null),
    ]);
    const raw = status && status.version ? String(status.version) : null;
    snapshot = {
      up: true,
      level: posture && posture.level ? postureWord(posture.level) : "—",
      version: raw || "—",
      short: raw ? raw.split(".").slice(0, 2).join(".") : "1.0",
    };
  } catch {
    snapshot = { up: false, level: "offline", version: "—", short: "—" };
  }

  const signature = JSON.stringify([snapshot, svc]);
  if (signature === lastSignature) return;
  lastSignature = signature;

  // header lamp — Ghostlight drives states through body classes
  document.body.classList.toggle("runtime-offline", !snapshot.up);
  el["state-word"].textContent = snapshot.up ? "Calm waters" : "Quiet pond";
  el["state-facts"].textContent = snapshot.up
    ? `http ${DAEMON_ORIGIN.replace("http://", "")} · posture ${snapshot.level}`
    : "no daemon on this machine";

  // service strip — truthful about which shape exists and who may act
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
  setFact(el["t-http"], snapshot.up ? `serving at ${DAEMON_ORIGIN}` : "no listener on 5641",
    snapshot.up ? "ok" : "down");
  setFact(el["t-posture"], snapshot.level, snapshot.up ? "" : "down");
  setFact(el["t-version"], snapshot.version);

  // about facts mirror
  setFact(el["f-daemon"], snapshot.up ? "running" : "not running", snapshot.up ? "ok" : "down");
  setFact(el["f-posture"], snapshot.level, snapshot.up ? "ok" : "down");
  setFact(el["f-version"], snapshot.version);
  el["card-version"].textContent = snapshot.short;
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
  lastSignature = ""; // force repaint on next tick
  tick();
}

el["btn-start"]?.addEventListener("click", () => act("service_start"));
el["btn-stop"]?.addEventListener("click", () => act("service_stop"));
el["btn-run-once"]?.addEventListener("click", () => act("daemon_run_once"));
document.getElementById("refresh-status")?.addEventListener("click", () => { lastSignature = ""; tick(); });

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

armCard();
tick();
setInterval(tick, POLL_MS);
