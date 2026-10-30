# Proposed native legacy routing implementation

Planning only; nothing in this document is an implementation or acceptance claim.
The current native candidate is unchanged. Read alongside
[native-legacy-routing-contract.md](native-legacy-routing-contract.md).

## Baseline decisions to preserve

Authority is the retained `desktop-baseline.json` implementation:
`electron/desktop-remote-route.ts`, `connection-route-identity.ts`,
`connection-config.ts`, `connection-registry.ts`, `window-connection-route.ts`,
`profile-session-routing.ts` and their consumers in `electron/main.ts` under
`apps/desktop`.

| Decision | Required behavior |
| --- | --- |
| Logical door | Explicit `connectionId: null` is Legacy; absence of a device default is a different state. A registered source is an explicit door, not a suggestion to the Legacy resolver. |
| Legacy selection | Eligible profile SSH, eligible profile URL/Cloud, environment URL/token pair, global SSH, global URL/Cloud, non-local registry primary when global mode is local, then local attachment. |
| Override eligibility | `profileSshOverride` requires a normalized host; `profileRemoteOverride` requires a nonempty URL. An incomplete stored profile block is not selected. Once a route is selected, malformed dial/auth data fails that route. Global SSH without a host and environment URL without its token are errors. |
| Local profile entry | `mode: local` clears the active override and inherits global routing; `savedSsh` is dormant editor memory. An own local entry also matters to `resolveProfileBackendRoute` when the primary itself is remote. |
| Registered Local | Global remote mode cannot turn This device into that remote. However, an explicit profile override delegates to the Legacy profile route, as `resolveRegistryLocalRoute` requires. A concrete missing local profile under the forced-local branch is refused; enumeration does not launch it. |
| Pre-dial provenance | Profile overrides may claim exactly one complete matching Saved envelope; global routes may claim only their matching primary; environment routes never claim an ID. Matching is not `registryScoped` authorization. |
| No default fallback | A chosen default/profile/source that cannot resolve remains an error. It must not dial an ambient current connection, another profile, a replacement Saved revision, or a locally installed agent. |

Two refinements to the earlier contract matter during implementation:

- The baseline provenance matcher is field-specific. URL/Cloud comparison uses
  normalized URL, kind, auth mode, canonical access headers, Cloud org and token
  in token mode; OAuth session tokens are not that identity. SSH comparison uses
  normalized host/user/port/key path/Hermes path/remote profile, excluding the
  bootstrap dashboard token. URL `remoteProfile` is not part of this Saved-ID
  matcher, but it **must** be part of the logical route's namespace/revision.
  Unreadable foreign secret envelopes cannot establish equality or uniqueness.
- Alias mapping is request-family-specific. SSH argv maps explicit `default`
  to the remote root (no `--profile` argument), never a `conn:...` pool key.
  `translateSelfProfileQuery` maps an explicit self selector to backend
  `default`, including sidebar `recents_profile`. Conversely,
  `remoteProfileQueryScope("mara", "default")` returns `mara` in the retained
  aggregate-session path. Its narrowly classified unknown-profile 400/404
  compatibility retry stays on the **same gateway**; it is not a generic
  auth/transport/missing-default fallback. Invalid/blank configured mappings
  normalize to same-name behavior. Port these separate policies and fixtures,
  rather than turning every matching JSON string into one backend name.

The Electron config reader's corrupt-file-to-local catch is deliberately not
copied: the approved native storage contract preserves invalid/future bytes,
shows an error, retains failed edits, and does not treat unreadable data as an
empty configuration.

## Interfaces and ownership

Add a typed `legacy` block to the existing private registry, with an independent
schema version, global entry and profile-keyed entries. Each entry has an edit
revision, stable route ID, transport snapshot, credential references and
optional backend-profile mapping. Retain explicit local/inherit and dormant SSH
editor state. The block stores snapshots, not pointers whose targets change
when somebody edits a Saved row. Continue preserving unknown native fields;
unknown/future schemas block writes and credential cleanup. Credentials, access
header values, cookies/tickets, temporary ports and runtime epochs are absent
from this JSON.

Proposed ownership, extending current modules:

