# Third-party notices

The native Rune shell uses anomalyco/rune, rune-kit and the OpenCode theme.
Their license and bundled font licenses are in `licenses/`.

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
