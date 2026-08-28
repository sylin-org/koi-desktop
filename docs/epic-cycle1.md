# Cycle 1 Epic — The Joyful Instrument

**Traceability:** implements `docs/product-requirements-cycle1.md` (must-haves; nice-to-haves
pulled in only where they ride the same work package).
**Machines:** build + test on the Windows workstation (this machine, local daemon at
`127.0.0.1:5641`, LAN daemons on brook/granite for cross-host work); parity validation on the
CachyOS machine (test01) at the end.
**Rule in force:** the webview holds no network; every daemon byte crosses via Tauri
commands. Every state shown is a state the daemon declared.

---

## 1. Delta assessment — current workbench vs cycle requirements

Current surface (commit `4f7a38b`): four panes (Discover, DNS, Status, About), tray with
posture polling, autostart + `--minimized`, event-driven lamp, Discover with grouping /
presence / filters / family marker / ping, DNS records with add/remove/TXT, service
controls, daemon run-once, debug sink. Transport: 15 Tauri commands, `mdns-event` +
`discover-stream` + status pushes, daemon origin hard-coded to `127.0.0.1:5641`.

### Keep (landed, maps directly to requirements)

| Current | Maps to | Note |
|---|---|---|
| Discover: device grouping, presence live/fading/gone, type labels, text filter, ping | C1, C2, C3, D3 | The curated lens exists. Presence windows (90s/10min) stay. |
| DNS records: list, add, remove, TXT set/clear | C7 (partial) | Rename/rebind = add+remove sequence; needs UX wiring only. |
| Transport idiom: Rust-owned bytes, event pushes, 60s reconcile | NF1, NF3, DP5 | Untouched. |
| Tray + posture polling + autostart + `--minimized` | H2, NF5 | Untouched. |
| Service controls (start/stop/run-once) + autostart toggle | Status actions (DP3) | Stay in Status under the action contract (consequence labels added). |
| Family marker regex + ◆ marker | C1 curation | Becomes the *curated lens* rule (see Modify). |
| Debug sink to repo-local `.tmp` | NF2 | Stays. |

### Modify

| Current | Change | Refs |
|---|---|---|
| Status pane = facts + controls (polled reconcile) | Restructure into the **streaming hero**: sentences newest-first, hero admission policy (watched / degraded / flapping / recent), designed quiet state. Service controls remain, relabeled under the action contract. | A1, A2, A4, DP3 |
| Discover as the whole discovery story | Discover becomes the **curated lens only** (family + opted-in inhabitants). Raw announcements move to the new Browser pane. The FAMILY regex becomes the lens rule, stated as data not a regex constant. | C1–C3, D1 |
| Daemon origin hard-coded `127.0.0.1:5641` | Multi-daemon transport: any node's read-only endpoints addressable by `(address, port)` for the cross-host diff and future scope views. Read-only GETs are auth-free by design; mutations stay local-only. | D5 |
| `daemon_status` returns `/v1/status` partially parsed | Surface the **capability ladder verbatim** (8 rungs, with skip reasons) for the honest glass and capability-aware assertions. | G1, G2, DP4 |
| DNS pane as a flat table | Add consequence labels to the action buttons (action contract); feed records into the sentence writer. | DP3, A1 |

### Add (new)

| New | Refs |
|---|---|
| **The sentence writer** — event/state → generated one-line summary; one module feeding Status, happenings, Discover summaries, and (later) the household page. | A1, B2, C4, I2 |
| **At a glance page** — hero (attention → watched → quiet) + latest happenings + click-through. | B1–B4 |
| **View registry** — event/capability type → target view mapping (data, not code). | B3 |
| **Browser pane (raw lens)** — full snapshot table (all types incl. non-koi), TXT expansion, first/last seen, live events, burst button, type dictionary. | D1–D4 |
| **Cross-host diff** — fetch two nodes' snapshots, three buckets (both / only A / only B). | D5 |
| **Deaf-detection** — daemon counters (bursts sent / answers received) + pane verdict ("announcing but hearing nothing — firewall, not network"). | D6 |
| **Trust pane** — two-fingerprint comparison step, grantor→grantee labeled grant, audit view, stranger entry from the Browser. | E1–E3, E6 |
| **Passage** — open the working endpoint from an inhabitant/proxy row. | F1 |
| **Care/star** — watch an inhabitant; feeds hero pins and the fade notification. | C6, A3 |
| **OS notification on watched fade** (opt-in by starring). | H1 |
| **Honest glass pane** — the 8-rung ladder with skip reasons; per-capability overlay. | G1, G2 |

### Drop

Nothing is deleted outright — the app is young and everything present maps to a
requirement. Retired *concepts* (per product-requirements, not code): mascot-as-home-surface
(stays on About), dashboard resurrection, frameworks, accounts, frictionless trust.

---

## 2. Work packages

Sequenced so each WP is independently testable on this machine before the next begins.
Every WP ends with: `cargo fmt`, clippy `-D warnings` (Rust), no CSP violations in the
debug sink, manual pass on this machine.

