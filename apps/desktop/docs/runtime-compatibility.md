# External Hermes compatibility boundary

Hermes Desktop Rune is a client, not an agent distribution. Its initial protocol
snapshots come from the upstream revision in `desktop-baseline.json`. Ordinary
builds keep those artifacts unchanged and require no backend source checkout.
A protocol update is a deliberate compatibility change, not an upstream merge.

## Connection modes

- Local: discover an installed launcher, or use the documented external source
  and prepared Python selectors. Reject this repository as an agent runtime.
  `hermes gateway ensure --json` resolves the canonical owner; private control
  tickets authenticate native WebSocket and HTTP connections. Detaching or
  closing the client does not terminate the gateway.
- URL: use the selected external gateway's URL and its existing token or OAuth
  path. Reconnection retains that connection and profile identity.
- SSH: use the selected host's installed Hermes and the existing SSH forwarding
  lifecycle. Desktop may own the dedicated remote serve process/tunnel for that
  connection. It does not install or upgrade the remote Hermes.

Remote resolution or authentication failure must never start a local owner.
Missing native runtime capabilities must fail visibly rather than silently
starting an independent local serve process, changing session source or dropping
requested session policy. Gateway APIs own sessions, profiles, tools and history.

The exact wire definitions are the retained OpenRPC/TypeScript artifacts in
`apps/shared/src`. Client adapters and capability checks live beside their API
consumers. GUI session creation, event replay, prompt receipts and native HTTP
credentials are described in `canonical-gateway-protocol.md`. Those contracts
are stronger than a version string alone; no universal minimum runtime version
is claimed from the current verification.

## State ownership

Electron userData holds client connections, preferences, desktop plugins, logs
and window state. Agent homes retain agent configuration and state. Explicit
settings actions may change the selected agent through existing APIs; startup
must not migrate/delete agent-home plugin folders. Unified packages installed in
the local agent may be scanned read-only and their desktop halves copied into
client storage. This is not synchronization of plugin stores between machines.

## Verified versus pending

Linux local native attachment to Chikibryak running Hermes 0.22.5 was verified;
loading profiles/history/models and closing the viewer preserved the canonical
gateway instance. Full live chat-cycle acceptance was waived by the user.

URL authentication uses real loopback WebSocket tests; SSH routing, forwarding
and lifecycle use controlled fixtures and shell subprocesses. Those checks do
not establish interoperability with a real remote host. Live remote URL/SSH
acceptance and native macOS/Windows packaging remain pending release checks.
