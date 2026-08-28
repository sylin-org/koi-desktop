// Koi sentences (ADR-035 / cycle-1 WP0): every daemon event becomes one honest
// line. Pure functions only — no DOM, no network, no Tauri. app.js reads this
// through window.KoiSentences; the view registry maps kinds to the pane that
// owns them (unknown kinds fall back to Status, never to silence).
//
// The kinds below mirror koi-dashboard/src/forward.rs exactly. A new daemon
// kind without an entry still renders (generic fallback) — but add it here so
// the sentence is written in the product's voice.
(function () {
  "use strict";

  // kind → the pane that owns the subject.
  const VIEWS = {
    discover: "discover",
    dns: "dns",
    status: "status",
    about: "about",
  };

  const REGISTRY = {
    "mdns.found": { view: "discover", tone: "info" },
    "mdns.resolved": { view: "discover", tone: "info" },
    "mdns.removed": { view: "discover", tone: "info" },
    "health.changed": { view: "status", tone: "info" },
    "dns.updated": { view: "dns", tone: "info" },
    "dns.removed": { view: "dns", tone: "info" },
    "dns.txt_updated": { view: "dns", tone: "info" },
    "dns.txt_removed": { view: "dns", tone: "info" },
    "certmesh.joined": { view: "status", tone: "good" },
    "certmesh.revoked": { view: "status", tone: "warn" },
    "certmesh.destroyed": { view: "status", tone: "bad" },
    "certmesh.cert_renewed": { view: "status", tone: "good" },
    "certmesh.cert_expiring_soon": { view: "status", tone: "warn" },
    "certmesh.cert_renewal_failed": { view: "status", tone: "bad" },
    "certmesh.bundle_updated": { view: "status", tone: "info" },
    "proxy.updated": { view: "discover", tone: "info" },
    "proxy.removed": { view: "discover", tone: "info" },
    "runtime.started": { view: "discover", tone: "good" },
    "runtime.stopped": { view: "discover", tone: "info" },
    "runtime.updated": { view: "discover", tone: "info" },
    "runtime.disconnected": { view: "status", tone: "warn" },
    "runtime.reconnected": { view: "discover", tone: "good" },
  };

  function shortType(t) {
    return String(t ?? "")
      .replace(/^_/, "")
      .replace(/\._tcp\.local\.$/, "")
      .replace(/\._tcp$/, "")
      .replace(/\._udp$/, "")
      .replace(/\.local\.?$/, "");
  }

  function label(data) {
    return (
      data.name || data.instance_name || data.hostname || data.service_name || ""
    );
  }

  // The stable subject a sentence is about (pinning and flapping key this).
  function subjectOf(kind, data) {
    data = data ?? {};
    if (kind.startsWith("runtime.")) return "container:" + (data.name || data.id || "?");
    if (kind.startsWith("mdns.")) return "announcement:" + (data.name || "?");
    if (kind.startsWith("dns.")) return "dns:" + (data.name || "?");
    if (kind.startsWith("certmesh.")) return "trust:" + (data.hostname || "ca");
    if (kind.startsWith("proxy.")) return "proxy:" + (data.name || "?");
    if (kind === "health.changed") return "health:" + (data.name || "?");
    return "kind:" + kind;
  }

  function sentenceFor(kind, data) {
    data = data ?? {};
    const entry = REGISTRY[kind] ?? { view: "status", tone: "info" };
    return { line: lineFor(kind, data), tone: entry.tone, target: entry.view };
  }

  function targetOf(kind) {
    const entry = REGISTRY[kind];
    return entry ? entry.view : "status";
  }

  function lineFor(kind, data) {
    const name = label(data);
    const who = name ? `${name}` : "something";
    switch (kind) {
      case "mdns.found":
        return `${who} appeared on the network (${shortType(data.service_type)})`;
      case "mdns.resolved":
        return `${who} resolved to ${data.ip || "an address"}:${
          data.port ?? "?"
        }`;
      case "mdns.removed":
        return `${shortType(data.service_type)} announcement from ${who} went away`;
      case "health.changed":
        return `health check ${data.name} is now ${data.status}`;
      case "dns.updated":
        return `DNS: ${data.name} → ${data.ip}`;
      case "dns.removed":
        return `DNS entry ${data.name} removed`;
      case "dns.txt_updated":
        return `DNS TXT record for ${data.name} changed`;
      case "dns.txt_removed":
        return `DNS TXT record for ${data.name} removed`;
      case "certmesh.joined":
        return `${data.hostname} joined the pond's trust (cert granted)`;
      case "certmesh.revoked":
        return `${data.hostname} was revoked — it no longer speaks our TLS`;
      case "certmesh.destroyed":
        return "the certmesh CA was destroyed";
      case "certmesh.cert_renewed":
        return "the daemon certificate renewed";
      case "certmesh.cert_expiring_soon":
        return `the daemon certificate expires in ${data.days_left} days`;
      case "certmesh.cert_renewal_failed":
        return `certificate renewal failed (${data.consecutive_failures} consecutive): ${data.reason}`;
      case "certmesh.bundle_updated":
        return data.self_revoked
          ? "trust bundle updated — self-revocation is in effect"
          : "trust bundle updated";
      case "proxy.updated":
        return `proxy for ${who} is live`;
      case "proxy.removed":
        return `proxy for ${who} went away`;
      case "runtime.started":
        return `container service ${who} started`;
      case "runtime.stopped":
        return `container service ${who} stopped`;
      case "runtime.updated":
        return `container service ${who} changed`;
      case "runtime.disconnected":
        return `Docker (${data.backend}) disconnected: ${data.reason ?? "unknown reason"}`;
      case "runtime.reconnected":
        return `Docker (${data.backend}) reconnected`;
      default:
        return `${kind} event`;
    }
  }

  window.KoiSentences = { sentenceFor, subjectOf, targetOf, REGISTRY };
})();
