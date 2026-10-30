# Native platform and distribution: next slices

Read-only audit of the current Rune client and retained Electron baseline;
no Cargo build, installation, download, remote access, GPU or device exercise
was performed for this map. A platform `cfg` branch or dependency feature is not
a successful compile/link or native acceptance result. The functional ledger
and immutable-binary acceptance notes remain the authority for executed gates.

## Observed starting point

- `.github/workflows/rune-client.yml` runs only Ubuntu 24.04: Rust 1.95.0,
  provenance for Rune, formatting, default-profile tests/build and five native
  scene/interaction scripts. It has no macOS/Windows job, release-profile build,
  installer assembly, uploaded release artifact, signing or update exercise.
- Root `build:rune` requests a release executable. Root `pack`/`dist` still invoke
  the retained Electron workspace. No Rune `.app`/Info.plist/entitlements, Windows
  package/resources or Linux desktop-entry/AppImage recipe was found.
- `apps/rune/Cargo.toml` pins the client resolution and local dependency patches.
  `build.rs` compiles vendored Sonic C through `cc`. Linux additionally builds
  bundled WebRTC AudioProcessing and links GTK/WebKitGTK/ALSA/D-Bus dependencies.
  Rune selects Metal on macOS and Vulkan/DX12 on other native targets; Wry uses
  WebKitGTK, WKWebView or WebView2. None of this establishes another host's build.
- This machine is Linux x86_64. The only installed Rust target is
  `x86_64-unknown-linux-gnu`; rustfmt, Rust source and LLVM tools are installed.
  GCC/cc, CMake, Meson, Ninja, LLVM, pkg-config, Xvfb, xdotool and D-Bus helpers
  are available. pkg-config found GTK3, WebKitGTK 4.1, ALSA, D-Bus, X11,
  xkbcommon, Wayland and Opus. No Xcode/codesign, Windows MSVC SDK or cross C
  compiler was found in PATH. `/usr/bin/signtool` belongs to `libnss3-tools`,
  not the Windows signing SDK.
- Read-only ELF inspection of the earlier accepted Linux debug candidate
  `5f99ea73ab734ce4796106db0ff086af8c422d962ab3862d0fdedb073adb6eb9`
  shows direct WebKitGTK 4.1, GTK3/GDK, libsoup3, JavaScriptCoreGTK, GLib/GIO,
  D-Bus, ALSA, libstdc++ and glibc dependencies. It is not a self-contained
  Linux distribution, and this does not record a final release dependency set.

## 1. Reproducible native compile/link matrix (P0)

Create separate real-host Linux, macOS and Windows build jobs with the locked
Rust 1.95 toolchain, exact source/provenance verification, formatting and target
contracts. Run both development diagnostics and the actual release profile.
Verify the architectures actually distributed; an Apple Silicon build does not
validate an Intel slice or a universal bundle. Cross-checking metadata on Linux
cannot substitute for an MSVC/Apple SDK link or native startup.

Use `Cargo.toml`, `Cargo.lock`, `build.rs`, vendored Rune/winit/Symphonia/Sonic
manifests and `THIRD_PARTY_NOTICES.md`. Extend CI provenance to every declared
local patch/archive, not just Rune. Resolve compiler/linker errors on the real
target rather than replacing the feature with an unavailable stub. In particular
exercise Sonic's C toolchain, Opus, CPAL/Rodio, Wry/Tao, native dialogs, keyring,
portable-pty, graphics backends and all `cfg(test)` targets that differ by host.
Capture toolchain/SDK/library versions and the immutable output hash.

Linux can provide clean locked builds, ELF dependency inspection and private
X11/software-GPU regression here. Still require a clean supported Linux image,
release linkage/glibc compatibility, real Wayland/compositor behavior and actual
hardware acceptance. The current X11 pointer fix is X11 evidence only.

## 2. Desktop-only packages and installed identity (P0)

