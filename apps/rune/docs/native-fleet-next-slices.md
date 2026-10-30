# Native fleet, legacy routes and project scope: next slices

Read-only implementation map against the retained Electron baseline named by
`desktop-baseline.json` (`ef75bc708867e3884cb40a91363be6dc6fddc034`). Paths below
refer to retained client behavior; no upstream agent implementation is required.
This audit did not run native or network acceptance. Current startup/window work
is covered separately by `native-startup-route-parity.md` and
`native-startup-route-acceptance.md`; it does not establish fleet/project parity.

The native client now has device defaults, primary/last-used source policy,
confirmed per-source profile memory, captured window pins and delayed-navigation
ownership fences. Reuse those rather than building another activation path.
`view/shell.rs::profile_rail` still projects only `Viewer.profiles` from the
current gateway. Its dropdown has the same limitation. `SidebarState` has
profile/project filters, grouping and actual session status, but no fleet roster
or entered-project owner. A filter on session cwd is not project scope.

## 1. Owner-qualified roster and sidebar data (P0 foundation)

Baseline authorities:

- `apps/desktop/electron/connection-registry.ts`: `buildAgentRoster`,
  `rememberSshEnumeration`, `shouldRetrySshInventory`, `resolveRegistryLocalRoute`.
- `apps/desktop/electron/main.ts`: `enumerateRegistryAgentSources` and
  `hermes:agents:roster`; `electron/connection-caches.ts`,
  `registry-backend-identity.ts` and `roster-source-status.ts`.
- `apps/desktop/src/app/chat/sidebar/fleet-rail.ts`, `gateway-group-model.ts`,
  `gateway-groups.tsx`, `gateway-group-preferences.ts`.

Add a client-owned union of source status plus source-qualified profile rows.
Keep logical saved source identity, current revision/authenticated principal and
backend profile identity distinct. Two gateways may expose the same profile and
session id. Current `gateway/sidebar.rs::SidebarSession::key`, sidebar live keys,
profile grouping and row actions omit the source; cross-source rows are currently
refused by `sidebar.rs::sidebar_action` and `prepare`. Resolve their owner
and retain its lease before reads or mutations instead of removing those guards.
Include empty profile groups only for an unnarrowed, successfully loaded view;
keep messaging group alias/collapse/order keys separate from recents.

Roster enumeration is not foreground activation. Keep previous successful names
when a source is temporarily unreachable; expose the reason beside that source.
Invalidate caches when id/revision names another machine or is removed. Collapse
registrations only when their known `install_id` proves one backend; an absent
identity is not evidence of equality. Preserve disambiguated mention handles.
A never-seen SSH connect-on-demand seed is distinct from a proved agent roster;
an unreachable, never-seen URL must not invent agents. Undialed SSH may use the
baseline's bounded read-only inventory, but listing a sidebar must not create
forwards, dashboards or agent owners. A deferred Local
source must likewise remain attach-on-demand. Per-source deadlines must prevent
one sleeping machine from holding the entire roster.

Reuse `connections::plugin::routes` for credential-free metadata, `Resolver` /
`Lease`, scoped `PageRequest::lease`, gateway profile/status reads and existing
sidebar parsing/lineage/mutation receipts. `plugin_host/sources.rs` already has
verified source bindings and partial roster infrastructure; its `roster()` skips
SSH and ensures other sources, so it is useful infrastructure, not proof that
native fleet enumeration or the complete baseline cache policy exists. Keep
roster cache ownership independent of the foreground draft; `PageNetwork`'s
current-scope reply filter is appropriate for foreground actions, not an entire
cross-source roster that must survive foreground navigation.

## 2. Fleet rail, grouped sessions and attention (P1, after owner data)

Baseline authorities:
`apps/desktop/src/app/chat/sidebar/profile-switcher.tsx` (`ProfileRail`,
`RestSquare`, `ProfileSquare`), `profile-dropdown-switcher.tsx`,
`fleet-gateway-menu-group.tsx`, `profile-launch-menu.tsx`,
`src/lib/connection-display.ts`, `profile-order.ts`, `profile-color.ts`,
`src/store/fleet-roster.ts` and `profile-dot-state.ts`.

Preserve the following behavior with Rune/OpenCode components in the existing
Hermes sidebar/footer locations:

- Active and at-rest gateway groups keep stable positions across selection:
  Local first, then the baseline label/id ordering. Home names that gateway's
  default; This device is a separate owner. Show route-local pending/error
  feedback while the previous conversation remains usable.
