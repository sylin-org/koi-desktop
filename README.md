# koi-desktop

Koi's desktop workbench — a friendly native shell over the [Koi](https://github.com/sylin-org/koi)
local substrate. The daemon stays headless; CLI, local web API, and this
workbench are three doors into the same pond.

## Decisions

- **Tauri 2 shell** (pinned to the same versions Ghostlight ships: tauri
  `=2.11.5`, tauri-build `=2.6.3`). Desktop lifecycle, tray, and window patterns
  are borrowed from `sylin-org/ghostlight` (`crates/orchestrator/src/desktop/mod.rs`).
- **Visual language**: Ghostlight's ground/band/card anatomy and single motion
  curve. **Palette**: Koi as published on sylin.org — accent `#60a5fa`, light
  `#93c5fd`, ground `#0f0e12`; family state colors (ok `#4ade80`,
  attention `#fbbf24`). The mascot is the published `koi-mascot.png`.
- **One frontend rule**: native views exist only where a browser genuinely
  cannot go (tray, ceremony wizards, OS integration). The workbench speaks to
  the daemon exclusively over its loopback HTTP API — no parallel backend.
- **Truthful states only** (ADR-020): an unreachable daemon reads "offline";
  nothing is invented.

## Status

Scaffold phase (ADR-033 in the koi repository): tray + first posture pane.

## Development

```sh
cargo run        # dev run against a local daemon on 127.0.0.1:5641
cargo build --release
```

Linux needs webkit2gtk; Windows needs nothing extra.
