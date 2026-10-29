# Lower-left rail, menus and statusbar: feature inventory

Audited 2026-10-29 against the retained Hermes Desktop source and native Rune
commit `57c5b46afd`. The user supplied a closeup and explicitly requested all
of its features, switches and settings, **not its visual density**. This is a
transfer checklist, not a completion claim. Keep the private reference image
in the conversation; do not copy its gateway name or workspace into fixtures.

Paths below are relative to `apps/desktop/src/` unless prefixed otherwise.
`Pending` means no native implementation. `Partial` names the working subset
and never means an unavailable affordance is a transferred feature.

## What the visible glyphs actually represent

The upper icon strip is `app/chat/sidebar/profile-switcher.tsx::ProfileRail`,
which becomes a fleet rail when several distinct gateways are registered.
It is not one generic overflow menu. The statusbar below it is a separate,
customizable surface (`app/shell/hooks/use-statusbar-items.tsx`).

| Reference affordance | Actual behavior | Native coverage |
| --- | --- | --- |
| Layers at the left | All profiles **on the active gateway**. In a single-gateway setup the pinned home/layers pill toggles default versus All; from a named profile it returns to default, not to All. The All view is not a connection switch. A gateway that does not serve default must not be offered an invalid default pick. | Pending All/default semantics. Native profile selection alone is partial. |
| Small monitor / network / terminal / cloud glyph and amber dot | `FleetDivider` names the gateway's kind. Amber means its last roster fetch found it unreachable; the tooltip can report sign-in/error state. This marker is not itself a switch or a local readiness claim. `connection-glyph.tsx` maps local, remote, SSH and cloud consistently. | Pending gateway grouping, kinds and roster reachability. |
| Separate monitor pill | This device's default profile when that gateway is at rest; selecting it changes the owning gateway. It is deliberately distinct from the active gateway's home pill. | Pending registry/fleet selection. Local-home attach in the connection form is only a subset. |
| Home pill | The default profile of the gateway whose group contains it; selected state follows the exact gateway/profile pair. House does not mean the VM running the client. | Pending home/fleet semantics. |
| Colored profile squares | Select that profile; active identity, background activity/attention and optional per-profile remote-override badge. Remote group squares retain the owning gateway in accessible names and actions. | Partial: native profile list and picker select a loaded profile. No fleet, reorder, recolor or status rollups. |
| Plus | Create an agent profile, with clone source and optional initial SOUL. | Pending real creation. |
| Cloud-download | Import a portable profile bundle via explicit file pick. It is **not** an app-update button. | Pending real import and transfer. |
| Ellipsis | Open Profiles management, including persona editing. It does not directly contain every application setting. | Pending Profiles view. |
| Additional plug when there is only one connection | Discover/connect a gateway through the unified settings page; once several exist the connection selector provides this door. | Partial connection form; no saved registry/settings page. |
| Command symbol in the statusbar | Open the Command Center (sessions, system, usage, maintenance), not simply the command palette. | Pending. |
| Gateway label and chevron | Saved connection selector, with kind, search at scale, selected/pending state and Connect/Manage entry. | Partial: current endpoint and change-connection form; no saved-connection picker. |
| Backend health label | Open the gateway health/recovery panel. It combines transport, inference readiness, messaging health and restart state; connected WebSocket alone does not mean Backend ready. | Partial: native connection state and reconnect only. |
| Workspace folder label | Focused conversation's project/cwd and its actions. The folder may live on another host. | Pending owner-aware cwd and menu. |

## Profile and fleet controls, including hidden interactions

Source owners: `app/chat/sidebar/profile-switcher.tsx`,
`profile-dropdown-switcher.tsx`, `fleet-rail.ts`, `use-fleet-roster.ts`,
`fleet-gateway-menu-group.tsx`, `profile-launch-menu.tsx` and `unserved-profiles.tsx`.

- [ ] Preserve active and at-rest gateway groups in registry order; switching
  must not move the target under the pointer. Deduplicate registrations of the
  same backend rather than displaying two fleets for one owner.
- [ ] Preserve route-specific pending spinner, unreachable/signed-out reason,
  last-known roster and visible recovery. Never substitute a same-named local
  profile for a failed remote pick.
