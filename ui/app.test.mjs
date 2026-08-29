// Workbench JS tests (cycle 1): the feed, the Status hero admission, the
// At-a-glance page, and the sentence registry — the pure layers, exercised
// in a DOM stub without a browser. Run: node --test ui/app.test.mjs
//
// boot() awaits a macrotask so the boot-time rejections (fetch, absent
// daemon) settle and their re-renders complete before assertions run —
// the same states a real no-daemon boot passes through.

import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { loadWorkbench, probe } from "./dom-stub.mjs";

const UI_ROOT = fileURLToPath(new URL(".", import.meta.url));

async function boot() {
  const { ctx, document } = loadWorkbench(UI_ROOT);
  // The stub's getElementById creates detached elements; wire the one parent
  // relationship the code reads back (glance-word → its .strip container).
  const strip = document.getElementById("glance-hero");
  strip.classList.add("strip");
  strip.appendChild(document.getElementById("glance-word"));
  await new Promise((r) => setImmediate(r));
  return { ctx, document };
}

const settle = () => new Promise((r) => setImmediate(r));

test("sentences: every registry kind writes a sentence with tone and target", async () => {
  const { ctx } = await boot();
  const kinds = probe(ctx, "Object.keys(window.KoiSentences.REGISTRY)");
  for (const kind of kinds) {
    const s = probe(ctx, `window.KoiSentences.sentenceFor(${JSON.stringify(kind)}, {name:"x"})`);
    assert.ok(s.line && s.line.length > 0, `${kind} writes a line`);
    assert.ok(["info", "good", "warn", "bad"].includes(s.tone), `${kind} tone is known`);
    assert.ok(s.target, `${kind} targets a view`);
  }
  // unknown kinds still render, into Status (never silence)
  const s = probe(ctx, `window.KoiSentences.sentenceFor("brand.new.kind", {})`);
  assert.equal(s.target, "status");
  assert.equal(s.line, "brand.new.kind event");
});

test("feed: a restart storm is ONE flapping row with a count", async () => {
  const { ctx } = await boot();
  for (const kind of ["runtime.started", "runtime.stopped", "runtime.started", "runtime.stopped"]) {
    probe(ctx, `feedAdmit({kind:"${kind}", line:"container service forge ${kind.endsWith("started") ? "started" : "stopped"}", tone:"info", target:"discover", subject:"container:forge"})`);
  }
  const row = probe(ctx, `feed.rows.length + "|" + feed.rows[0].line`);
  assert.equal(row, "1|container service forge started — 4 times in the last 90s");
});

test("feed: watched pins persist and unpin", async () => {
  const { ctx } = await boot();
  probe(ctx, `feedPin("container:forge")`);
  assert.equal(probe(ctx, `[...feed.watched].join(",")`), "container:forge");
  assert.equal(
    probe(ctx, `localStorage.getItem("koi-watched")`),
    JSON.stringify(["container:forge"]),
  );
  probe(ctx, `feedUnpin("container:forge")`);
  assert.equal(probe(ctx, "feed.watched.size"), 0);
});

test("glance: hero is attention → watched → quiet, in that order", async () => {
  const { ctx, document } = await boot();
  const word = () => document.getElementById("glance-word").textContent;

  // quiet first
  assert.equal(word(), "All quiet");
  assert.equal(document.getElementById("glance-detail").textContent,
    "nothing needs you right now");

  // a warn event earns the hero
  probe(ctx, `feedAdmit({kind:"certmesh.revoked", line:"rogue was revoked", tone:"warn", target:"status", subject:"trust:rogue"})`);
  assert.equal(word(), "Needs you");
  assert.equal(document.getElementById("glance-hero").getAttribute("data-tone"), "warn");

  // attention clears → watched holds the hero even with an empty stream
  probe(ctx, `feed.rows.length = 0; feedNotify();`);
  probe(ctx, `feedPin("container:forge")`);
  assert.equal(word(), "Pond is living");
  assert.equal(document.getElementById("glance-hero").getAttribute("data-tone"), null);

  // unpin → quiet again
  probe(ctx, `feedUnpin("container:forge"); feedNotify();`);
  assert.equal(word(), "All quiet");
});

test("glance: happenings dedupe consecutive same-subject rows with a count", async () => {
  const { ctx, document } = await boot();
  probe(ctx, `
    feedAdmit({kind:"dns.updated", line:"DNS: app.internal → 10.0.0.5", tone:"info", target:"dns", subject:"dns:app.internal"});
    feedAdmit({kind:"dns.updated", line:"DNS: app.internal → 10.0.0.6", tone:"info", target:"dns", subject:"dns:app.internal"});
  `);
  const lines = [...document.getElementById("glance-happenings").children]
    .filter((n) => n.className === "stream-row")
    .map((n) => n.querySelector(".line").textContent);
  assert.equal(lines.length, 1, "one row for the repeated subject");
  assert.match(lines[0], /×2/);
  assert.match(lines[0], /10\.0\.0\.6/, "latest sentence wins");
});

test("glance: rows carry the registry target (click-through data)", async () => {
  const { ctx } = await boot();
  probe(ctx, `feedAdmit({kind:"dns.updated", line:"DNS: a → b", tone:"info", target:"dns", subject:"dns:a"})`);
  assert.equal(probe(ctx, "feed.rows[0].target"), "dns");
});

test("glance: digest shows the honest present until happenings exist", async () => {
  const { ctx, document } = await boot();
  const digest = document.getElementById("glance-digest");
  assert.equal(digest.hidden, false);
  assert.match(digest.textContent, /The pond right now: 0 inhabitants/);
  assert.match(digest.textContent, /daemon down/);
  probe(ctx, `feedAdmit({kind:"mdns.found", line:"printer appeared", tone:"info", target:"discover", subject:"announcement:printer"})`);
  assert.equal(digest.hidden, true, "real happenings retire the digest");
});

test("glance: rows observed while the pane was closed group as 'since you last looked'", async () => {
  const { ctx, document } = await boot();
  const glanceView = document.getElementById("view-glance");
  // open pane: everything seen
  glanceView.classList.add("active");
  probe(ctx, `feedAdmit({kind:"mdns.found", line:"first appeared", tone:"info", target:"discover", subject:"announcement:first"})`);
  assert.ok(probe(ctx, "glanceSince") > 0, "open pane marks seen");
  // close pane: new events are unseen (clock advances so it is strictly newer)
  glanceView.classList.remove("active");
  probe(ctx, `__advanceClock(1000);
    feedAdmit({kind:"mdns.found", line:"second appeared", tone:"info", target:"discover", subject:"announcement:second"})`);
  glanceView.classList.add("active");
  probe(ctx, "feedNotify()");
  await settle();
  const heads = [...document.getElementById("glance-happenings").children]
    .filter((n) => n.className === "hap-group-head")
    .map((n) => n.textContent);
  assert.deepEqual(heads, ["Since you last looked", "This session"]);
});

test("stream cap: the feed stays bounded at 50 rows", async () => {
  const { ctx } = await boot();
  probe(ctx, `
    for (let i = 0; i < 60; i++) {
      feedAdmit({kind:"mdns.found", line:"s" + i, tone:"info", target:"discover", subject:"announcement:s" + i});
    }
  `);
  assert.equal(probe(ctx, "feed.rows.length"), 50);
});