- At most 13 total fleet squares use the strip; above that, the grouped condensed
  menu retains every route, launch/default action, New, Import and Manage.
  Preserve wheel/keyboard overflow access and the hidden-rail replacement picker.
- Named square drag reorder must not also select or recolor. Hold/context color
  choice supports Auto and swatches. Baseline rail order/colors are client
  preferences keyed by profile name, shared by active/rest groups; activity and
  actions are keyed by source/profile. Do not silently change those scopes.
  Gateway/profile session-group alias/order/collapse preferences use exact owner
  keys and retain filtered/offline entries during reorder.
- Current and at-rest context actions carry the clicked source/profile, without
  selecting it merely to open a menu. Preserve the baseline's action differences
  (including no Export on at-rest squares), source-qualified rename/SOUL/delete,
  Open in New Window and Set Default. Existing current-source native actions are
  only part of this surface.
- Attention is needs-input > working > unread, with accessible counts. Fold
  loaded rows, proven-owner unlisted live runtimes/hidden Bot Chats and persisted
  unread markers; exclude cron-only completion. Ambiguous legacy unread markers
  must not mark both same-named profiles. The active square suppresses its own
  indicator as in the baseline. Recent timestamps do not prove working.

Refresh on mount, focus/visibility, registry change and gateway-open edge. The
baseline `use-fleet-roster.ts` / `store/fleet-roster.ts` coalesces calls, uses a
60-second freshness window and a 5-second retry window for partial failures;
forced refresh during an in-flight read queues one follow-up. There is no sidebar
fleet polling timer. Separately refresh the current profile list on active-window
edges (`use-profile-rail-refresh-on-active.ts`). Native `app::on_focus` currently
only updates notifications/key capture. Browser-only served-profile capability
branches in `unserved-profiles.tsx` are not automatically native requirements.

Reuse `SidebarState::status`, conversation/chat owner state, identity image
loading and locked client preference patch storage. Bot metadata color in
`identities.rs` is agent-owned appearance; it does not implement the separate
client rail-color preference. Preserve current-source and fleet projections from
one source-qualified model so the rail, picker, Bots and Capabilities do not
invent inconsistent rosters.

## 3. Legacy null-source and remote-override recovery (P0 route correctness)

Baseline authorities:
`apps/desktop/src/store/profile.ts::profilePickConnectionId`,
`ensureGatewayProfile(forceLegacyRoute)`, `store/profile-remote-override.ts`,
`app/chat/sidebar/profile-remote-override-dialog.tsx`,
`apps/desktop/electron/connection-config.ts::profileRemoteOverride`,
`resolveProfileBackendRoute`, `translateSelfProfileQuery`,
`resolveProfileApiRequest`, `electron/desktop-remote-route.ts::resolveDesktopRemoteRoute`
and `connection-route-identity.ts`, plus `electron/main.ts`'s legacy resolver.

Native `routing.rs::queue_default_route` currently resolves `connectionId:null`
through the primary window's remembered in-process connection. This preserves a
captured legacy source within a run, but does not recover the durable legacy
client configuration, global remote selection or per-profile SSH/URL overrides.
There is no native override edit/remove/token-recovery surface or alias-to-backend
profile mapping. `client::profiles::ProfilePreferences` is presentation storage,
not this connection policy.

Implement one explicit resolver for the legacy door, distinct from registered
Local and saved source UUID routes. Preserve the baseline precedence (explicit
per-profile route, existing `HERMES_DESKTOP_REMOTE_URL/_TOKEN` compatibility,
global remote configuration, then existing Local attach); absence may reach
another rung, authentication/connectivity errors
may not silently dial Local. Named picks on This device retain the legacy door;
its default profile and explicitly captured fleet actions retain their exact
source. Restore the appropriate client-owned legacy configuration rather than
reading/writing agent configuration or provisioning an embedded backend.
The retained legacy file is the client's `userData/connection.json`
(`electron/main.ts::DESKTOP_CONNECTION_CONFIG_PATH`). Preserve its bytes during
import; Electron-encrypted secrets that cannot be recovered require explicit
credential recovery, not reinterpretation as an absent route.

Keep Desktop alias and actual backend profile separate across RPC, REST queries,
assets, history, cwd, streams and mutation receipts. Token rotation/removal must
invalidate the correct cached principal. Preserve remote badge/readback,
first-connect confirmation, registry-name collision warning, failed-save recovery
and keyring capability/explicit insecure-storage opt-in behavior from the
baseline. Only confirmed auth failures offer re-enter-token recovery; a timeout
is not a revoked credential. Cloud/OAuth remains its own uncompleted route,
not a token-shaped replacement. Reuse the native keyring/transaction interfaces,
leases, source epochs and route-owner fence; retain unknown/future preference
fields and previous usable workspace on failure.

