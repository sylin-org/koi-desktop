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
tray reveal, login startup, live status/events, discovery and device comparison, DNS,
trust, capability glass, and read-only Pond publishing. Version 0.1.3 asks the
daemon to arm its narrow LAN adapter and renders only the URL backed by the
daemon's real socket and host-policy assessment; the full operator API remains
local.

On Windows, an installed SYSTEM daemon and the interactive workbench meet through
Koi's authenticated named pipe. A readable breadcrumb is only a fast path, not a
deployment requirement. The installer records one operator SID; the workbench does
not broaden that trust boundary.

Watched-item fade notifications are deliberately one-shot per fade episode. A
successful handoff means Windows accepted the notification; Focus Assist / Do Not
Disturb can still suppress the visible banner, and Koi does not override that OS
policy. Pond is separate from notifications and from the operator API: it is an
explicitly armed, read-only listener whose exact URL and firewall assessment come
from the daemon.

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

### Arch Linux and derivatives

The VCS package installs the workbench at `/usr/bin/koi-desktop`, plus its
desktop entry and icon. From a checkout:

```sh
cd packaging/arch
makepkg -si
```

The manifest uses Arch's current `webkit2gtk-4.1` and
`libayatana-appindicator` packages. It builds the locked source and never
creates an autostart entry pointing into the checkout. The workbench's own
Autostart switch can then register the durable installed executable.

### Fedora and derivatives

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

### Alpine Linux

The native workbench links Alpine's shared GTK, WebKitGTK, and app-indicator
libraries. The repository therefore disables Rust's default static CRT only for
musl targets in `.cargo/config.toml`; application code and release behavior stay
the same. With `build-base`, `webkit2gtk-4.1-dev`, and
`libayatana-appindicator-dev` installed, the ordinary locked test, clippy, and
release commands above build natively on Alpine.

Build the native APK from the commit-pinned recipe, then install that artifact
through `apk` so the binary, desktop entry, icon, and upgrades have one owner:

```sh
doas apk add alpine-sdk atools desktop-file-utils
doas addgroup "$USER" abuild
abuild-keygen -a -n
cd packaging/alpine
abuild -r
doas apk add --allow-untrusted \
  --repository "$HOME/packages/packaging" koi-desktop
```

Later recipe revisions follow the ordinary package path: rebuild with `abuild -r`,
then upgrade the installed package from that repository:

```sh
doas apk upgrade --allow-untrusted \
  --repository "$HOME/packages/packaging" koi-desktop
```

The APK depends on Alpine's WebKitGTK/app-indicator shared libraries plus
`xdg-utils` and the platform `polkit` provider; it never installs a checkout
executable.

Pond is a separately armed, read-only router inside the one Koi daemon. Phone
publishes the fixed browser bundle, asks Koi to acquire the derived fourth port,
and displays the exact URL Koi reports. Stop sharing disarms it; restarting Koi
restores an enabled intent and continues reconciliation.

### Shared Rust shell

Normal launch and autostart use the production Maud shell in `koi-ui`, pinned
alongside `koi-client` to Koi `17fb591148c8c93f97a8db6322018f6847a456d5`.
No sibling checkout or renderer-selection flag is needed. The existing singleton,
tray, authenticated local-control handoff and native motion boundary are retained.

The internal `koi-ui` protocol admits only the exact main-window root and bounded
search/favorite/selection/peer query fields, its embedded `/refresh.js` asset, and
a bounded `/compare` POST containing only a selected DeviceId. Rust reads
one schema-checked authenticated catalog, renders a dated snapshot and embeds its
original assets. A credential-free DOM adapter rereads complete Rust-rendered snapshots
five seconds after successful reads, preserving filters, selection and unsent drafts.
Temporary failure marks retained evidence stale, disables old Open links and retries
with bounded backoff; page exit aborts/fences the reader. No additional HTTP listener
or desktop credential in JavaScript. A failed read is unavailable, never empty.

Home / Devices / Settings / About use the shared Rust components. Browser access
and phone pairing are parked; the workbench is the Home entry point. Advanced tools opens the retained workbench at
its unchanged Tauri asset origin, preserving local storage and its existing
watched-item import. Its Home button returns to the shared shell and releases
native event listeners, including registrations that finish during navigation.
Windows return navigation uses the registered `http://koi-ui.localhost/` origin
(initial window setup translates the custom scheme; later navigation does not).
Home never selects a legacy pane. Cleanup has a three-second deadline: a failure
keeps the current pane visible with a retry/reopen message, and late registrations
still release. Reopen restores live subscriptions after a failed return.
External Open validates a complete HTTP(S) URL without embedded credentials,
control characters or ambiguous authority syntax. Windows invokes the native
browser association directly, never a command interpreter; Linux/macOS pass one
URL argument to their native launcher. Acceptance by a launcher is not a promise
that the remote page is reachable or trusted. Home now supports submitted search,
favorite filtering, stable selection and wide/narrow service details. Its inspectable
Open links are intercepted at native navigation and cannot replace the privileged
workbench with a remote page. API-only/absent services retain connection details.
Automatic snapshot refresh/reconnect is implemented; installed journey acceptance
remains R07 work. This is polling, not a second catalog event/state model.
The read-only Pond bundle retains its existing public projection; it does not
receive the operator catalog.

Linux observes the actual GTK animation preference and applies/removes only its
own shared reduction stylesheet. Media-query propagation is not assumed.
MacOS remains physically unverified. See Koi's R06/shared-shell report for the
new artifact's test and platform-acceptance status; old probe evidence is not a
production-package pass.

Checks:

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all --check
node --test ui/app.test.mjs
cargo build --locked --release
```

Shared component, hostile-input, authenticated HTTP and offline browser commands
live in Koi's `crates/koi-ui/README.md`. Install through the native package recipe
before collecting deployment evidence; keep a fresh exact prior package and an
interruption-safe recovery guard. Historical renderer experiments remain
reproducible at their immutable commits, not as an alternative product mode.

### Devices and comparison

Devices groups the shared catalog by opaque device identity, marks this computer
explicitly, and links hosted services to their existing Home details. Discovery and
joined CertMesh identity are separate facts. Counts include the whole snapshot,
including retained unavailable services, independently of Home filters.

“Compare what devices can see” is available in Devices and Advanced → Status.
Choose a discovered Koi peer, then start a read. The native adapter retains one
in-memory result and resolves its endpoint from the current catalog. It never sends
local authority to a peer. Missing/refused/slow/unsupported reads and incompatible
query coverage stay incomplete; two readable snapshots with matching mDNS query
scope can report agreement or differences, including changed endpoint/TXT details.
Observer labels and local receipt times accompany the result. Physical interface and
same-network equivalence are not reported. No browser listener or firewall change
is part of comparison. The legacy Diff UI and manual node list are retired.