- [ ] Preserve served versus unserved profiles: a listed but unserved profile
  is visible with its explanation and disabled for selection.
- [ ] Preserve active identity plus background rollup: needs-input, working and
  unread counts with accessible text. Do not derive Working from recent activity
  timestamps or use color alone.
- [ ] Preserve horizontal scrolling, overflow discovery and keyboard access.
  Beyond 13 total profile squares (including the fleet), the baseline uses a
  condensed menu. The condensed menu retains profile/gateway choices, New,
  Import and launch actions; Manage remains reachable. Do not simply truncate
  the selectable profiles at the native rail's current first 12 squares.
- [ ] Preserve local square drag reorder without accidental selection or
  recoloring; preserve long-press and context-menu recoloring, explicit color
  swatches and Auto color. At-rest gateway groups are not drag-sorted across
  machines. Client color/order preferences are not agent config mutations.
- [ ] Preserve right-click **Open in new window** when the native host supports
  it, carrying `(connectionId, profile)` into the new window.
- [ ] Preserve **Set as default startup profile** as a client-owned launch route,
  distinct from the backend profile whose canonical id is `default`; show the
  current default and gate missing host capability.
- [ ] Preserve **Color**, **Rename**, **Edit SOUL.md**, **Export**, **Delete** on
  local named squares. At-rest squares offer Switch to that gateway/profile,
  Color, Rename, SOUL and Delete; the retained at-rest menu does not offer export.
- [ ] Preserve the legacy per-profile **Connect to remote host** entry only on
  its applicable single-gateway path, with remote badge/readback. Fleet routing
  replaces that legacy door when multiple registered gateways exist.
- [ ] Preserve the rail show/hide preference and its replacement profile picker
  beside the statusbar gateway selector. Hiding a rail must not remove the last
  usable door to profile selection. Simple mode must shadow, not overwrite, the
  Advanced preference.

The last visibility switch is reachable through the Sessions filter menu,
shell context menu and registered `view.toggleProfileRail` action. See
`app/chat/sidebar/filter-menu.tsx`, `app/context-menu/shell-menu-items.tsx`,
`app/contrib/controller.tsx`, `store/profile-rail-prefs.ts` and `store/interface-mode.ts`.

## Creation, import, management and destructive actions

These are agent-owned operations, not local mock rows. The native reader must
not enable them until their real routed mutations and recovery exist.

| Feature | Required contract / source | Native status |
| --- | --- | --- |
| Create | `app/profiles/create-profile-dialog.tsx`: validated profile name, clone from an existing profile or no clone, optional SOUL.md, visible busy/success/error, refresh and land on a fresh chat in the created profile. Older backend persona fallback is explicit. | Pending |
| Import | `store/profile-share.ts`: explicit `.tar.gz`/`.tgz` pick; backend import as a new profile; refresh and select it. The bundle includes config, skills, SOUL and cron with credentials excluded by the backend contract; `desktop.json` carries skin/appearance mode, user theme definitions, profile color and layout tree. Validate each imported desktop field. | Pending; native file staging is not profile import. |
| Export | Same owner: save-path choice, backend archive plus desktop overlay, cancellation, progress/error and final archive result. Retained flow passes a path, not file bytes; a remote backend cannot be given an unrelated client-local path. Native remote import/export requires an explicit transfer seam. | Pending |
| Profiles management | `app/profiles/index.tsx`: searchable list, New, selected profile detail, display name/default badge, path, `.env` presence, model/provider and skill count, rename/delete actions, SOUL editor with missing/loading/error/dirty/save behavior. It is a separate management surface, not a second bot-session browser. | Pending |
| Rename | `rename-profile-dialog.tsx`: ordinary profile rename migrates client tiles/preferences and fences old requests/dials; default profile gets a Unicode display name while canonical id remains `default`. Fleet rename acts on the square's explicit owning route, without local teardown. | Pending |
| SOUL editing | `getProfileSoul` / `updateProfileSoul`, scoped to the owning connection/profile; stale reads may not fill another editor, errors remain visible, save reflects actual backend success. | Pending |
| Delete | `delete-profile-dialog.tsx`: explicit destructive confirmation naming the profile/owner/path; backend refuses default deletion; fence old bindings and drop persisted tiles so they cannot recreate the removed profile; deleting the active local selection returns to default after reconciliation. Fleet deletion never retires an unrelated local backend. | Pending |
| Legacy remote override | `profile-remote-override-dialog.tsx`: read/save/remove override, URL/token validation, encrypted-token capability, explicit opt-in if storage lacks encryption, first-connect confirmation, registry-name collision warning, refresh and error states. Tokens remain in the host bridge, never ordinary profile data. | Pending |