| Files | Responsibility |
| --- | --- |
| `connections/legacy.rs` and siblings | Types, normalization, pure selection, dependency revisions, baseline provenance matcher and explicit import plan. |
| `connections/core/legacy.rs`, `core/registry.rs` | Existing OS transaction, stale-form guards, secret staging/rollback, route resolution and selective retirement. All writers include Legacy references in credential reachability. |
| `connections/leases.rs`, `worker.rs` | Typed Saved/Legacy lease owners; cancellable resolve/commit through the existing actor. No second foreground-selection pipeline. |
| `source.rs` | Explicit logical route ownership, actual transport stamp, profile policy and guards; extend existing `Binding` rather than inventing SourceId inference. |
| `gateway/route.rs` plus existing Gateway consumers | Typed wire profile/auth policy for RPC, REST, streams/ranges, terminal, media/audio, roster and SDK operations. |
| `app/legacy.rs`, `app/routing.rs`, existing Settings view | Scoped forms and save/apply actions; existing staged resolve/commit and owner/input fence; original Hermes locations and OpenCode components. |
| `client/windows.rs`, startup coordinator | Credential-free logical Legacy descriptors for cold restore/defaults; fresh resolution rather than the first Connection captured in a process. |

The useful common request is conceptually:

```rust
struct ResolveRoute {
    door: RouteDoor,             // Legacy | explicit Saved(id, revision)
    desktop_profile: DesktopProfileId,
    expected: Option<ResolutionVersion>,
    intent: u64,
}
struct ResolvedRoute {
    binding: Binding,
    profile_policy: ProfilePolicy,
    provenance: Option<MatchedSaved>, // display/descriptor evidence only
    authorization: RouteAuthorization, // Legacy != RegistryScoped
    resolution: ResolutionGuard,
}
```

`Resolver::resolve_route` returns a cancellable ticket. `commit_route` rechecks
its captured dependencies/cancellation before probe and before publication or
metadata writes. Preserve the outgoing workspace until the application accepts
that correlated result. Ordinary Saved plugin APIs retain their behavior.
Global save, per-profile save, test and explicit apply are separate operations;
observing a changed preference must not navigate an unrelated workspace.

`Binding` needs two explicit identities: its route owner and actual transport
owner. Existing Saved/Direct bindings have the same identity in both positions.
A resolver-created Legacy binding has a stable Legacy route/version owner and
an actual Saved or Direct transport stamp. This is necessary when a Legacy
alias and a registry-scoped request use the same transport/profile spelling but
mean different backend namespaces. Never manufacture `Connection::Saved` merely
because the descriptor matcher found an ID. Conversely, a real reused Saved
lease remains the authority for that transport's lifetime, without changing a
Legacy request into registry-scoped authorization.

Construct this binding only from a successful typed resolution receipt; no
public unchecked `SourceStamp` setter. Validate its route guard separately from
the actual lease stamp. The runtime principal includes the actual incarnation
and namespace policy. Stable route/version storage excludes an SSH forward's
port. Session/editor/row identities use the route owner; network IO uses the
actual transport. Keep the original `Resolver` actor in the binding and every
queued/sent `PageRequest`, background request and long-lived SDK/stream handle
through its final receipt. Holding only a strong `Lease` is insufficient. Keep
that sender outside the actor-owned lease table/primary lease to avoid a
self-retaining actor cycle.

`ProfilePolicy` should expose typed operations such as `rpc_scope`,
`rest_self_query`, `session_aggregate_scope`, `ssh_spawn_profile` and
`validate_response_owner`, not a recursive JSON rewrite. Preserve `all`, other
explicit profiles, pagination and mutation UUID/CAS parameters. Inspect backend
ownership before projecting a desktop alias. An unscoped older response may be
backfilled only in the baseline-declared compatibility path; never overwrite an
explicit foreign owner.

Resolution guards capture only dependencies which can change that choice:
profile entry/presence, the selected global entry or environment snapshot,
primary ID/revision only on registry fallback, auth generation and profile
policy. Thus changing unrelated profile B does not retire A; adding an override
for A invalidates its previously inherited route. Capture environment values
once through an injected process snapshot, never mutable global profile state.

## Complete vertical slices

