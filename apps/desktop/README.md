# Hermes Desktop Rune

The current interface is the extracted Electron desktop from Hermes. It connects
to separately installed local or remote agents. Rune parity will follow; the
interface has not been redesigned during extraction.

See the [repository README](../../README.md) for setup and connection selectors,
[BUILDING.md](BUILDING.md) for packaging and [e2e/README.md](e2e/README.md) for
isolated external-runtime tests.

```sh
# From the repository root
npm ci
npm run dev
npm run typecheck
npm test
npm run pack
npm run test:package --workspace apps/desktop
```

Agent execution, profiles, tools and conversation history belong to the connected
Hermes. The app owns its connection records, preferences, runtime desktop plugins,
window state and logs in its own Electron userData. Client boot diagnostics live
at `<userData>/logs/desktop.log`; they do not share the agent log directory.
Closing a local desktop connection leaves the canonical gateway running.

If no local Hermes is installed, choose an existing URL or SSH connection in the
setup screen. The client does not bootstrap, update or uninstall an agent.
Client updates are manual from this fork's releases.

On macOS, the optional two-Command screenshot shortcut requires Input Monitoring
and Screen & System Audio Recording permissions. It attaches the screenshot to
the active composer; review it before sending. macOS and Windows native packages
still require host verification.

For code changes, read AGENTS.md and src/AGENTS.md. Keep frontend state scoped to
its owning connection/profile/session and keep Node access behind typed preload
capabilities. Prefer behavior tests; server-source text is not a client contract.
