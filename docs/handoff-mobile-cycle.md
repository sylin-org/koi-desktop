# Handoff — cycle 1 (the joyful instrument), state after the WP2–WP9 arc

**Where we are (2026-08-29 ~01:30 local):** every work package except the final
CachyOS parity pass (WP10) is LANDED and pushed. The workbench on this machine
runs the release build with all panes; the daemon serves the republished pond
UI (5 files) so the phone view is the same surface.

## Landed this arc (koi-desktop main)

- `4a9665d` WP2 at-a-glance — hero (attention → watched → quiet), happenings
  deduplicated newest-first with counts, since-you-last-looked grouping, honest
  session-start digest; the sentence feed lifted into a shared store; fixed
  three real defects found on the way (duplicated discover definitions — the
  second shadowed type labels and its removed-handler called a nonexistent
  function; status click-through passed a VIEW name into targetOf so every row
  landed on Status).
- `83d2160` WP3 browser pane — the raw lens: everything the daemon hears with
  first/last seen from ITS snapshot, TXT + type dictionary expansion on click,
  filters, burst; Discover becomes the curated pond (family lens default,
  "everything" toggle).
- `e41d80a` WP4 cross-host diff — two nodes, three buckets, withdrawals never
  count as seen; `daemon_get` now surfaces the daemon's own error body (brook
  answers 503 `capability_disabled` honestly — its mDNS yields 5353).
- WP5 daemon half (koi `2ebb868`): BurstStats counters (bursts, answers per
  burst) in the browser cache; `record_burst` fires on worker (re)start only —
  heartbeat touches are deliberately NOT bursts.
- `c3159ca` WP5 pane half — the deaf verdict: fresh burst + zero answers =
  "Deaf? … firewall (mDNS udp 5353), not a quiet network"; stale silence is
  judged inconclusive.
- `795b8e9` WP6 trust pane — honest role card (CA / locked / open node), member
  roster with labeled two-step-armed revoke, grant ceremony with BOTH
  fingerprints side by side (invite code format `<secret>.<fp>`), guidance
  gated on the compared-them statement, audit trail via DAT-gated
  `/v1/certmesh/log`, stranger flow from Browser rows; new doors
  `certmesh_status/diagnose/log/invite/revoke` ride the breadcrumb DAT.
- `46c4bf5` WP7 passage — Open buttons on resolved rows; `composeUrl` derives
  scheme from the announcement's own TXT hints and refuses junk; `open_url`
  double-gates to http(s).
- `aef85a8` WP8 care + fade — stars on Discover/Browser rows (the same watched
  set that pins the hero), one OS notification per fade episode, revival
  re-arms; `tauri-plugin-notification =2.3.1` (the cycle's one authorized
  dependency) invoked from a Rust command.
- `2a51912` WP9 honest glass — the full 8-rung ladder verbatim from
  `/v1/status`, click-to-expand reasons, reachable from Status's button;
  registry mapping: certmesh events now click through to Trust.

## Daemon-side defect found + fixed (koi, dev+main `4dc3ba5`)

The Windows service built env-only config — `config.toml` was silently ignored
in service mode. Measured: `http_bind = "0.0.0.0"` bound loopback-only, which
was the handoff's open mystery (LAN root refused while loopback served 200).
`Config::from_service_launch` now parses the SCM launch line through the normal
Cli (precedence CLI > env > file > default, loud env-only fallback). Service
restarted on the new binary: **loopback healthz 200, LAN root 200 serving the
pond** (0.0.0.0:5641 listening).

## Physical evidence this arc

- Burst counters live on the real LAN: baseline burst heard 49 answers; a
  deliberately-disabled `Koi mDNS (udp 5353)` rule did NOT produce silence —
  other allow rules shadow it (stale lab-era rules, ticket J2). Rule restored
  (Enabled: Yes, verified). The deaf VERDICT mapping is covered by tests; the
  firewall-lever acceptance needs the stale-rule cleanup first.
- Pond UI republished: `{"ok":true,"published":5}`; LAN root 200 at 22,606
  bytes serving the new panes (grep glance-word in served app.js = present).
- Release workbench launched (poke listener answers on 127.0.0.1:5640); debug
  sink boot is clean: "workbench booted" + "snapshot ok: instances=30", zero
  JS errors, zero CSP violations.
- JS harness: `node --test ui/app.test.mjs` — 25 cases, all green (pure layers
  exercised in a DOM stub: feed/flapping/glance/dedupe/lens/browser/diff/
  verdict/trust/passage/care/glass).

## Remaining for the cycle (physical, operator-attended)

1. **WP10 CachyOS parity pass** — build the Linux workbench on test01, run the
   full manual pass there (panes, webview guards, tray, notifications).
2. **Trust ceremony physical acceptance** — the pane supports the full arc;
   running it means creating a LOCAL CA on the standing daemon (`koi certmesh
   create`, reversible with `destroy`) then invite → join a member → revoke →
   audit. Operator call on the standing-daemon mutation.
3. **Operator phone test** — Status → Phone → scan the QR; the pond on the LAN
   now serves the new panes read-only.
4. Full manual pass on this machine (all panes, diff vs a LAN node, one real
   enrollment) and the cycle-closing review against product-requirements M rows
   (B4's digest, C5 biography, C7 rename/rebind, D7 export, E5, H3, J1 remain
   N-row nice-to-haves or deferred by design).
