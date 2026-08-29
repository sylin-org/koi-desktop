# Handoff — mobile access + pond QR (cycle 1, in progress)

**Where we are (2026-08-28 late evening):** the QR/mobile-access feature is fully
implemented in both repos and was live-verified piece by piece. The only remaining
step is (re)starting the koi service and the operator's phone test — the last attempt
stopped the service and Windows TIME_WAIT sockets on port 5641 blocked the immediate
restart. They clear in 2–4 minutes; just wait, then start.

## Exact resume steps

1. `sc start koi` (elevated one-shot, `.tmp/svc-start2.ps1` pattern).
2. Verify: `curl http://127.0.0.1:5641/healthz` → 200 (daemon up, reads
   `%ProgramData%\koi\config.toml` with `http_bind = "0.0.0.0"` — already written).
3. Verify LAN: `curl http://192.168.1.137:5641/` → should return the published pond UI
   (the files are already on disk under `%ProgramData%\koi\ui\` — publishing survives
   daemon restarts). Firewall rule `Koi Web UI (tcp 5641)` already exists (enabled,
   allow, scoped to the service exe).
4. The workbench (`koi-desktop.exe`) is running with the poke listener on
   `127.0.0.1:5640` — `curl 127.0.0.1:5640/poke` forces an immediate re-read.
5. Operator scans the QR (workbench Status → Phone button → `pond_qr_target` +
   `pond_qr_svg`) — lands on `http://192.168.1.137:5641/` = the same pond, read-only,
   mobile-styled.

## Known state notes

- The koi service ran the OLD binary for most of tonight's mobile testing; the current
  on-disk release binary has everything (ui_dir serving, ADR-035 ladder, scope labels).
  If the service crashes on start, check for TIME_WAIT on 5641 first (RL-20 candidate:
  "a restarted listener must wait out TIME_WAIT or set SO_REUSEADDR deliberately").
- The pond UI publish (PUT /v1/ui, DAT-gated, five fixed files) was verified with a
  200 "published:5" and all three assets served 200 from loopback.
- brook/granite standing services: active (restored). systemd-resolved disabled on
  both (DNS via gateway, verified) — recorded in the ledger.
- The Windows koi service is intentionally uninstalled-then-reinstalled today; the
  mobile-access toggle wrote `http_bind = "0.0.0.0"` into
  `%ProgramData%\koi\config.toml` (line 52) — keep it.

## Commits so far (this arc)

- koi repo: `8bb9e6c` (daemon serves published pond UI; PUT /v1/ui; base64 in
  koi-serve; docker-gated method fix) — plus earlier ADR-035/P1 commits through
  `3014713`.
- koi-desktop: `36af531` (requirements + epic), `11a9d52` (WP0 foundations),
  `25ea695` (WP1 streaming hero), `8dcb1cd` (poke channel), `2bac987` (mascot 2:1
  scale fix), `e6c216d` (mobile UI + QR), `8dcb1cd`+ poke EOF/timeout fixes.
- **Uncommitted:** nothing known in either repo; the requirements + epic docs are
  committed (`36af531`).

## The one mystery still open

`GET /` on the LAN IP returned 000 (refused) while loopback served 200 — explained
by the loopback-only bind at the time. After the config-driven 0.0.0.0 restart, the
LAN IP has NOT been re-verified from this machine (the service was stopped mid-check).
First thing after restart: `curl http://192.168.1.137:5641/` → expect 200.
