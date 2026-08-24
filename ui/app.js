// Koi workbench, first pane: truthful daemon/posture facts, polled.
// Pure layers hold no document (Ghostlight's rule): transport and words are
// testable without a browser; only the composition root touches the DOM.

const DAEMON_ORIGIN = "http://127.0.0.1:5641";
const POLL_MS = 5000;

// ── transport ────────────────────────────────────────────────────────
async function getJson(path) {
  const response = await fetch(DAEMON_ORIGIN + path, { cache: "no-store" });
  if (!response.ok) throw new Error("http " + response.status);
  return response.json();
}

// ── words: one wording source for state, mirroring ADR-020 truthfulness ──
function postureWord(level) {
  switch (level) {
    case "open": return "open";
    case "authenticated": return "authenticated";
    case "confidential": return "confidential";
    default: return "unknown";
  }
}

// ── composition root ─────────────────────────────────────────────────
const el = {
  lamp: document.getElementById("lamp"),
  band: document.getElementById("band-word"),
  daemon: document.getElementById("f-daemon"),
  posture: document.getElementById("f-posture"),
  host: document.getElementById("f-host"),
  version: document.getElementById("f-version"),
};

function setFact(node, text, tone) {
  node.textContent = text;
  node.className = tone || "";
}

let lastSignature = "";

async function tick() {
  let snapshot;
  try {
    const [status, posture] = await Promise.all([
      getJson("/v1/status").catch(() => null),
      getJson("/v1/certmesh/posture").catch(() => null),
    ]);
    snapshot = {
      up: true,
      level: posture && posture.level ? postureWord(posture.level) : "—",
      version: status && status.version ? String(status.version) : "—",
    };
  } catch {
    snapshot = { up: false, level: "offline", version: "—" };
  }

  const signature = JSON.stringify(snapshot);
  if (signature === lastSignature) return;
  lastSignature = signature;

  el.lamp.className = snapshot.up ? "lamp up" : "lamp";
  el.band.textContent = snapshot.up
    ? "the pond is calm"
    : "the pond is quiet — no daemon on this machine";

  setFact(el.daemon, snapshot.up ? "running" : "not running", snapshot.up ? "ok" : "down");
  setFact(el.posture, snapshot.level, snapshot.level === "offline" ? "down" : "");
  if (!el.host.textContent || el.host.textContent === "…") {
    el.host.textContent = window.location.hostname || "this machine";
  }
  setFact(el.version, snapshot.version);
}

tick();
setInterval(tick, POLL_MS);
