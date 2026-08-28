# Koi Desktop — Product Requirements (Cycle 1)

**Sources:** synthetic audience interviews (2026-08-28) — Dana (homelabber, primary), Mika
(studio operator, time-poor), Sam (household, phone), Ferris (security admin), future-you
(operator, six months on) — plus operator direction recorded in-session.
**Status of sources:** synthetic hypothesis-extractors, not validation. The one real user is
the operator; a week of operator playtesting on real machines is the first real validation.
**Classification:** M = must-have (product-defining; the cycle fails without it), N =
nice-to-have (delight amplifier, secondary persona, or follow-on). Status: landed / partial
/ new (relative to the workbench as of commit `4f7a38b`).

---

## Positioning (locked)

> **"Two labels on a container and every machine on your LAN reaches it over TLS by name."**

This is the answer to "what does this replace" (hand-edited hosts files, TLS ceremony,
avahi-browse incantations, Portainer-for-one-container). Every surface reinforces this
sentence.

## Design principles (non-negotiable)

- **DP1 Truthful covenant** — never show an untrue state. Spend the trust deposit once on a
  wrong green dot and the app is over. (Every persona converged on this independently.)
- **DP2 Quiet instrument** — restraint is delight. "All quiet — nothing needs you" is a
  designed state. No celebration spam, no wizards, no accounts, no cloud.
- **DP3 Friction placement** — actions in 1 click, 2 at most, consequence stated before the
  state changes, intent labeled exactly. The one deliberate exception: trust-granting keeps
  friction by design.
- **DP4 Capability honesty** — every surface reflects the daemon's declared capability state
  (ADR-035 ladder, with skip reasons). A topology change is a different designed state,
  never a silent absence.
- **DP5 Events-first** — live streams and the 60-second reconcile safety net only. No polling.
- **DP6 Lean construction** — pure JS layers, no framework, the webview holds no network
  (all daemon bytes via Tauri commands), CSP lessons applied (classes only in injected
  markup). Operator direction: the koi mascot is the About identity card only — it is never
  a default home surface.

---

## Epic A — Status (streaming hero session)

| ID | Class | Status | Requirement | Source |
|----|-------|--------|-------------|--------|
| A1 | M | new | Streaming hero: live events land newest-first as generated **sentences**, never raw JSON. | future-you, Mika |
| A2 | M | partial | Lamp reflects daemon state; the streaming surface has a designed **quiet state** ("all quiet — nothing needs you"). | Mika, future-you |
| A3 | M | new | **Watched items pin** in the hero (feeds H1). | Sam, future-you |
| A4 | M | new | **Hero admission policy**: watched items always; degraded or flapping items; recent state changes that fade as they settle. By-design capability skips never earn the hero (they live in the honest glass). | Dana, future-you |
| A5 | M | new | **Flapping policy**: a restart storm is ONE row with a count — flapping itself is the condition, not each restart. | Dana |

## Epic B — At a glance (the quick read)

| ID | Class | Status | Requirement | Source |
|----|-------|--------|-------------|--------|
| B1 | M | new | Hero: attention items first, watched items second, else the quiet state. The "while I was away" answer. | future-you |
| B2 | M | new | **Latest happenings**: the sentence stream, grouped, deduplicated, newest first. | future-you, Dana |
| B3 | M | new | **View registry click-through**: every row opens its corresponding view (runtime → Discover, dns → records, certmesh → trust, capability → honest glass). The registry is data, not hardcoded. | operator |
| B4 | N | new | "While you were away" session-start digest as a distinct grouping. | future-you |

## Epic C — Discover (curated lens — the inhabitants)

| ID | Class | Status | Requirement | Source |
|----|-------|--------|-------------|--------|
| C1 | M | landed | Inhabitants grouped by device; TXT-friendly names; type labels. | Dana, Mika |
| C2 | M | landed | Presence states: live / fading / gone (workbench remembers past daemon eviction). | Dana |
| C3 | M | landed | Type / state / text filters. | Dana |
| C4 | M | new | One-line **sentence summary** per inhabitant (from the sentence writer). | future-you |
| C5 | N | new | Full **biography**: one-paragraph event history per inhabitant ("this is forge, build box, up 40 days, rejoined Tuesday"). | future-you |
| C6 | M | new | **Care/star**: mark an inhabitant watched (feeds A3, H1). | Sam, future-you |
| C7 | N | new | **Rename/rebind**: edit a `.internal` name from the inhabitant view. To name is to tame. | operator |
| C8 | M | new | **Passage entry**: click through to the working endpoint (see Epic F). | Mika, Dana |

## Epic D — Browser (raw lens — the water)

| ID | Class | Status | Requirement | Source |
|----|-------|--------|-------------|--------|
| D1 | M | new (endpoint exists) | Raw snapshot table: ALL service types incl. non-koi; type, instance, host, port, first/last seen. `GET /v1/mdns/browser/snapshot` per node. | Dana, Ferris |
| D2 | M | exists | Live announce/withdraw events via the browser SSE stream. | Dana |
| D3 | M | landed (daemon) | Type dictionary: human label + description for well-known types (`_hap._tcp` → HomeKit). Displayed, not hidden. | Mika |
| D4 | M | new | TXT records expanded on selection — never hidden behind curation. | Dana |
| D5 | M | new | **Cross-host diff**: fetch two nodes' snapshots, three buckets (both / only A / only B). The diagnosis for "why can't forge see the printer." Unique to koi's multi-daemon architecture. | Dana, future-you |
| D6 | M | new (small daemon change) | **Deaf-detection**: expose burst-sent / answers-received counters so the pane can render "announcing but hearing nothing — firewall, not network." Closes a real incident class on this machine. | Dana, operator ticket |
| D7 | N | new | **Inventory export**: first-seen/last-seen CSV/JSON. | Ferris |
| D8 | N | out of scope | Interface attribution (wifi vs wired origin) — mdns-sd limitation; recorded, not faked. | Dana |

