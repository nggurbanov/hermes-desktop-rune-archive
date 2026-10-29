# Native Capabilities, sidebar and desktop extensions

This is an implementation inventory for the full desktop migration. It does not
assert that every retained desktop behavior has been ported. The native pages use
Rune Kit controls and the OpenCode themes; executable desktop plugins retain the
original React SDK in a separate native WebView window.

## Capabilities

| Surface | Implemented native behavior |
| --- | --- |
| Profile | Explicit override independent of the chat, frozen request profile, read generation invalidation on changes, busy/OAuth guards. |
| Skills | Installed and official catalog, search/preview/security scan, install/uninstall/update, serialized bulk toggles, learned/external content editing and archive. |
| Toolsets | Enable state, provider setup, model assignment, credential fields, post-setup actions, terminal backend readiness and selection. |
| Browser and Computer Use | Original browser-profile consent semantics, sparse config save, real platform readiness/health and grant/permission actions. |
| Agent plugins | Canonical keys, source inspection/full-SHA install, capability consent, scanner/update confirmation, enable/remove, schema-driven settings and serialized changed-secret-first saves. |
| MCP | Complete-map load requirement, command/URL/args/env/headers forms, add/edit/remove/enable/test, original per-tool whitelist/blacklist semantics, plugin-owned configuration restrictions, catalog credential setup and async installation. |
| MCP OAuth | Real random-port loopback callback, verified provider state/host/path, original start/callback/poll/cancel RPCs, explicit browser action, expiry and cancellation cleanup. Tokens stay on the agent. |
| MCP logs | Real scoped stdio/agent log sources, original server-section filtering, 2-second polling, error/retry and stale server/source generation fencing. |
| Hosted connectors | Catalog/accounts, alias/reconnect/remove/rename, typed setup prompts, approval/skip/stop, asynchronous operation recovery, exact MEMBER revision and organization/role restrictions, disabled-tool policy. |

Prompt-affecting capability changes retain the original next-conversation
semantics. Fixture browsing never falls through to a live agent; fixture writes
are disabled. Tests use synthetic results, temporary local callbacks and a local
WebSocket peer, without live agent writes.

## Sidebar

The native session sidebar reads canonical sidebar slices, with the documented
404 fallback to legacy profile-session slices. It retains pinned rows outside
limits, owner/source stamps, named Bot Chat exclusion, source sections, local
04:00 date groups, project/profile/status filters, branch collapse, sort/display
preferences, unread state, archived view and job/run controls. `is_active` alone
never becomes a working status.

Pin/read/rename/archive/delete updates use immutable mutation snapshots,
preconditions and request IDs. An ambiguous response retains the exact receipt in
private client storage. Connection principal, profile and target session stay
bound across an exact retry; a changed owner cannot silently inherit the write.
Historical scheduler runs open read-only using the original run verdict.

Manual drag reorder, PR/worktree topology, shift-click pinning and backend full
text session search still require their corresponding native services/gestures.
The visible search currently filters the loaded canonical sidebar rows.

## Executable desktop plugins

`plugin_host` owns a separate Tao/Wry process, its loopback document server and
JSON IPC. It embeds the real original runtime loader, React singletons, SDK,
components and styles. Plugins are executable trusted ESM, as in the original
Desktop contract; this is process/error isolation, not a new restricted plugin
language. Native parent authority never sends a token, gateway URL, cookie or
native ticket into JavaScript.

The native Plugins toolbar opens an installed-extension inventory or an explicit
local ESM source chooser. The installed inventory reuses original disk discovery,
package opt-in decisions, enable/disable and hot reload. Backend file/directory
changes are observed through real scoped polling readers. Private storage scoped to the authenticated source retains plugin decisions and
namespaced SDK state across profile navigation. Ambient window
lifetime and an explicitly selected capability profile are separate: the latter
controls requests/storage and cannot inherit the ambient chat.

RPC/REST remains authenticated in the native parent, with profile/connection
checks. The original `HermesGateway` and `JsonRpcGatewayClient` run over opaque
native socket receipts. Their request multiplexing, canonical adapter,
attach/replay, server-request closures and profile leases remain the original
implementation; socket closure is no longer tied to each individual RPC. The
native socket binds missing profile envelopes to its captured profile, while
leaving connection-global heartbeat/capability methods unchanged. A separate
observer provides the global gateway event tap, with duplicate sequence fencing.

Plugin namespace WebSockets use the same native broker, preserve JSON/binary
frames and enforce the original decoded-path boundary. Native-http grants and
remote credentials remain entirely in Rust. Multipart uploads retain the
original 256 MiB limit through acknowledged 8 MiB chunks, immutable request/file
snapshots, complete-byte validation and the real authenticated multipart API.

