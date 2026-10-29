# Hermes Desktop Rune

A standalone native Hermes desktop client. Rune and OpenCode supply native
controls, fonts and both appearances; Hermes retains its navigation, identities,
conversation ownership and split/group layout. Agents remain independently
installed and managed. The retained Electron client is the behavior baseline.

**Full functional parity is in progress.** The app now sends and streams real
turns, handles stop/admissions/approval/clarify, edits agent settings and supports
native audio. This is a development build, not a completed migration. See
[functional parity](docs/functional-parity.md) for implemented behavior and open
acceptance work; the feature inventories retain the complete target scope.

## Build and run

Rust 1.95+ is required. On Debian/Ubuntu the native build needs the Rune graphics
headers, ALSA, GTK/WebKitGTK 4.1, Opus and the AEC build toolchain:

```sh
sudo apt install libxkbcommon-dev libwayland-dev libasound2-dev \
  libwebkit2gtk-4.1-dev libopus-dev cmake meson ninja-build libclang-dev llvm \
  gstreamer1.0-nice gstreamer1.0-plugins-bad
rustup component add llvm-tools-preview
npm run dev:rune
npm run dev:rune -- --local-home /absolute/path/to/hermes-home
npm run dev:rune -- --gateway https://existing-gateway.example/prefix
```

A GPU/Vulkan driver is needed; Mesa software Vulkan works in a VM. XDG portals
provide native dialogs on Linux. Local private-ticket discovery is currently
verified on Linux; macOS/Windows local discovery and distribution acceptance
remain open. The minimum native window is 1024×520 logical pixels.

Local attach never starts, installs or updates an agent. Saved Local, Remote and
SSH connections use client-owned metadata and OS credential storage. SSH forwards
to an existing gateway with strict host-key validation. A one-time URL token can
also be entered in the secure form or supplied through `HERMES_GATEWAY_TOKEN`.
The client does not store credentials in its JSON preferences or plugin child.

The Command Center exposes explicit backend maintenance/update/restart actions;
these operate the selected external agent only after the corresponding user
invocation. They are independent from desktop installation and startup.

## Current surfaces

- Hermes Sessions/Bots navigation, canonical Bot Chats, conversation tabs,
  sidebar sources/pins/filtering, bot/profile identity management and archives.
- T3 Code-style multiline composer, provider/model search and favorites,
  a compact model summary and one reasoning/tier/approval popover, real file
  upload and durable prepared sends. Hermes dictation and voice actions remain.
- Native rich transcript, earlier history, canonical edit/retry/restore/branch
  with durable mutation recovery, copy/quote/read aloud, tools and images.
- Separate native Rune subagent windows: lazy read-only history/streaming,
  focus/reuse, theme propagation and viewer-only closure.
- Settings, Capabilities, Messaging, Artifacts, Scheduled jobs, saved Connections
  and Command Center with real scoped read/write operations and failure states.
- Dictation into drafts, STT/Hermes/TTS conversation, wake listening, spoken
  replies and GPT-Live. Linux uses native WebRTC/Opus with WebRTC AEC3; actual
  provider/device/platform acceptance limits are in [voice parity](docs/voice-parity.md).
- Files tree/text editor with read-before-save, conflict preservation, local
  Rename/Trash, scoped remote access, image previews and streaming download.
- Persistent split/group pane layout with native docking, stacked tabs,
  minimize/close/reopen and divider resizing. More original presets/tile/window
  interactions remain in scope.
- Original React Hermes GUI SDK/runtime in an owned native WebView process,
  including bundled Bots UI. Native Rune remains the application shell. The SDK
  uses authenticated opaque native socket/REST brokers. See [notices](THIRD_PARTY_NOTICES.md).

Terminal and Review implementation/acceptance is tracked in the parity ledger;
no hidden placeholder counts as a completed feature. Client state lives under
its own OS state directory, never under an agent home. Drafts and overlays are
scoped by connection principal/profile/stored session. Unknown send outcomes are
reconciled with their original identity rather than automatically resent.

## Verification

```sh
cargo test --locked --manifest-path apps/rune/Cargo.toml
cargo build --locked --manifest-path apps/rune/Cargo.toml
node apps/rune/scripts/check-scenes.mjs
node apps/rune/scripts/check-composer.mjs
node apps/rune/scripts/check-pages.mjs
node apps/rune/scripts/check-chat.mjs
node apps/rune/scripts/check-decks.mjs
node apps/rune/scripts/check-client-settings.mjs
node apps/rune/scripts/check-notifications.mjs
xvfb-run -a node apps/rune/scripts/check-preview.mjs
xvfb-run -a node apps/rune/scripts/check-subagent-windows.mjs
xvfb-run -a node apps/rune/scripts/check-plugin-host.mjs --canonical
xvfb-run -a node apps/rune/scripts/check-builtin-bots.mjs
```

Native `--scene` states use synthetic data and disable backend mutation. Useful
scenes include `reader`, `chat-working`, `chat-decisions`, `bot-manager`,
`connections`, `settings`, `capabilities`, `messaging`, `artifacts`, `cron`,
`command-center`, `terminal`, `file-preview`, `layout`, `composer`, `models`,
`reasoning` / `permissions` (the same combined model-settings menu), `rich-chat`,
`subagents`, `voice` and `voice-engine`. Combine with `--theme`
`light`/`dark`, `--tree /tmp/tree.json` and `--snap /tmp/screenshot.png`.

`--check-connection ... --check-pages` is a separate read-only probe. Real agent
mutations, provider calls, prompts and human microphone access are excluded from
the automated acceptance suite. The optional virtual audio test uses its own
Pulse server/null sink and must be explicitly invoked as documented in the
voice ledger.