Build native packages rather than routing Rune through Electron packaging.
Retained authorities/reuse inputs are `apps/desktop/product-identity.cjs`,
`electron-builder.config.cjs`, `assets/icon*`, `scripts/standalone-packaging.test.mjs`,
`prepared-packaging.mjs`, and the standalone install-stamp/feed contracts.
The retained formats are Linux AppImage, macOS app ZIP and Windows MSIX; preserve
the applicable product/install behavior without copying agent payload builders.

Package the native executable, required system/runtime dependency policy,
embedded SDK/assets, icons, notices and modified dependency source obligations.
Prove a clean source export can build without developer caches and an installed
client launches with no Hermes agent, Python agent environment, Node/npm or
repository checkout present. External tools such as Git/OpenSSH remain explicit
capabilities with visible missing-tool recovery; they must not trigger agent
installation. SDK WebView and spectator children use `current_exe` in
`plugin_host.rs` / `subagents/process.rs`; verify those child modes from the
installed path, including spaces/non-ASCII characters and read-only install dirs.

For Linux, install a consistent desktop-entry/icon/application identity and
protocol handler, then test desktop launch and notification activation. The
native notification transport currently sends `desktop-entry=hermes`, while the
retained standalone product identity is `io.github.nggurbanov.hermes-desktop-rune`;
the package and adapter must agree. Resolve runtime GTK/WebKit/graphics/portal
requirements on a clean machine, not only this provisioned VM.

For macOS, assemble a real `.app` with executable/bundle identity, version/icon,
usage descriptions and narrowly appropriate entitlements. Reuse the retained
microphone/local-network copy and signing scripts/contracts as references;
Electron JIT/library-validation entitlements are not automatically Rune's needs.
Sign every shipped executable/library and native child mode with hardened-runtime
requirements, notarize the exact artifact and staple/verify where applicable.
Run Gatekeeper/quarantine install-launch acceptance on macOS.

For Windows, add native executable resources/subsystem policy, package identity,
manifest/icon, protocol and notification activation registration, runtime/WebView2
deployment policy and applicable VC runtime dependencies. Produce and validate
MSIX with real Windows tooling, certificate/publisher consistency and install /
upgrade / uninstall tests. Having `windows-native` keyring or Wry features enabled
does not establish any of those properties.

## 3. Local attach, secrets and durable files (P0 implementation gaps)

`src/gateway.rs::Gateway::connect(Connection::Local)` and `gateway/local.rs`
implement private local-ticket attach only on Linux. Other platforms explicitly
return an unsupported-local-attach error. Implement the installed Hermes host's
actual private control transport/discovery on macOS/Windows, retaining canonical
home/profile/instance/authority checks, fresh ticket scopes, bounded deadlines,
multiplexed secondary profiles and credential-free endpoint publication.
Use the retained `apps/desktop/electron/local-gateway.ts` and its ticket-identity,
deadline, symlinked-home and restart/detach tests as behavioral references.
Do not copy a lifecycle/provisioning fallback into normal attach. Closing every
client must leave the external canonical owner running.

`client.rs::state_dir` has Linux/macOS/Windows paths and Cargo enables OS-keyring
backends, but native privacy, keyring-unavailable/locked recovery and persistence
are not accepted on the other hosts. Exercise `client::private_write`,
`client/file_lock.rs`, startup/window/editor stores and saved registry secrets
with real concurrent processes on each filesystem: replace-existing atomicity,
permissions/ACLs, locked files, crash/reclaim, stale revision, future schema,
unknown fields and failed-write retry. A Unix-only permission check passing on
Linux is not Windows ACL or macOS Keychain acceptance.

## 4. Notifications, devices, terminal and native windows (P1)