Wire owners are `api/profiles.ts`: profile list/create, scoped rename/delete,
SOUL read/write, setup-command, export/import. Host-owned launch defaults live
in `store/default-profile.ts` and `store/windows.ts`. A default launch route
is not a `config.set` against an agent.

## Gateway selector and recovery panel

- [ ] Saved local, URL/token, SSH and cloud registrations and their labels/kinds.
  `ConnectionSwitcher` sorts consistently; large lists search label/address/kind,
  support keyboard selection and own Cmd/Ctrl+F while open, with empty-result
  and pending states. Connect/Manage remains discoverable.
- [ ] Picking a saved gateway performs one soft re-home: preserve the shell,
  invalidate owner-bound cached state, then reconnect to the exact gateway and
  requested profile. Failure leaves explicit recovery; it never starts a local
  agent as fallback.
- [ ] Retained **This device** confirmation distinguishes installed runtime
  versus missing runtime, and replacing the center versus named-profile pick
  (`local-device-switch.tsx`). In this standalone project, attach to an existing
  installation/home or expose explicit setup guidance; do not silently restore
  an in-repository installer or agent provisioning side effect.
- [ ] Health panel shows transport and inference separately; reason text,
  real messaging-platform health and recent activity/log tail while open.
- [ ] Preserve **Reconnect**, **Open System / View all logs**, and separately
  presented **Restart gateway**. Reconnect does not restart the owner. Restart
  is a disruptive, explicit agent lifecycle action with owner routing,
  confirmation/progress/recovery from `store/system-actions.ts`.

Sources: `app/chat/sidebar/connection-switcher.tsx`, `connection-glyph.tsx`,
`local-device-switch.tsx`, `store/connections.ts`, `app/shell/gateway-menu-panel.tsx`,
`lib/gateway-health-pill.ts` and `app/shell/hooks/use-statusbar-items.tsx`.

## Workspace menu

All pending natively. `use-statusbar-items.tsx` and
`app/shell/hooks/use-change-workspace.ts` require the focused conversation's
owner and stored/runtime identities, not merely the foreground profile.

- [ ] Display named project when known, otherwise cwd leaf; tooltip/copy uses
  the shared display formatter. Cold tiles and tab switches must not briefly
  paint the previous chat's live cwd.
- [ ] **Change working directory** through a remote-aware directory chooser and
  `session.cwd.set`; disabled while busy. The retained menu hides this action
  for cold tiles without a runtime and for tiles on another gateway when the
  chooser would browse the wrong host. Do not route those to the client VM.
- [ ] **Copy full path** to the client clipboard.
- [ ] **Reveal in OS file manager** only for an actual local workspace.
- [ ] **Reveal in sidebar file tree**, bound to that conversation's owner.

## Command Center behind the command-symbol button

Source `app/command-center/index.tsx` has four sections. All remain pending in
Rune; merely replacing this button with Cmd+K would lose these features.

| Section | Features to preserve | Authority / gates |
| --- | --- | --- |
| Sessions | Search, open, pin/unpin, export, delete confirmation, metadata, paged load-more and loading/error/empty states. | Session owner and durable pin truth; deletion/export are real operations. |
| System | Real gateway/messaging state, backend version/active-session count; Restart gateway and Update Hermes with action progress; log files `agent/errors/gateway/desktop`, level `ALL/INFO/WARNING/ERROR`, text search and live tail. | Selected backend; update/restart are explicit actions against the external runtime, never automatic bundled maintenance. |
| Usage | 7/30/90-day selection, refresh/retry, totals for sessions/calls/input-output tokens, daily token graph, per-model table and top-skill usage. | Backend analytics; unavailable data is not synthetic zeros. |
| Maintenance | Doctor, Security audit, Backup, Debug share with returned links/copy; running action log; curator status, last run, pause/resume and Run now; memory-layer summary and separate MEMORY/USER reset confirmations. | Existing scoped backend APIs; debug share creates an external disclosure and memory resets are destructive user actions. Preserve their explicit invocation/confirmation, cancellation and failures. |