## 4. Entered projects and New Session ownership (P1, after route semantics)

Baseline authorities:
`apps/desktop/src/store/project-scope.ts`, `projects.ts::resolveNewSessionCwd`,
`followEnteredProjectCwd`, `projectProfile`,
`app/session/new-session-route.ts::defaultNewSessionTarget`,
`app/session/hooks/use-session-actions/index.ts::startFreshSessionDraft`,
`createBackendSessionForSend`, `openNewSessionTile`,
`app/chat/reuse-draft-tab.ts`, and `app/chat/sidebar/projects/*`.

Add an entered project scope owned by source/profile, separate from sidebar
filter preferences and the backend active-project pointer. A concrete project
suppresses generic window/device defaults before either is resolved. All/Home
remain eligible for the generic default; Home independently means detached cwd.
An explicit route/profile/project/worktree/cwd wins over generic defaults.
Configured default cwd is inherited context, not an explicit project action.
Source/profile changes must leave a foreign project scope synchronously.

Preserve cwd precedence: explicit string path; explicit null/Home detached;
entered project's primary/default-branch checkout; otherwise that intended
profile's configured default. A generic new chat must not inherit an unrelated
stored conversation or linked worktree cwd. Entering a project updates an
untouched draft's workspace only; an existing conversation keeps its own cwd.
Capture owner, project and cwd at create/send intent and fence late reads and
mutations. Blank draft-tab reuse requires exact intended owner, actual empty
text/attachments/queue and known idle state; unknown/restoring is not empty.

Reuse `PageRequest::rpc`, `PageNetwork` owner fencing, `Composer` cwd /
`cwd_explicit`, actual editor slots and `app/tools.rs::workspace_cwd`.
The pinned `apps/shared/src/gateway-contract.generated.ts` already defines
`projects.list/tree/project_sessions/set_active` and the remaining CRUD methods;
no new agent RPC is needed. Native Rune currently has no projects controller or
`projects.*` consumer. Implement the project tree, Home/overview/entered lanes,
paging and real backend mutations in subsequent complete slices; a cwd filter
or new-session-only facade cannot close the project feature ledger.

## Acceptance contracts for these slices

Use private client stores, strict loopback gateways and controlled SSH adapters;
actual native input/window acceptance in both themes follows controller tests.
No live provider or user-agent mutation is needed to prove the client contracts.

1. Two sources expose identical profile/session names with different history and
   principal. Grouping, pin/read/archive/rename/delete, profile attention and
   window actions remain independent through delayed reads, source replacement,
   reconnect and two open windows. No outgoing roster paints under an incoming
   source whose profile read failed.
2. Mount/focus/visibility/registry/open events produce the expected coalesced
   reads, forced follow-up and bounded partial retry; elapsed time alone does
   not poll the fleet. Offline sources retain known rows/recovery, removed or
   revised sources lose old cache entries, same known install id deduplicates,
   unknown ids do not. Enumeration creates no forward/dashboard/agent owner.
3. Real rail at 13 and 14 total profiles retains every route/action in strip and
   condensed forms, stable group position, wheel/keyboard access, hidden-rail
   recovery, current draft and explicit menu owner. Drag, tap and color hold are
   distinct. Independent preference writes/restart preserve order/color/aliases
   and unknown fields without agent configuration mutations.
4. Proven live working/needs-input, hidden Bot Chat and restart-persisted unread
   exercise count priority/accessibility. Cron and ambiguous legacy markers do
   not create false attention on same-named profiles at another source.
5. Legacy null, explicit Local and saved UUID select distinct fixtures. Test a
   per-profile override, global remote, alias mapping, missing credentials,
   401/403 re-entry and ordinary connectivity failure across REST/RPC/stream /
   asset/cwd paths. Failed writes and failed remote activation preserve both
   previous source and typed draft; no implicit Local substitution occurs.
6. Project A on source X competes with device default Y, a profile window and a
   stale draft route. Generic concrete-project actions stay on A/X; All/Home,
   configured defaults, explicit string/null cwd, project/worktree "+" and tiles
   follow their declared precedence. Switching source/profile retires foreign
   scope; entering a project leaves an existing chat's cwd unchanged. Delayed
   project/default replies cannot steal newer typed input or another owner.

These contracts extend the parity ledger; none is marked accepted by this map.
Existing detailed inventories remain `lower-left-parity.md`, `shell-parity.md`
and `functional-parity.md`. Packaging, platform acceptance, topology restoration
and other outstanding feature areas remain required independently.