The original bundled Hermes Bots implementation is included with its groups,
hidden bots, room actions and Petdex. It has an explicit native Plugins entry;
it is not replaced by the simple native profile editor. Its TSX and noVNC
dependencies compile as ES modules with original license notices. Native parent
callbacks handle file selection/save, stored-session navigation, new chat drafts
and existing-agent activation. Async dialog/navigation replies retain a fresh
window token; explicitly authorized same-source profile navigation adopts its
scope, while manual owner changes close the old host. Agent activation preserves
the original SDK's retained profile client after the native parent accepts it.

Read-only registry metadata exposes labels/kinds and verified profiles, never
addresses or credentials. Saved source routes bind an immutable saved ID and
principal revision; unsaved sources use an opaque authenticated-owner ID. The
existing client keychain/probe/SSH service resolves retained leases without
changing the foreground. Retaining the current SSH source promotes its existing
forward, preserving the endpoint of already-open sockets. A foreign source owns
its separate forward; final release, removal, revision changes and tunnel exits
retire only its own leases. Failed activation cannot commit registry selection
or close the previous foreground.

RPC, REST, uploads and namespace/core sockets capture the selected private
connection/lease independently of the ambient window. Equal profile names and
event sequence numbers at different sources remain distinct. Native source
navigation resolves and verifies the target profile, commits the verified lease,
awaits the new source/profile hydration, then adopts the same fresh window token.
Manual owner changes still close the old host. The SDK updates its default route
only after its original retained-client activation succeeds. Uploads or pending
HTTP operations block an explicit owner change until their receipts finish.
Saved plugin route/storage identity survives an ephemeral SSH-port change;
credential rotation still changes its private storage principal.

Remote/local saved roster sources are probed through the existing service; an
undialed SSH source remains connect-on-demand until an explicit route operation.
Unknown, changed or removed routes fail explicitly instead of falling back to
another source. Transient failed probes may retry after a bounded delay. The
currently leased background profile inventory still needs periodic fresh
metadata reconciliation (including all original avatar/UI metadata), and native
cross-source parent navigation needs full window-level acceptance in addition to
the service/SDK contract tests below.

The compatibility window also needs the native main pane geometry, contributed
data-engine integration and plugin preview execution grants before complete
plugin parity can be claimed. Settings render callbacks and sub-pages already
render in the compatibility window.

Build provenance and copied third-party licenses are recorded in
`assets/plugin-host/manifest.json`, `THIRD_PARTY_LICENSES.txt` and
`THIRD_PARTY_NOTICES.md`. Build inputs exclude agent Python code.

## Verification

The native executable smoke renders a synthetic ESM plugin using the real SDK
Button and React hooks, clicks it, verifies its profile-scoped RPC, deduplicated
live events, binary multipart chunks, namespace streams, namespaced storage and
unload disposal. Its canonical lane verifies actual identified session creation,
snapshot projection and a revision-fenced `session.title` → `session.mutate`
translation. The bundled Bots smoke uses two synthetic profile identities, opens the original
group member picker, and verifies the original Petdex gallery/thumb RPCs and UI.
The cross-source smoke runs the original profile-route/retain/activation APIs
with duplicate profile names on two controlled sources, verifies successive RPCs
share the same runtime socket, and checks event identities do not collapse.
It also checks an already-enabled plugin registers once, preserving the original
runtime loader's decisions:



```
xvfb-run -a node apps/rune/scripts/check-plugin-routing.mjs
xvfb-run -a node apps/rune/scripts/check-plugin-host.mjs --snapshot /tmp/hermes-plugin-sdk-native.png
xvfb-run -a node apps/rune/scripts/check-plugin-host.mjs --canonical --snapshot /tmp/hermes-plugin-sdk-canonical.png
xvfb-run -a node apps/rune/scripts/check-builtin-bots.mjs --snapshot /tmp/hermes-builtin-bots-native.png
xvfb-run -a node apps/rune/scripts/check-builtin-bots.mjs --group-dialog --snapshot /tmp/hermes-builtin-groups-native.png
xvfb-run -a node apps/rune/scripts/check-builtin-bots.mjs --petdex --snapshot /tmp/hermes-builtin-petdex-native.png
```

Rust contracts additionally cover retained-forward reuse, last-reference cleanup,
credential revision fencing, activation commit failure, exact frozen HTTP/socket
target selection, unknown/unserved route rejection and descriptor redaction.

It uses synthetic backend replies and no microphone or live agent. Its optional
Linux screenshot uses GDK on the explicit test display, avoiding session-portal
capture of another desktop. Actual Wry/WebKitGTK feature probing found secure
loopback/getUserMedia available and both standard/prefixed RTCPeerConnection
absent on this Linux image. Native live voice uses its separate media engine;
this plugin host makes no WebRTC capability claim from compile success alone.
