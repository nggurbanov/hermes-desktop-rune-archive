# Hermes Desktop Rune

A standalone desktop client for existing Hermes agents, extracted from
[NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent).

The standalone Electron interface is the behavioral baseline. The native Rune
port preserves its panel placement and logic while reusing the OpenCode Rune
visual system. A later phase will redesign the conversation workflow.

The first native increment is a read-only gateway viewer in
[apps/rune](apps/rune/README.md). Run it with `npm run dev:rune`.

## Development

Use Node 26+ and npm 11.17+ (npm 12.0.2 is the current build toolchain).

```sh
npm ci
npm run dev
npm run build
npm run typecheck
npm test
```

Only `apps/desktop` and `apps/shared` are npm workspaces. An application build
needs no Python agent checkout or agent dependency environment. Native helpers
still require the host platform toolchain; on Linux install `libx11-dev` and
`libxi-dev` for the modifier-key helper.

## Connect an existing Hermes

The client discovers separately installed local Hermes launchers and can connect
to remote gateways by URL or SSH. It does not install or bundle an agent. A
missing local installation opens the existing connection setup experience.

Existing explicit development selectors remain supported:
`HERMES_DESKTOP_HERMES` for an external launcher, or
`HERMES_DESKTOP_HERMES_ROOT` plus `HERMES_DESKTOP_PYTHON` for an external
source installation and its prepared interpreter. The desktop checkout cannot
be selected as its own agent runtime.

Hermes owns agent execution, profiles, sessions, memory, and tools. Closing this
client disconnects its viewer without stopping the canonical local gateway.
SSH connections retain the existing dedicated-server ownership behavior.
See [the runtime contract](apps/desktop/docs/runtime-compatibility.md) for
protocol requirements, state ownership and verification limits.

## Packaging

```sh
npm run pack                    # unpacked application for this host
npm run dist                    # host installer/archive
```

The app uses its own OS identity, window/connection state and `hermes-rune://`
protocol. It does not replace the official Hermes app or register the official
`hermes://` protocol. Updates are currently manual through this repository's
releases; the desktop never updates or uninstalls the connected agent.

macOS defaults to ZIP distribution. Optional DMG packaging requires an explicitly
prepared dmgbuild tool tree; it does not provision the Hermes Python environment.
Windows and macOS packaging still need verification on their respective hosts.

## Extraction status

The upstream reference is pinned in [desktop-baseline.json](desktop-baseline.json).
Generated gateway TypeScript/OpenRPC and slash-command fallback definitions are
retained client compatibility snapshots; ordinary builds never regenerate them
from Python server modules.

The repository contains only the desktop client, its shared gateway client,
JavaScript build tools and desktop documentation/tests. Python/server sources,
the upstream installer, TUI, dashboard, website and legacy release CI are removed.

Preferences, desktop runtime plugins and client logs live under this app's own
Electron userData directory. Locally installed unified agent packages may supply
desktop plugin halves through read-only source scans; generated copies belong to
the client. Starting the client never moves plugins out of an agent home.

The default Vitest lane is client-only. Optional Python/server compatibility
checks use `npm run test:external` and a selected external installation. See
[the E2E guide](apps/desktop/e2e/README.md). The old bundled-agent installation and
self-update tests were removed with those features; standalone startup, updater
policy, connection and packaging contracts remain covered.

Local attachment to Chikibryak (Hermes 0.22.5) was verified on Linux. URL/SSH
contract tests use controlled fixtures; live remote-host acceptance remains
unverified. The user waived live full-chat-cycle acceptance for extraction.
Clean-source installation, typechecks, all 16,391 client tests (30 platform
skips), Linux unpacked packaging and actual no-agent startup passed on
2026-10-29. Windows and macOS packaging remain unverified. The first read-only Rune increment is implemented and verified on Linux; full
parity has not been established.

Upstream MIT copyright and license notices are preserved in [LICENSE](LICENSE).
