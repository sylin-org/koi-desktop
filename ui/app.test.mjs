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

// ── WP3: the Browser (raw lens) + the Discover lens ──────────────────

function seedRaw(ctx) {
  // one koi-family announcement, one non-koi (printer), one withdrawn
  probe(ctx, `
    __advanceClock(1000);
    upsertInstance({ service_type: "_koi-serve._tcp.local.", name: "sparkle", instance_name: "sparkle", host: "sparkle.internal.", ip: "192.168.1.137", port: 5641, txt: { path: "/v1/ui" }, resolved: true, first_seen: "2026-08-28T20:00:00+00:00", last_seen: new Date(Date.now() - 1000).toISOString() });
    upsertInstance({ service_type: "_ipp._tcp.local.", name: "Brother HL", instance_name: "Brother HL-L2350", host: "BRW1.internal.", ip: "192.168.1.42", port: 631, txt: { rp: "ipp/print" }, resolved: true, first_seen: "2026-08-28T20:00:00+00:00", last_seen: new Date(Date.now() - 2000).toISOString() });
    upsertInstance({ service_type: "_googlecast._tcp.local.", name: "Living Room", instance_name: "Living Room TV", host: "cast.internal.", ip: "192.168.1.50", port: 8009, txt: {}, resolved: false, removed_at: new Date(Date.now() - 3000).toISOString(), first_seen: "2026-08-28T20:00:00+00:00", last_seen: new Date(Date.now() - 3000).toISOString() });
  `);
}

test("browser: the raw lens shows non-koi announcements the curated lens hides", async () => {
  const { ctx, document } = await boot();
  seedRaw(ctx);
  probe(ctx, "renderBrowser()");
  const rows = [...document.getElementById("browser-queue").children]
    .filter((n) => n.className.startsWith("row browser"));
  assert.equal(rows.length, 3, "printer + cast + koi all visible raw");
  // discover defaults to the family lens: only the koi peer
  probe(ctx, "renderAllGroups()");
  const discoverRows = document.getElementById("discover-queue").querySelectorAll(".row");
  assert.equal(discoverRows.length, 1, "curated lens hides the printer and the cast device");
  // the "everything" lens opens the water
  probe(ctx, `document.getElementById("discover-lens").value = "all"; discoverLens = "all"; renderAllGroups();`);
  const rawRows = document.getElementById("discover-queue").querySelectorAll(".row");
  assert.equal(rawRows.length, 3);
});

test("browser: removals are shown, never hidden", async () => {
  const { ctx, document } = await boot();
  seedRaw(ctx);
  probe(ctx, "renderBrowser()");
  const removed = [...document.getElementById("browser-queue").children]
    .filter((n) => n.className.includes("removed"));
  assert.equal(removed.length, 1, "the withdrawn cast device is still visible, dimmed");
  // filter to active only
  probe(ctx, `browserStateFilter = "live"; renderBrowser();`);
  const active = [...document.getElementById("browser-queue").children]
    .filter((n) => n.className.startsWith("row browser"));
  assert.equal(active.length, 2);
});