Maintenance source is `app/command-center/maintenance.tsx`; its API owners are
`api/system.ts` and the retained analytics/session APIs. No implementation of
these agent operations belongs in Rune itself.

## Statusbar visibility and optional switches

The statusbar's own right-click menu (`app/shell/statusbar-controls.tsx`) is a
preferences surface: all registered items with a `toggleLabel`, including plugin
contributions, appear once by id in left-to-right order. It stays open while
several checkboxes change, offers **Reset statusbar layout**, and **Hide statusbar**.
Core entry points/version notices marked `lockedVisible` cannot be hidden.
All of this is pending in Rune.

The core inventory, including items absent from this screenshot:

- [ ] Command Center (locked), gateway selector (locked), replacement profile
  selector (locked), backend health and conditional free-tier/sign-in chip.
- [ ] Workspace, subagent/activity tree with running/failed counts, Cron and
  Webhooks route shortcuts.
- [ ] Running-turn timer and focused-session timer; context meter with real
  breakdown/compressions; prompt-cache hit rate and tokens/sec; applicable plan
  allowance gauge linking Billing; supported system-resources panel (GPU,
  VRAM, RAM and unified-memory information).
- [ ] Real profile-scoped approval-mode selector (Manual/Smart/Off), distinct
  from the native composer's staged draft approval choice.
- [ ] Show/hide/focus actual Terminal; client version/update and, for remote
  connections, backend version/update. Version notices are not hideable.
- [ ] Preserve backend/host capability gates. Unsupported resources have no
  phantom switch; opted-in meters with no data retain an honest placeholder.
  Focused-session metrics must not follow an unrelated primary or hidden tile.
- [ ] Persist per-item visibility and whole-bar visibility separately. Reset
  restores the shipped item layout, not whole-bar visibility. Simple mode
  shadows Advanced preferences. Hidden statusbar has an accessible way back
  through `view.toggleStatusbar` (default Cmd/Ctrl+Shift+S) and command palette.
- [ ] Bare shell context menu preserves New chat, host-supported New window,
  command palette, Show/hide statusbar, Show/hide profile rail, Show/hide target
  pane tab strip, Settings and explicit Update action.

Source owners: `store/statusbar-prefs.ts`, `store/profile-rail-prefs.ts`,
`app/shell/hooks/use-statusbar-items.tsx`, `system-resources-statusbar.tsx`,
`approval-mode-menu.tsx`, `app/context-menu/shell-menu-items.tsx` and registered
actions in `app/contrib/controller.tsx` / `lib/keybinds/actions.ts`.

## Source sections directly above the rail

Telegram and API are instances of actual messaging sources, not permanent
hardcoded buttons. Preserve per-source collapse, counts/load-more, profile and
gateway ownership, pin/unread/session actions and empty/error handling. API's
wire source is `api_server`. Cron Jobs is a real jobs/run tree with open-run,
manage and trigger actions, Advanced-mode visibility and independent limits.
These remain pending; their detailed source/API contracts are already in
`screenshot-parity.md`. A recreated row header does not finish these sections.

## Presentation direction and acceptance

Keep the entry points at the sidebar foot and bottom statusbar, preserving the
current Hermes ownership cues. Use comfortable native hit targets and named
groups rather than copying tiny icon density. The rail can expose current
gateway/profile plus a clear picker; Create, Import and Manage remain reachable
without requiring an undiscoverable long press. The picker/context menus retain
the full action set, and the existing condensed path establishes that this is a
valid presentation adaptation. Recovery and identity should remain visible.

Mark an item transferred only after its routed action works with actual
loading/error/recovery, keyboard/Escape and narrow-window behavior. Test same-name
profiles on different gateways, unserved and signed-out sources, late replies,
All versus default, hidden-rail/hidden-statusbar recovery and state persistence.
Use synthetic fixtures for screenshots; a native semantic tree is not OS
screen-reader acceptance. Native macOS/Windows host capabilities need their own
acceptance. This inventory preserves scope; it does not authorize background
agent mutations, automatic install/update or removal of the retained baseline.