Design note: Discover is the pond (curated, koi's view); the Browser is the water (raw, the
network's own words). If the raw view ever contradicts the curated one, that visibility is a
feature — a cache or derivation bug should surface by the two lenses existing side by side.

## Epic E — Trust (ceremony + audit)

| ID | Class | Status | Requirement | Source |
|----|-------|--------|-------------|--------|
| E1 | M | new | **Two-fingerprint comparison** as its own deliberate step — friction is the feature. Never a big friendly "Welcome" button. | Ferris |
| E2 | M | new | Grant action labeled **grantor → grantee** with the exact consequence ("signed by your CA, revocable anytime"). | Ferris |
| E3 | M | new (UI) | **Audit log**: every grant, revoke, renewal readable in the UI. A CA without an audit trail is a diary, not a CA. | Ferris |
| E4 | M | partial | Revocation reflected truthfully in all surfaces — including the pond visibly retracting. Lying here is lying when it matters most. | Ferris |
| E5 | N | new | Revocation propagation view: members receiving the revocation, with timestamps. | Ferris |
| E6 | M | new | **Stranger flow**: an unknown announcing device surfaces from the Browser into the ceremony entry (discovery and trust are one connected feature). | Ferris, Dana |

## Epic F — Passage (the payoff)

| ID | Class | Status | Requirement | Source |
|----|-------|--------|-------------|--------|
| F1 | M | new | **Open the working endpoint** from an inhabitant/proxy: composed URL, one click, the service the pond already health-checked and cert-anchored. | Mika, Dana |
| F2 | N / open | new | Trust-on-viewer guarantees for the *opening* machine (viewer-side cert visibility). Open question — needs its own design pass. | Dana |

## Epic G — Honest glass (ADR-035 made visible)

| ID | Class | Status | Requirement | Source |
|----|-------|--------|-------------|--------|
| G1 | M | partial (daemon landed) | Capability ladder with **skip reasons** — 8 rungs incl. `ipc`. The reason is data, not a log line. | Ferris, future-you |
| G2 | M | new | Glass **pane** (full ladder) + per-capability **reason overlay** when clicked from Status. Lanes/files assert capability state via `/v1/status`, never socket presence. | future-you |

## Epic H — Notifications & tray

| ID | Class | Status | Requirement | Source |
|----|-------|--------|-------------|--------|
| H1 | M | new | **One OS notification on watched-item fade.** Opt-in by starring; never more. | Sam, Mika, future-you |
| H2 | M | landed | Tray lamp states. | operator |
| H3 | N | new | Tray menu: inhabitant count, quick actions. | operator |

## Epic I — Household (deferred)

| ID | Class | Status | Requirement | Source |
|----|-------|--------|-------------|--------|
| I1 | N | deferred, needs own ADR | Read-only pond-sentence page reachable on the LAN from a phone. Brushes ADR-033's dashboard deprecation — own decision required. The sentence writer already produces its content. | Sam |
| I2 | N | — | Incident one-liners ("the photo server is down") share the sentence writer's source. | Sam |

## Epic J — Product tickets / ops

| ID | Class | Status | Requirement | Source |
|----|-------|--------|-------------|--------|
| J1 | M (operator-recorded) | new | Surface the daemon's own firewall warning from an unelevated `Run once` ("your fish can't hear the pond"). Partially covered by D6's deaf-detection for the browsing surface. | operator ticket |
| J2 | N (ops) | — | Stale lab-era firewall rules cleanup (operator-approved operation, not UI). | operator notes |

---

## Non-functional requirements (all M)

| NF | Requirement |
|----|-------------|
| NF1 | The webview holds no network — all daemon bytes via Tauri commands (Ghostlight rule). |
| NF2 | Pure JS layers, no framework; CSP lessons (classes only in injected markup, no inline styles). |
| NF3 | Events-first; the 60-second reconcile is a safety net, never the mechanism. |
| NF4 | Zero telemetry. Always. |
| NF5 | Tauri versions pinned to Ghostlight's exact choices; family fixes transfer directly. |
| NF6 | The truthful covenant (DP1) outranks every feature. |

## Explicitly retired / out of scope

- The koi mascot as a swimming/home surface — the mascot is the **About identity card only** (operator direction).
- Resurrecting or extending the daemon's web dashboard (ADR-033 deprecation stands).
- Frameworks, state libraries, build tooling beyond the current pure-JS setup.
- Accounts, cloud, anything that phones home.
- Making trust-granting frictionless (Ferris's friction is deliberate).

## Open questions for the cycle

1. F2: what does the workbench promise about viewer-side trust when opening an endpoint?
2. C7: does the daemon expose record *add/edit* (not just remove) for rebinding?
3. E6: what identifies a "stranger" (mDNS name? MAC? IP?) and how long is it remembered?
4. B2 retention: how much happenings-history does the workbench keep locally (diary depth)?