| Area | Existing implementation to reuse | Remaining native-host gate |
| --- | --- | --- |
| Notifications | `notifications/service.rs`, Linux broker/registry/transport, original SDK callback ownership | macOS UserNotifications and Windows toast adapters are explicitly unimplemented; add permission/application identity, body/button/dismissal callbacks and broker parity. Test installed activation, focus/quiet/replay, denied permission, source/window retirement and same-process multiwindow dedup. Linux D-Bus private-daemon proofs do not establish real desktop policy. |
| Dictation, voice and playback | `audio/device.rs`, Linux `native_live.rs`/AEC3; non-Linux `live_web.rs`/Wry media; shared live protocol, media player, Sonic/Symphonia and sound bank | Real microphone permission denial/grant/revocation, hotplug/default-device changes, sample formats, input/output routing, echo/double-talk, interruption and cleanup. macOS/Windows WebView media path exists but has not been accepted there. Separate private virtual-device checks from physical/provider acceptance. |
| Terminal, Git and SSH | `terminal/worker.rs`, portable-pty, POSIX/PowerShell quoting, `review/git/process.rs`, `connections/ssh.rs` | macOS PTY/login-shell behavior; Windows ConPTY, cmd/PowerShell, Unicode/IME, resize, Ctrl+C/paste/selection and path quoting; installed GUI PATH/tool recovery; actual controlled SSH hosts, strict known-host checks, forward restart and final owned-child teardown. Linux-only `/proc` cwd discovery does not prove equivalent host cwd behavior. |
| Windows, input and host integration | `desktop_window.rs`, `app/windows*`, pinned Rune Runtime/winit, native peer/startup scripts | Real Metal/DX12/Vulkan surfaces, native menus/Command-vs-Control, clipboard text/images, IME, file dialogs/drag-drop, focus/minimize/occlusion, multi-monitor/DPI/work-area clamping, full topology restore, dirty-window Quit and OS shutdown. Ordinary peer, subagent and Wry child lifecycle all need installed-host proof. Linux Xvfb geometry is not macOS traffic-light or Windows frame acceptance. |

Include native/SDK speech ownership, remaining global hotkeys/tray/Quick Entry /
HUD and hardware feedback from the full ledger; current in-window keybindings
are not global OS shortcuts. Use the pinned framework's platform helpers only
where the actual Hermes consumer is integrated and observed. Existing detailed
limits are in `voice-parity.md`, `native-notifications-parity.md`,
`native-plugin-notifications.md`, `native-terminal-parity.md`,
`native-audio-player-parity.md` and `native-full-windows-parity.md`.

## 5. Desktop updates and release acceptance (P1)

There is no Rune desktop updater/distribution pipeline yet. `operations.rs`
`/api/hermes/update*` targets the connected external agent; it cannot count as
updating this standalone desktop. Retained references are
`apps/desktop/electron/updater/*`, `app-updater.ts`, `app-installer-file.ts`,
`scripts/notarize*.mjs`, `scripts/mac-sign.mjs`, `scripts/sign-msix.mjs`,
`update-feed.cjs` and update UI/store contracts.

Transfer channel/feed identity, check/download/progress/error/retry behavior,
artifact verification, clean preflight/flush/quit, install/relaunch, rollback /
recovery and user preferences. Preserve OS/package-manager-managed update modes.
Use a desktop-only helper/native API where required; an old bundled-agent Python
updater is not an acceptable dependency. Exercise local controlled artifacts and
failure points on each OS before a release feed is considered accepted.
Updating the client must not restart or update a separately managed agent.

For each shipped platform, publish one evidence record tying source revision,
locked dependencies, native toolchain/SDK, artifact hash, notices/signatures and
clean install/update/uninstall results to the exact binary. Re-run the ordinary
Hermes feature/window gates from that installed artifact, then the host-specific
attach/keyring/notification/device/terminal gates. A Linux release can be fully
verified on Linux; macOS signing, Gatekeeper, Keychain, permissions and Metal,
and Windows MSIX/WinRT, Credential Manager, ConPTY and native window behavior
require those real hosts. None is marked complete by this audit.
