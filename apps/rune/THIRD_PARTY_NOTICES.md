# Third-party notices

The native Rune shell uses anomalyco/rune, rune-kit and the OpenCode theme from
the single pinned revision `bfb8facae2e4665e5e6d17afe85561884a7fd402`.
The complete source snapshot is retained in [`vendor/rune/`](vendor/rune/), with
the original MIT license, bundled font licenses, an offline source verifier and
an explicit Hermes native-window patch. Its original/modified source hashes and
additions are recorded in `vendor/rune/HERMES_PATCH.json`; patch scope and native
acceptance limits are documented in [`vendor/rune/HERMES.md`](vendor/rune/HERMES.md).
The copied distribution license texts also remain in `licenses/`.

The desktop-plugin compatibility window embeds the existing Hermes Desktop
React SDK, its runtime loader and its web components under the repository's
MIT license. This preserves executable React/DOM plugins in a separate native
WebView process. No Python agent code is included in the compatibility assets.

React, React DOM and the other libraries included by that SDK retain their
original licenses. The generated, versioned package inventory and copied
license/notice texts are in
[`assets/plugin-host/THIRD_PARTY_LICENSES.txt`](assets/plugin-host/THIRD_PARTY_LICENSES.txt).
[`assets/plugin-host/manifest.json`](assets/plugin-host/manifest.json) records
exact source-file hashes, the repository source commit, package-lock hash,
package versions, license-file hashes and asset/archive hashes.

Rebuild with `node apps/rune/scripts/build-plugin-host.mjs` after installing the
repository's locked npm dependencies. The script disables local environment
file loading, uses one React/React DOM singleton and atomically replaces the
embedded archive and its provenance files.

Wry and Tao provide the native compatibility window; the system browser engine
is WebKitGTK on Linux, WKWebView on macOS and WebView2 on Windows. Their Rust
package licenses are recorded by Cargo's package metadata and lockfile.

The shared native image preview directly uses resvg 0.45.1; its original MIT
and Apache license texts are retained in `licenses/resvg-0.45.1/`. The Linux
notification adapter directly uses zbus 5.19.0 and the locked futures-util
0.3.34; their original texts are retained in their named `licenses/` directories.
The complete dependency resolution remains in `Cargo.lock`; final distribution
assembly and platform packaging are tracked separately in the parity ledger.


Native audio-file playback statically includes the unmodified Sonic library,
pinned source `b93885dcb70aae50c6f76b0fe4e0868f029a077e`, under Apache-2.0.
Its complete source/header, license and source hashes are in
[`vendor/sonic/`](vendor/sonic/). The build does not download Sonic or run ffmpeg.

Four exact Symphonia 0.5.5 crates are retained under `vendor/` with local resource
validation patches: `symphonia-core`, `symphonia-metadata`,
`symphonia-format-riff` and `symphonia-format-isomp4`. Each retains the complete
MPL-2.0 text, original source files and a `HERMES_PATCH.json` recording the
registry archive checksum, modified files and before/after hashes. Distributions
must preserve these notices and make the corresponding modified MPL-covered
source available. The native audio ledger describes the narrow patch scope.
The synthetic compressed audio fixtures have their generation commands and
hashes in [`assets/test-audio/manifest.json`](assets/test-audio/manifest.json).
