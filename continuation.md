# koi-desktop — continuation prompt (cycle 1: the joyful instrument)

You are continuing the Koi desktop workbench (Tauri 2, sibling repo to `sylin-org/koi`).
The daemon-side stable-1.0 gate is CLOSED (see `sylin-org/koi/continuation.md`); this
repo's cycle 1 makes the pond visible, lovable, and reachable from a phone.

Read FIRST, then re-verify every premise against the tree:

1. `docs/product-requirements-cycle1.md` — the requirement harvest (audience
   interviews → M/N classification). The M rows are the contract.
2. `docs/epic-cycle1.md` — delta assessment (keep/modify/add/drop) and the 11 work
   packages with acceptance criteria.
3. `docs/handoff-mobile-cycle.md` — the in-flight state at the last handoff.
4. `sylin-org/koi` repo: `docs/adr/033` (workbench), `docs/adr/035` (gentle
   participation — detect/yield/declare/granted-latitude/settlement), `docs/adr/032`
   (matrix + stable-gate), `docs/lessons-learned.md` (RL-1..RL-19).

## Verified state (re-verify with git status + git log)

- koi-desktop `main` at `2ec0772`+ : WP0-WP9 LANDED (WP0 `11a9d52`, WP1 `25ea695`,
  poke `69b76cb`+fixes, mascot `2bac987`, mobile+QR `e6c216d`, WP2 glance `4a9665d`,
  WP3 browser `83d2160`, WP4 diff `e41d80a`, WP5 verdict `c3159ca`, WP6 trust
  `795b8e9`, WP7 passage `46c4bf5`, WP8 care `aef85a8`, WP9 glass `2a51912`).
  JS harness: `node --test ui/app.test.mjs` (25 cases). See
  `docs/handoff-mobile-cycle.md` for the arc details and the physical tail
  (WP10 CachyOS parity, trust ceremony, operator phone test).
- koi repo `main`/`dev` at `2ebb868`+ : mobile serving (`8bb9e6c`), ADR-035
  ladder work, **service-config fix (`4dc3ba5`: the Windows service ignored
  config.toml — `Config::from_service_launch` now applies CLI > env > file >
  default in service mode; LAN pond serving verified 200)**, **deaf-detection
  counters (`2ebb868`: BurstStats in the browser cache/snapshot)**.
- The koi service on this workstation: STOPPED at last check (TIME_WAIT race after a
  restart — wait 2–4 min, then `sc start koi` elevated). Config
  `%ProgramData%\koi\config.toml` has `http_bind = "0.0.0.0"` (mobile access ON).
- Pond UI already published: `%ProgramData%\koi\ui\` holds the five files; `GET /`
  on the daemon serves them (read-only LAN view).

## The product cycle (what "done" means)

WP0 foundations and the poke channel are LANDED. Remaining, in order:

- **WP1** Status streaming hero — sentence stream (newest first, cap 50), watched
  pins (localStorage `koi-watched`), hero admission policy (watched / degraded /
  flapping / recent; by-design capability skips never earn the hero), flapping = ONE
  row + count per 90s window, designed quiet state ("all quiet — nothing needs you").
- **WP2** At a glance — hero (attention → watched → quiet) + latest happenings list;
  every row click-through via the view registry.
- **WP3** Browser pane (raw lens) — `GET /v1/mdns/browser/snapshot` per node; all
  types incl. non-koi; TXT expanded; first/last seen; live events; burst button.
- **WP4** Cross-host diff — two nodes' snapshots, three buckets (both / only A /
  only B). The unique capability: koi has daemons on every machine.
- **WP5** Deaf-detection — daemon counters (bursts sent / answers received) + pane
  verdict ("announcing but hearing nothing — firewall, not network").
- **WP6** Trust pane — two-fingerprint compare, grantor→grantee labeled grant, audit
  view, revocation shown truthfully. Friction here is a feature (Ferris).
- **WP7** Passage — open the working endpoint from an inhabitant row (one click).
- **WP8** Care + notifications — star → hero pin → one OS notification on fade.
- **WP9** Honest glass pane — the 8-rung ladder with skip reasons as data.

Then: **CachyOS (test01) parity pass** — Linux build of the workbench, same manual
pass, webview guards — and the cycle closes.

## Transport + architecture rules (never break these)

- The webview holds no network: all daemon bytes via Tauri commands (Ghostlight rule).
- Events-first: live streams + the 60-second reconcile safety net only. No polling.
- Pure JS layers, no framework; CSP lessons (classes only in injected markup).
- The poke channel: the workbench binds `127.0.0.1:5640` loopback-only;
  `GET /poke` → immediate re-read; `GET /health` → "koi ui here";
  `koi-desktop --poke` (or `curl 127.0.0.1:5640/poke`) is the nudge. Retry the bind
  with backoff — a kill-then-relaunch leaves the old socket lingering.
- Lanes assert capability state via `/v1/status` (8-rung ladder with skip reasons,
  ADR-035), never socket-file presence. A topology change is a different designed
  state, not a failure.
- The mascot renders at integer scales of the 100px sprite only (2:1 card, 3:1 echo);
  anything else splits pixel-art source pixels unevenly and reads as distortion.

## Environment facts (measured, recorded)

- Windows workstation `stone-leaded-sparkle` at `192.168.1.137`; the koi service is
  currently uninstalled-then-reinstalled at operator direction — `koi install` from
  `target\release\koi.exe` restores it; the mobile-access firewall rule is named
  `Koi Web UI (tcp 5641)`.
- brook/granite standing daemons run `--no-mdns`-style (they yield 5353) and watch
  the shared Docker socket — lanes that derive labeled containers must isolate the
  runtime themselves (stop the standing service around the test, restore after).
- systemd-resolved was disabled on brook/granite (it held 5353/5355); DNS goes via
  the gateway, verified.
- Credentials: brook/granite via DPAPI blob
  (`%LOCALAPPDATA%\Koi\lab-scheduler\lab-password.dpapi` → KOI_LAB_PASSWORD);
  test01 = test/test only (KOI_TEST01_PASSWORD), never the lab password.
- plink quoting from a session shell: write probe/runner `.ps1` files into `.tmp/`
  and run with `pwsh -File`; inline one-liners break at the bash→pwsh→plink layers.

## Standing constraints (unchanged)

- External publication/posts: operator-only. Draft, never post.
- Elevation + `--allow-system-mutation` for workstation mutations; catalog grants
  enforced (windows: scm, firewall, trust-store).
- Workstations are daily drivers: run-scoped, preflighted, exactly restored.
- Full gates per landing: fmt, clippy -D warnings, locked tests, audit. Commit per
  slice; push dev+main (koi) / main (koi-desktop); clean tree before lab deploys.
- Scratch lives in repo-local `.tmp/` (gitignored) — never %TEMP%.
- Zero telemetry. The truthful covenant (never show an untrue state) outranks every
  feature.
