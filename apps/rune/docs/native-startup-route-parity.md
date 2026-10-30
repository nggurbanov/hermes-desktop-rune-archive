# Device default and startup routing: implementation map

Status: implementation integrated; native acceptance is tracked separately in
`native-startup-route-acceptance.md`. The retained Electron baseline separates
three decisions that the native client must preserve:

- Generic New Session consults the device default logical source/profile route.
- An explicitly profile-pinned window retains its own initial source/profile
  default. Ordinary peers inherit their opener only for bootstrap.
- Fresh application launch has a separate primary/last-used source policy.

The device store, primary/last-used registry policy and native actions now exist.
They are distinct from full fleet presentation, legacy source override recovery,
project/tile scopes, topology restoration and platform acceptance; those remain
requirements rather than implied consequences of this routing implementation.
The baseline authority is `apps/desktop/src/app/session/new-session-route.ts`,
`src/app/chat/reuse-draft-tab.ts`, session actions, `store/connections.ts`,
connection-registry Settings and `electron/desktop-profile.ts`.

## Storage and ownership

Use a device-wide client store outside `client::Preferences`, because the latter
is also copied into per-window presentation. Reuse locked patch preservation,
explicit errors and pending retry. A logical default has source id plus profile;
it does not store credentials, a transient authenticated Connection, a forward
port or an immutable authentication revision. Distinguish no default, explicit
legacy profile (`connectionId: null`) and a registered Local/Remote/SSH source.
Capture and validate the current row revision when activation actually begins.

Registry startup state needs primary, last-used, launch mode (primary by default)
and confirmed last profile per source; current `selected` alone is insufficient.
Explicit device default wins at fresh primary launch. Otherwise valid last-used
wins when enabled, with primary as its defined missing-source fallback. A failed
preference read cannot be interpreted as absent preference. Explicit CLI/window
routes and newer user choices supersede late startup reads.

Updates from other windows refresh the preference mirror without navigating an
existing conversation or draft. Source-qualified successful profile rename should
retarget only its matching default; successful deletion/removal clears only its
matching route. Failed mutations and identically named profiles on other agents
must not alter it.

## Activation and user actions

Reuse Resolver/retained Lease, fenced commit, `use_connection_for_profile` and
owner-changing guards. Preserve the outgoing workspace until the requested source
and profile validate. Fence delayed replies by action intent, source id/revision
and requested profile. Activation failure is visible and cannot substitute a
source/profile. Legacy null-source routes also have per-profile remote override
semantics; mapping them all to registered Local would lose behavior.

Generic project-scoped actions suppress the generic device default. All/Home do
not. Explicit source/profile/project/cwd/existing-session/tile actions keep their
captured owner. Reusing a blank draft also requires matching its intended owner.
Changing a default does not move an already open conversation or draft.

Profile rail and condensed menus need source-qualified Open in New Window,
Set as Default and checked Default Profile actions without selecting a route
as a side effect of opening its context menu. Connections needs primary
indication/action and the startup switch, including when only one source exists.
Make Primary remains available in last-used mode. Reuse native Menu/Switch,
existing avatars and both themes.

The original coordinator audit found an active-source-only rail. The ninth
integration adds cached source-qualified groups and demand-driven refresh on
mount/focus/visibility, registry changes and successful connection-open edges;
see `native-fleet-parity.md`. Stale/inflight reads coalesce, and cache expiry or
rendering alone does not dial SSH sources. Complete hidden/unlisted/persisted
activity and legacy route consumers still require their separate acceptance.

## Required evidence

Contracts must cover the three window kinds, project/explicit exclusions,
identical profile names across sources, delayed activation/newer intent,
unchanged existing conversations, primary/last-used/fallback/remembered-profile
precedence, source revision replacement/removal/missing profile, scoped
rename/delete/forget maintenance, failed and future-schema storage, restart and
cross-window observation. Native acceptance must exercise the actual context
menus/startup control, condensed fleet actions and both appearances. No provider
or production mutation is needed to prove these client routing contracts.

## Implemented coordinator and remaining boundaries

`client::startup` stores versioned logical defaults in device-owned `startup.json`.
It preserves unknown fields, uses a real cross-process lock, refreshes mirrors
without navigation and retains failed write intent for retry. The connections
registry separately owns primary, last-used, launch mode and remembered confirmed
profiles. A successful profile-only pick, Bot Chat or stored-tab open updates that
source's remembered profile without changing the last-used source.

The application coordinator resolves the captured saved row revision, verifies
the exact target profile and retains its source lease before changing workspaces.
A cancellable correlated receipt cannot automatically promote an outgoing
foreground lease. Window/editor/scope/page/read intent and actual text/attachment
payload fence delayed navigation. Explicit current-workspace, existing-session,
Bot and page actions supersede pending generic navigation. Unsupported/unreadable
default storage is an error, not an absent preference. Explicit CLI `--profile`
keeps the recovered source while discarding its unrelated old conversation.

Profile-pinned windows consult their immutable captured route before the device
default, including when the in-memory pin must be recovered from window metadata.
A failed credential recovery is visible and preserves the outgoing workspace.
Generic New Session creates a fresh conversation rather than reopening canonical
Bot Chat from the pin's original bootstrap target.

Source-qualified successful profile rename/delete/Forget maintains only the
matching logical default and remembered profile. Context actions and startup
controls use native Rune/OpenCode components in the Hermes positions.

Source-qualified fleet grouping/status/order/color now exists; complete fleet
attention and legacy route consumers, project-scoped generic New Session and
simultaneous tile ownership remain open. Explicit legacy null-source
routing currently requires the primary legacy connection retained in this
process; complete persisted legacy/global/per-profile remote override recovery
is still required. It must not be approximated by mapping null to registered
Local or by embedding credentials in the logical preference. See the acceptance
record for exact immutable binaries and observed cases.
