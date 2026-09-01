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
  cannot go (tray, ceremony wizards, OS integration). The workbench discovers
  the one real daemon through its authenticated local-control transport, then
  uses that daemon's loopback HTTP API — no parallel backend.
- **Truthful states only** (ADR-020): an unreachable daemon reads "offline";
  nothing is invented.

## Status

The workbench is functional on Windows and glibc Linux: singleton lifecycle,
tray reveal, login startup, live status/events, discovery/browser/diff, DNS,
trust, capability glass, and read-only Pond publishing. Version 0.1.1 renders
the data root reported by the authenticated local daemon instead of assuming an
OS path.

## Development

```sh
cargo run        # dev run against a local daemon on 127.0.0.1:5641
cargo build --release
node --test ui/app.test.mjs
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

Linux needs webkit2gtk; Windows needs nothing extra.

## Linux packages

Build a native RPM with the Tauri CLI:

```sh
cargo tauri build --bundles rpm
```

On an immutable Fedora/Bluefin host, keep compiler and WebKit development
packages in a Fedora toolbox, then layer only the resulting RPM with rpm-ostree
and reboot. Bluefin 44 physically passed install, upgrade, XDG autostart, GNOME
SNI reveal, and a fresh Wayland login with this shape.

Tauri's bundled linuxdeploy currently cannot strip Fedora 44/modern Arch ELF
files containing RELR sections, so an AppImage failure there is a bundler
compatibility limit, not a failed native build. Use the RPM on Fedora-family
hosts until that upstream toolchain catches up.

Pond's current HTTP server follows the daemon's bind. A default loopback daemon
can publish the files locally but is not reachable from a phone; do not claim
LAN access or silently expose the full operator API. Koi needs a dedicated
read-only Pond listener for that default-install experience.