test("browser: a row expands to its TXT records and type dictionary", async () => {
  const { ctx, document } = await boot();
  seedRaw(ctx);
  probe(ctx, `browserExpandedKey = "_ipp._tcp.local. Brother HL"; renderBrowser();`);
  const detail = document.getElementById("browser-queue").querySelector(".row-detail");
  assert.ok(detail, "the selected row expands");
  assert.match(detail.textContent, /TXT rp = ipp\/print/);
  // an expanded row of a dictionary-known type shows the friendly label
  probe(ctx, `typeLabels.set("_koi-serve._tcp.local.", { label: "Koi serve", description: "The pond's own service" });
    browserExpandedKey = "_koi-serve._tcp.local. sparkle"; renderBrowser();`);
  const koiDetail = [...document.getElementById("browser-queue").querySelectorAll(".row-detail")][0];
  assert.match(koiDetail.textContent, /Koi serve/);
  assert.match(koiDetail.textContent, /The pond's own service/);
});

test("browser: family members carry the diamond in the raw view too", async () => {
  const { ctx, document } = await boot();
  seedRaw(ctx);
  probe(ctx, "renderBrowser()");
  const family = [...document.getElementById("browser-queue").children]
    .filter((n) => n.className.includes("family"));
  assert.equal(family.length, 1, "one family row among the raw water");
});

// ── WP4: the cross-host diff ─────────────────────────────────────────

test("diff: three buckets over (type, name), withdrawals never count as seen", async () => {
  const { ctx } = await boot();
  const out = probe(ctx, `
    diffInstances(
      [
        { service_type: "_koi-serve._tcp.local.", name: "sparkle", instance_name: "sparkle", host: "a.internal.", port: 5641 },
        { service_type: "_ipp._tcp.local.", name: "Brother", instance_name: "Brother", host: "b.internal.", port: 631 },
        { service_type: "_googlecast._tcp.local.", name: "gone", instance_name: "gone", removed_at: "2026-08-28T00:00:00+00:00" },
      ],
      [
        { service_type: "_koi-serve._tcp.local.", name: "sparkle", instance_name: "sparkle", host: "a2.internal.", port: 5641 },
        { service_type: "_hap._tcp.local.", name: "Lamp", instance_name: "Lamp", host: "c.internal.", port: 8080 },
      ],
    )
  `);
  assert.equal(out.both.length, 1, "sparkle seen by both");
  assert.equal(out.onlyA.length, 1, "the printer only on A");
  assert.equal(out.onlyB.length, 1, "the lamp only on B");
  assert.equal(out.both[0][0].name, "sparkle");
});

test("diff: nodes persist and the selects rebuild", async () => {
  const { ctx, document } = await boot();
  probe(ctx, `diffSaveNodes([{ name: "brook", address: "192.168.1.44", port: 5641 }]); diffRefreshSelects();`);
  const a = document.getElementById("diff-a");
  const b = document.getElementById("diff-b");
  assert.equal(a.children.length, 2, "this machine + brook");
  assert.equal(b.children.length, 1, "nodes only for B");
  assert.match(b.children[0].textContent, /brook/);
  assert.match(probe(ctx, `localStorage.getItem("koi-diff-nodes")`), /192\.168\.1\.44/);
});

// ── WP5: deaf-detection verdict ──────────────────────────────────────

test("deaf verdict: no burst yet, hearing, deaf, and stale-silence states", async () => {
  const { ctx } = await boot();
  const v = (b) => probe(ctx, `deafVerdict(${JSON.stringify(b)})`);
  assert.match(v(null).text, /No query burst/);
  assert.match(v({ bursts_sent: 0 }).text, /No query burst/);
  const heard = v({ bursts_sent: 3, answers_total: 9, last_burst_at: "x", last_burst_answers: 4, last_burst_age_secs: 12 });
  assert.equal(heard.tone, "ok");
  assert.match(heard.text, /heard 4 answers/);
  const deaf = v({ bursts_sent: 3, answers_total: 0, last_burst_at: "x", last_burst_answers: 0, last_burst_age_secs: 30 });
  assert.equal(deaf.tone, "bad");
  assert.match(deaf.text, /firewall/);
  const stale = v({ bursts_sent: 3, answers_total: 0, last_burst_at: "x", last_burst_answers: 0, last_burst_age_secs: 900 });
  assert.equal(stale.tone, "warn");
  assert.match(stale.text, /stale/);
});

test("deaf verdict: renders from the snapshot into the browser pane", async () => {
  const { ctx, document } = await boot();
  probe(ctx, `
    latestSnapRaw = { burst: { bursts_sent: 2, answers_total: 0, last_burst_at: "2026-08-28T00:00:00Z", last_burst_answers: 0, last_burst_age_secs: 15 } };
    renderBrowser();
  `);
  const verdict = document.getElementById("browser-verdict");
  assert.equal(verdict.hidden, false);
  assert.match(verdict.textContent, /Deaf\?/);
  assert.ok(verdict.className.includes("bad"));
});
