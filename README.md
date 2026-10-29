# Hermes Desktop Rune

A standalone desktop client for existing Hermes agents, extracted from
[NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent).

The first phase keeps the existing Electron interface. Rune parity and a later
conversation-focused redesign follow after standalone extraction is verified.

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

Build/runtime/packaging separation is in progress. Backend directories remain
until independent builds and external local, URL, and SSH acceptance pass. Their
presence is temporary and does not make them desktop build inputs. Do not run the
upstream agent installer or updater to build this client.

The default Vitest lane is client-only. Python/server parity checks use the
explicit `npm run test:external` lane and require a selected external installation.
The inherited installer/update E2E lane and shared agent-home plugin/preference
ownership still need review before backend deletion. See
[the E2E guide](apps/desktop/e2e/README.md) for external-runtime testing.

Upstream MIT copyright and license notices are preserved in [LICENSE](LICENSE).
