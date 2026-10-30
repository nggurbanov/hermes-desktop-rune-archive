# Device default and startup routing: implementation map

Status: required, not implemented. The retained Electron baseline separates
three decisions that the native client must preserve:

- Generic New Session consults the device default logical source/profile route.
- An explicitly profile-pinned window retains its own initial source/profile
  default. Ordinary peers inherit their opener only for bootstrap.
- Fresh application launch has a separate primary/last-used source policy.

The current ordinary-peer New Session contract proves the no-device-default
case only. It cannot establish these missing preferences or startup behavior.
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

The current rail lists the active source only. Full fleet needs cached named
profiles grouped by source, with demand-driven refresh on mount/focus/visibility,
registry changes and connection-open edges. Coalesce stale/inflight requests;
rendering or periodic fleet polling must not dial every SSH source.

## Required evidence

Contracts must cover the three window kinds, project/explicit exclusions,
identical profile names across sources, delayed activation/newer intent,
unchanged existing conversations, primary/last-used/fallback/remembered-profile
precedence, source revision replacement/removal/missing profile, scoped
rename/delete/forget maintenance, failed and future-schema storage, restart and
cross-window observation. Native acceptance must exercise the actual context
menus/startup control, condensed fleet actions and both appearances. No provider
or production mutation is needed to prove these client routing contracts.
