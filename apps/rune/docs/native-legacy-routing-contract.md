# Legacy routing contract

Scope update: managed Nous/Hermes Cloud portal, organization discovery and hosted-agent selection are excluded from this parity phase. Existing Cloud configuration is preserved without automatic connection. Generic remote URL OAuth and access headers remain in scope. The baseline descriptions below are retained as historical reference.

This is the retained Electron behavior that the next native resolver must
implement. Saved-source fleet UI does not close this dependency. Authority:
`electron/desktop-remote-route.ts`, `connection-config.ts`,
`connection-route-identity.ts`, `connection-registry.ts` and
`src/app/session/new-session-route.ts` under `apps/desktop`.

An explicit logical `connectionId: null` selects the legacy profile routing
door. It is distinct from no default and from a registered Local source. The
first Connection captured in a process cannot serve as its durable resolver.

## Selection

For the requested profile, select in this order:

1. Eligible per-profile SSH override with a normalized host.
2. Eligible per-profile URL/Cloud override with a nonempty URL.
3. Environment URL with its required token.
4. Global SSH settings.
5. Global URL/Cloud settings.
6. Registry primary transport when global settings select local.
7. Local attachment.

An incomplete stored profile block is ineligible; a selected global SSH block
without a host and an environment URL without its token are errors. Invalid
selected dial data,
authentication failure or an absent requested profile cannot fall through to
another source/profile or start a local agent. Explicit registered-source
requests retain their own source and bypass this general legacy ladder.

Per-profile `mode: local` clears an override and inherits the global route. It
does not force a local backend. `savedSsh` is dormant editor memory. A registered
Local source remains genuinely local under a global remote; however the exact
baseline `resolveRegistryLocalRoute` deliberately delegates an explicit
per-profile override to that legacy route (#90477). Both rules must survive.

Make Primary changes registry startup policy without rewriting an explicit
legacy global remote. Legacy global/profile settings therefore need independent
transport snapshots, rather than pointers to editable registry rows.

## Identity, aliases and credentials

Freeze complete route identity before dialing. Profile overrides can claim a
registry id only for one unique full-envelope match; global settings can claim
only an exactly matching primary. Environment routes never claim registry ids.
The baseline matcher compares normalized URL, auth mode, canonical access
headers, Cloud kind/org and token in token mode; OAuth session tokens are not
that identity. SSH compares normalized host/user/port/key path/Hermes path/remote
profile, excluding the bootstrap dashboard token. URL remoteProfile is absent
from this Saved-ID matcher but belongs in the logical route namespace/version.
URL-only deduplication cannot establish equality. A legacy descriptor may carry
a matching registry id while remaining logically legacy. Exact refinements and
typed implementation slices are in `native-legacy-implementation-plan.md`.

Desktop profile labels and remote profile names are separate namespaces.
Preserve `remoteProfile` for URL/Cloud and SSH, and SSH `remoteHermesPath`.
Translate only declared self-profile fields and filters, including sidebar
`recents_profile`; keep `all` and other explicit cross-profile selectors.
Validate response ownership in the backend namespace before projecting the UI
label. Test the baseline's explicit `default` mapping separately on SSH,
request filtering and aggregate session reads. Never recursively replace equal
strings throughout JSON. Canonical Bot Chat belongs to the resolved backend
profile and cannot be guessed from recency.

Current native fixed-port SSH forwarding is not the full Electron installed
Hermes bootstrap. OAuth/Cloud, access headers and encrypted Electron credentials
also require their actual consumers. An unresolved capability must be visible,
not silently converted into anonymous/token HTTP or a guessed SSH port.

## Native ownership and migration

Co-locate a versioned legacy routing block with the native connection registry
to use one existing OS transaction. Independent global/profile route entries
carry stable ids/revisions, transport configuration and keyring references.
Reuse the dial/probe/tunnel/lease machinery, with distinct Saved/Legacy owners.
Changing one override retires only its old owner revision. Observation is local
metadata work and never performs fleet network polling.

The root coordinator retains the logical route separately from the actual
Connection, validates the exact requested profile and preserves the outgoing
workspace/lease until accepting a correlated result. Cancellation, owner/input
changes and changed scope revisions fence delayed results and metadata commits.

Import is an explicit selected-file/bytes operation with a safe preview and
old-id to UUID mapping. Leave the source file unchanged. Plain or safeStorage
envelopes cannot be passed as bearer strings; recover through the actual
credential bridge or request re-entry. Store tokens and access-header values in
the OS keyring. Do not copy opaque foreign fields into credential-free JSON;
unknown native fields survive locked patches. Invalid/future files preserve
their bytes and block writes/resolution, rather than becoming absent settings.

Window/default descriptors never contain credentials, OAuth tickets/cookies,
temporary forward ports or runtime epochs. Failed writes retain the exact user
intent for retry. Tests use private files, fake credential/tunnel services and
strict local fixture gateways, never real user configuration or live agents.