1. **Durable Legacy token/URL routing and profile aliases.** Ship global and
   per-profile forms, explicit clear/inherit, locked storage/secret rollback,
   selection, full access-header envelope, request-family profile policy,
   source-owned bindings, save/test/apply, cold restore and visible repair.
   Exercise chat/history/stop, ordinary pages, range/media, terminal/SDK and
   multi-window requests through the same resolved binding. A selected
   unsupported auth/transport remains explicit while later slices are pending;
   it must never be reinterpreted as anonymous/token/local access.
2. **Installed-Hermes SSH.** Preserve host/user/port/key path, effective
   `ssh -G` fingerprint, `remoteHermesPath` and remote profile. Resolve the
   installed external launcher/platform/home, reuse only with the baseline
   ownership proof, reconcile the actually served token and maintain exact
   bootstrap generation/nonce/lease cleanup. Existing fixed-port forwarding
   stays a distinct supported route, not a substitute for this feature. Normal
   close leaves canonical attached gateways alive; only verified app-owned
   isolated backends are stopped. Local unscopable legacy operations similarly
   require an actual external scoped backend/capability, never a request against
   the host backend's ambient launch home. No bundled/imported agent core.
3. **OAuth/Cloud.** Preserve remote versus Cloud kind and org; implement native
   PKCE/browser or isolated cookie flow according to advertised capabilities,
   refresh rotation and one-use WS tickets. Include access headers on discovery,
   ticket, REST/WS and stream consumers within the exact base origin/path.
   Distinguish reauthentication from timeout/DNS/refused/5xx. Preserve Cloud
   stale-team fallback only for the exact `org_access_denied`/`org_not_found`
   codes, including the team picker. Passive fleet observation cannot unlock a
   keychain or launch sign-in. Tokens/header values live in the credential
   service, not window descriptors or logs.
4. **Explicit migration and release acceptance.** Selected-file/bytes preview
   imports both v1 global/profile config and v2 registry with an explicit old-ID
   to UUID mapping. Decode supported `plain`/safeStorage envelopes through the
   actual bridge or request re-entry; opaque ciphertext is never a bearer.
   Keep the source file unchanged and do not copy arbitrary foreign fields
   into credential-free JSON. Cover missing native defaults versus explicit
   Legacy defaults, pinned/inherited peers, launch primary/last-used, invalid
   files, expired credentials and repair. Retain the separate baseline
   `registryPrimaryBootRoute` decision; do not let the Legacy fallback override
   an already selected explicit Saved default/boot route.

Every slice includes private fixtures and actual native interaction evidence;
all four and their platform consumers are required before calling this
legacy/global/per-profile gap closed.

## Required behavioral contracts

- Table-driven precedence, eligibility and alias-default cases above, including
  explicit global remote versus Make Primary, and Local-under-global-remote
  with/without a profile override. Failed selected routes issue zero fallback
  gateway, local-ensure or provider requests.
- Two real OS-lock writers changing different global/profile/Saved fields;
  stale form rejection before secret/probe side effects; unknown-field
  preservation; invalid/future bytes unchanged; failed exact intents retry
  after repair. Unrelated config edits keep an active lease valid.
- Two strict loopback gateways sharing profile/session names but differing
  credentials, headers, org and histories. Probe pre-dial provenance with
  unique/duplicate/absent matches and explicit qualification; a matched legacy
  ID remains Legacy in window, terminal, preview, version and Settings actions.
- Strict wire tests for alias `mara` to backend `dixie` and `default` across
  profiles/RPC, canonical Bot Chat, config writes, sidebar `recents_profile`,
  cron filters, aggregate sessions and session mutations. Foreign rows never
  become owned merely by relabeling; full pagination/CAS survives translation.
- Delayed resolve/probe/commit plus newer input or explicit same-owner action;
  edited/deleted route while queued; opener close with pending request retaining
  the original actor; last real owner disposal releases it; no current-source
  substitution or stale metadata promotion.
- SSH fixtures prove argv/profile/path/fingerprint, bootstrap coalescing,
  served-token adoption and exact ownership cleanup. Private fake OAuth/Cloud
  endpoints and private credential services prove refresh/ticket/header/recovery
  without a real provider, user keychain, live agent or physical audio.
- Native cold launch with an explicit Legacy default and no previously captured
  Connection; unusable route/config/default shows actionable repair and no
  fallback traffic. Repair clears only its owned error. Both themes, pinned and
  inherited windows, active streams and retained drafts remain isolated.