### WP0 — Foundations (sentence writer, view registry, multi-daemon GET)
**Refs:** A1, B2, B3, C4, D5 (transport), G1 (data).
- `ui/sentences.js`: pure module, `sentence(event) → {line, tone, target}` for every event
  type in the current `/v1/events` corpus (+ capability skips from `/v1/status`).
- View registry: `{event_kind → view}` table; unknown kinds fall back to Status.
- `daemon_get(address, port, path)` Tauri command (read-only GET to any node) alongside the
  existing local-origin commands.
- **Accept:** every event type in a recorded session produces a sentence; a fetch of
  brook's `/v1/mdns/browser/snapshot` succeeds from the workbench.

### WP1 — Status streaming hero
**Refs:** A1–A5, DP3. Depends on WP0.
- Restructure Status per the hero spec; sentences stream; watched pins; flapping = one row
  + count; quiet state designed; service controls relabeled with consequences.
- **Accept:** kill a container → hero shows the sentence within one event beat, no flapping
  spam (one row + count); star a service → it pins; all-calm → quiet state.

### WP2 — At a glance page
**Refs:** B1–B4. Depends on WP0.
- Hero (attention → watched → quiet) + happenings list; click-through via the registry.
- **Accept:** after WP1's chaos, the at-a-glance page recounts it correctly and every row
  navigates to the right view.

### WP3 — Browser pane (raw lens, local)
**Refs:** D1–D4. Depends on WP0 (registry for type labels reuse).
- Table over the local daemon's `/v1/mdns/browser/snapshot`; live rows via the existing
  `mdns-event` push; TXT expansion; type dictionary labels; burst button (existing command).
- **Accept:** side by side with Discover, the raw table shows non-koi announcements
  (printers, cast devices) that the curated lens hides.

### WP4 — Cross-host diff
**Refs:** D5. Depends on WP0 + WP3.
- Pick two daemons (local + a LAN node); three buckets: both / only A / only B.
- **Accept:** stop the story container on brook mid-test → the diff shows it vanishing from
  brook's view while granite still hears it (or vice versa) — a real multicast partition
  made visible.

### WP5 — Deaf-detection (daemon + pane)
**Refs:** D6. Daemon change → rebuild + redeploy daemons before testing.
- Counters in the browser worker/cache (bursts sent, answers received per burst) exposed in
  the snapshot; pane renders the verdict when a burst hears nothing.
- **Accept:** with the mDNS firewall rule blocked, the pane states the deaf verdict; with
  the rule restored, it clears.

### WP6 — Trust pane
**Refs:** E1–E3, E6. Prereq: verify the certmesh daemon doors (invite/grant/revoke/audit)
are reachable via Tauri commands with the breadcrumb DAT.
- Two-fingerprint side-by-side compare → grant (labeled grantor → grantee) → audit view.
- **Accept:** enroll a real member through the pane on this machine; the audit view shows
  the grant; a revoke propagates and the UI shows the member's state change.

### WP7 — Passage
**Refs:** F1, C8.
- Open-endpoint action on inhabitant/proxy rows (composed URL, one click).
- **Accept:** every healthy derived service in the pond opens correctly from the workbench.

### WP8 — Care + fade notification
**Refs:** C6, H1, A3. Depends on WP1.
- Star/unstar (persisted locally); watched fade → one OS notification. Tauri notification
  plugin (the only new dependency this cycle).
- **Accept:** star a service, stop its container → exactly one notification; restart → no
  notification.

### WP9 — Honest glass pane
**Refs:** G1, G2. Depends on WP0.
- Full ladder pane + per-capability overlay from Status clicks.
- **Accept:** with resolved's mDNS re-enabled on a lab box, the pane shows
  `mdns: skipped — <reason>` verbatim; on this workstation it shows mounted.

### WP10 — CachyOS parity pass
- Build the Linux workbench; run on test01 against its local daemon + LAN daemons; verify
  pane parity, webview guards, tray, notifications.
- **Accept:** the full manual pass completes on CachyOS with no Windows-only assumptions
  surfacing.

---

## 3. Cycle definition of done

1. Every M requirement in product-requirements-cycle1 is landed (F2/I1/J2 explicitly
   deferred) or has a recorded deferral rationale.
2. The full manual pass is green on this machine (Windows) — all panes, all actions, the
   cross-host diff against brook/granite, one trust ceremony against a real enrollment.
3. The parity pass is green on CachyOS.
4. Zero CSP violations, zero framework imports, zero webview network access (debug sink
   + policy violation listener silent).
5. Gates: `cargo fmt/clippy/test` for the Rust side; the debug sink clean; no new
   dependencies beyond the notification plugin (WP8).

---

## 4. Known daemon-side prerequisites (tracked, small)

- WP5 counters (bursts sent / answers received) — `koi-dashboard` browser worker + snapshot.
- WP6: confirm the certmesh audit log is reachable via an endpoint (the audit *file* exists;
  if no route mounts it, that is a small `koi-serve` addition).
- C7 edit: `/v1/dns` remove exists; confirm add/edit covers rebinding (the DNS pane already
  calls add/remove — believed sufficient).
