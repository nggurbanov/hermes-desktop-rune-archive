# Concurrent surfaces and native peer windows

Status: next-step design, refreshed against checkpoint `f0461bc305` on 2026-10-29.
This document proposes the remaining concurrent-surface and native host work; it
does not establish its implementation or acceptance.
The standalone retained Electron tree is the behavior baseline. Rune is pinned to
`bfb8facae2e4665e5e6d17afe85561884a7fd402`.

## Baseline behavior

- `apps/desktop/src/app/chat/session-tile.tsx` mounts the real ChatView/ChatBar
  under a per-runtime `SessionView` and a tile `ComposerScope`. Submit, slash,
  steering, edit, retry, restore, branch and stop go through the session delegate
  in `session-tile-actions.ts`. A second transcript with an ambient composer is
  not a session tile.
- `session-view.tsx` reads even the main workspace from its own `$sessionStates`
  slice. The ambient globals serve a fresh draft before a runtime exists. A
  background turn must never paint into whichever conversation is selected now.
- `composer/scope.tsx` separates attachment chips, awaiting-input semantics,
  transcript history and focus routing. Its independent `ComposerSurfaceContext`
  identifies one mounted input even when the same stored session is displayed
  more than once or no session exists yet. Live draft text belongs to the mounted
  editor; it is not one globally shared contentEditable.
- `store/session-states.ts` persists session tiles with durable stored id,
  docking edge/anchor/slot, workspace mode and exact credential-free owner route.
  Runtime bindings are ephemeral. Connection/profile changes do not reinterpret
  another source's stored ids. Reconnect invalidates the affected owner bindings.
  A failed resume stays in that tile with an explicit retry.
- `route-tile.tsx` renders the actual built-in or contributed page beside the
  chat. `store/route-tiles.ts` persists its path and docking edge. These pages use
  the window's ambient source/profile; a route tile does not itself claim a new
  independently pinned profile. Captured asynchronous operations still need
  their original owner fence.
- `preview-tile.tsx` mirrors preview tabs into the same pane tree. A file/artifact
  tab's identity includes its owning conversation; a URL tab is a navigable
  vessel with a minted id, not a singleton derived from its URL.
  `store/preview-ownership.ts` distinguishes durable session, pending runtime and
  true draft ownership, follows compression aliases, and shares pinned tabs only
  within the appropriate owner profile bucket.
- `store/preview.ts` hides a popped browser tab from its docked mirror while the
  native browser window owns it. Closing the pop-out re-docks that same tab with
  the latest URL/title. `adoptPersistedBrowserTab` re-homes a new renderer onto
  the actual owning bucket instead of duplicating the tab into the foreground
  profile. The popped flag is memory-only, so restart restores a docked tab.
- `electron/session-windows.ts` distinguishes compact interactive session windows
  (`win=secondary`), lazy spectators (`watch=1`), browser pop-outs, and full peers
  (`peer=1`). Full peers are ordinary complete app renderers. Their size cascades
  32px from the opener and is clamped to a work area. Compact windows use a
  420x620 minimum. The retained compact-window helper deduplicates by raw session
  id; the native implementation must use a source/profile-qualified key to meet
  the client's existing isolation rules rather than inherit that ambiguity.
- `electron/main.ts::createInstanceWindow` inherits the opener's current route
  unless explicitly given a profile route. `store/connections.ts` skips app-launch
  source restoration for a peer. `use-gateway-boot.ts` honors its captured boot
  route. In `new-session-route.ts`, an ordinary peer's boot seed does not override
  later generic New Session actions: those use the saved device default. Only
  an explicit `profileWindow=1` keeps the profile-window default pinned. Project,
  bot and existing-session actions retain their explicit captured route.
- `store/composer.ts` and `composer-draft-stash.ts` merge shared draft text using
  monotonically ordered per-key stamps and clear tombstones. Another window's
  stale whole-map write cannot resurrect a sent draft or erase a newer one.
  Local attachment buffers stay with their editor. Fresh chats mint lifecycle
  UUIDs. `pending-submissions.ts` stores admissions individually by UUID so one
  window cannot remove another window's pending submission.

## Current native limits

`Viewer` currently owns one foreground source, profile, selected session, route,
composer popup, tool owner and page-owner generation. `composer_scope()` derives
all of them from that foreground selection. `Conversation` already retains
multiple owner-keyed views and workers, but several Viewer action guards still
require the one current `connection_epoch`. Its existing worker, journal, CAS,
policy-stamp and recovery contracts should be reused, not duplicated.

The fifth checkpoint gives existing tabs actual independent `Composer` and
`TextEdit` controllers with stable `EditorId` values. Its v4 private draft store
already locks read/merge/write, applies per-entry CAS with monotonic revisions
and clear tombstones, and passes two-process contention contracts. These are
implemented prerequisites, not remaining whole-map draft work.

`Editors.bindings` still maps one `DraftScope` to one editor: two simultaneous
surfaces of one stored session need separate `SurfaceId → EditorId` bindings.
Two fresh conversations still share `DraftScope.session: None`, so they need an
independent conversation lifecycle UUID as well. `Composer::primary()` currently
uses one device identity, which must not be mounted concurrently in two windows.
Device/layout/display/profile/sidebar preferences now use locked leaf-patch
writes with explicit field intents and failed-write retry. Saved connection
registry transactions, live observation and per-window geometry still need
the corresponding shared services. Array/set fields remain atomic values; this
is not complete multiple-window state reconciliation.

`app/decks.rs::pane_visible` currently recognizes only fixed workspace/sidebar/
tool panes. `view/layout.rs` consumes caller-supplied pane bodies and does not
manufacture interactive session, route or preview controllers. Deck sanitation
already understands transient session/route ids, which is useful scaffolding,
not concurrent-surface implementation. `preview::Service` and page request
namespaces also need actual per-surface owners before multiple live panes can
share them safely.

## Ownership model and application seams

Introduce these client-only identities; they are not new gateway RPC parameters:

```rust
struct WindowId(Uuid);
struct SurfaceId(Uuid);
struct EditorId(Uuid);
struct SourceInstance { registry_id: String, revision: String, epoch: u64 }
struct ProfileOwner { source: SourceInstance, profile: String }
enum SessionIdentity { Fresh(Uuid), Stored(String) }
struct ConversationKey { owner: ProfileOwner, session: SessionIdentity }
enum SurfaceTarget {
    Chat(ConversationKey),
    Route { path: String, binding: RouteBinding },
    Preview(PreviewTabId),
}
enum RouteBinding { FollowWindow, Pinned(ProfileOwner) }
struct SurfaceRecord { id: SurfaceId, window: WindowId, target: SurfaceTarget }
```

The source registry allocates process-unique live epochs and retains source
leases. Epochs are not independent counters restarted in every window. Persist
credential-free source/profile/durable ids and placements, never a live epoch,
runtime id, token or forward port. `Pinned` is used only for an explicit owned
surface; ordinary route tiles retain their baseline FollowWindow behavior.

Separate window state from reusable application services:

- Window state owns layout, selected sidebar route, focused surface, popups,
  keyboard/IME routing, editor buffers, modal visibility and pane presentation.
- Existing Conversation workers own authoritative session views and mutation
  serialization. Sharing a transcript does not share its editor or disclosure
  state. Closing a surface detaches its subscription; it does not stop an agent
  or remove another surface's queued send.
- A workspace service owns source leases, surface/window registration, draft
  persistence patches and admission recovery ownership. Existing device sound
  and notification services retain their current shared brokers.

Required narrow application APIs:

```rust
open_chat_surface(window, target, placement) -> SurfaceId
focus_surface(window, surface)
move_surface(surface, from_window, to_window, placement) -> Result<()>
close_surface(surface) -> Result<()>
connection_for(owner: &ProfileOwner) -> Result<OwnedConnection>
send_from(surface: SurfaceId) -> Result<()>
stop_from(surface: SurfaceId) -> Result<()>
render_chat(viewer, surface, cx) -> El
render_composer(viewer, surface, cx) -> El
```

Existing main actions become wrappers around the focused surface. Every button,
menu, paste, file dialog, dictation completion, slash, retry and external submit
captures a SurfaceId and resolved owner at invocation. Pending callbacks do not
look up whichever surface is focused on completion. Namespace retained element
keys by SurfaceId; native window key/IME dispatch remains window-local. External
submit routing additionally carries WindowId/SurfaceId and a claim UUID so exactly
one editor consumes it. Model summaries and draft policy stamps remain specific
to that editor, while session policy truth remains per conversation.

Keep separate live EditorIds even when two surfaces show one session. A dock/
pop-out move transfers the same editor and attachments; an explicit second view
mints another editor initialized from the latest shared text stash. It must not
share TextEdit, attachment mutation handles or policy revision counters. Preserve
stable ids for restored surfaces, and never mount one persisted EditorId twice.

Extend the existing locked per-entry draft writer with shared observation and
surface/window ownership. Its current CAS/tombstones must be retained. A proposed
cross-surface receipt is `{ stash_key, editor_id, editor_revision,
store_revision }`; broker order supplies the monotonic store revision. A submitted
clear names the captured receipt and cannot clear newer text or another editor's
unsent buffer. Broadcast stash changes without replacing actively edited local
buffers/attachments. Preserve tombstones, unsupported-schema refusal and each
admission's existing UUID/CAS journal. Peer windows must also patch preferences
rather than save stale full Preferences snapshots. Preserve pending receipts if
all surfaces close or a renderer crashes; never automatically replay an uncertain
mutation merely because another window mounted.

Add PreviewTab records with explicit owner, target identity, pin policy, current
vessel state and optional owning pop-out. Each live preview gets its actual
Service/controller and cancellation token. Route request names include SurfaceId;
closing Settings cannot cancel another pane's artifact or history read. Scope
invalidations retire only the affected source incarnation/profile and its owned
pending UI work.

## Native window strategies

| Strategy | Assessment |
| --- | --- |
| Lazy static WindowSpecs | Suitable for a known Settings/About window. Not generic New Window parity: the number of possible states is predeclared. Windowless defers native creation but does not add runtime factories. No behavioral basis for a 64-peer limit. |
| Separate native process per peer plus workspace broker | Can reuse a complete Viewer and the current pinned Rune immediately. Needs private bootstrap IPC, a broker surviving primary-window close, shared draft/preference patching, source-lease retirement, crash recovery and application-level quit/focus/menu routing. Independent processes that merely share JSON files are insufficient. Existing read-only subagent child transport is precedent for private ownership, not a full interactive peer implementation. |
| Narrow Rune dynamic-host extension | Preferred final host model if we accept one explicitly maintained pinned framework patch. It preserves one application lifecycle/GPU pool and lets ordinary peers reuse Viewer without a second client-state IPC protocol. Surface ownership and shared persistence still must be implemented first. |

### Exact pinned Rune boundary

`crates/rune/src/app.rs::App::window(&mut self, WindowSpec<T>)` appends to
`App.windows` before `run(self)` consumes the App. AppHandle/WindowHandle can show,
hide or reopen that registered host; neither accepts a new runtime WindowSpec.
`windows.rs::flatten` and `tooling.rs::run_windows*` consume a FnOnce factory.
`window.rs::start_all` builds its hosts into `Windows.hosts` before resuming them.
`window/event_loop.rs` only starts that initial collection. The internal Vec is
not a public dynamic creation capability.

There is a second lifecycle distinction: App::window marks those windows as
secondaries that hide on close and never keep the loop alive alone. Ordinary
WindowSpecs in a multi-window run can have independent main lifetimes. A full
peer must not disappear just because the original primary window closes.

Propose an event-thread WindowSpawner that accepts a keyed factory and reports an
asynchronous creation receipt. Factory output is an AnyWindowSpec created on the
event thread; do not require Viewer/El/Rc state to be Send or synchronously wait
on the same UI thread. A thread-safe entry point can enqueue a Send factory that
constructs its state on that thread. A UI-thread entry point may accept local
captures. These are proposed APIs, not APIs available at the pinned revision.

The extension needs unique live keys, generation-qualified host slots, correct
shortcut/menu registration before first resume, creation failure receipts,
independent close policy and keeps-loop accounting, Wayland recreation, native
focus tokens, and removal of closed hosts. Reuse the existing `start`, AnyHost,
GPU weak pool and cleanup paths. Bound pending creation work as backpressure;
do not turn a queue bound into a maximum number of lifetime windows. Explicit
application Quit remains global; ordinary peer close is local. Reopen/Dock
activation must use retained application services even if the first host closed.

No Rune dependency or vendored source change is authorized by this audit. An
extension requires its own pinned patch/provenance and platform verification.

## Minimal staged vertical implementation

1. Add identities, source resolution and shared draft patching. Parameterize the
   existing chat/composer/actions by SurfaceId; make main a normal surface. Prove
   two fresh editors and two running existing sessions retain different text,
   attachments, model settings, prompts, queues and stop targets while focus moves.
   Use controlled gateways, including identical profile/session labels on two
   sources. No view-only tile milestone counts as this stage.
2. Populate real session/route/preview controllers in the retained pane registry.
   Exercise split/stack/move/close, dirty-page guards, compression aliases,
   preview pin ownership and source-specific reconnect/retirement. Persist layout
   and durable owners separately from live runtime bindings; resume only when the
   matching source is ready, with per-pane failure/retry.
3. Implement the chosen dynamic native-host mechanism, then open a complete peer
   using the same Viewer/window bootstrap. BootstrapKind must distinguish
   PrimaryLaunch, PeerInherited, ProfilePinned, InteractiveSession, Watch and
   PreviewPopout. Only PrimaryLaunch runs application launch restoration.
   NewSession in PeerInherited uses the current saved default; explicit profiles
   remain pinned. Register interactive compact windows by qualified conversation
   identity and kind; keep read-only spectators distinct.
4. Prove actual native independence: two full peers and concurrent chat panes,
   different sources and drafts, correct per-window IME/keyboard focus, a pending
   send when the originating pane closes, primary close while a peer continues,
   profile-pinned versus inherited NewSession, lost-ACK recovery without duplicate
   admissions, and browser move/re-dock with the same tab owner. Test stash races
   and receipts through real shared storage. For a Rune extension, separately
   test repeated create/close/reuse beyond any predeclared pool size without
   requiring dozens of GPU-heavy production Viewer states.

The major risks are accidental ambient-state reads, editor identity reuse,
whole-file lost updates, cross-source epoch collisions, child-process broker
lifetime, and native platform lifecycle/focus differences. Each is a concrete
acceptance boundary above. This plan does not introduce an agent-side API or
promise parity from replicated read-only views.

## Rechecked implementation boundaries after the editor checkpoint

Rune's element handlers require a `'static` application state, while `States`,
`Cx::retype` and the handler mapping are crate-private. Render borrowed explicit
`ChatSurfaceView` references, but keep callbacks typed to `Viewer` and capture
surface/generation tokens. A borrowed `Deref<Viewer>` facade does not redirect
inherited methods away from the foreground owner. Rune keyed layout lookup uses
the first matching key; every focus/anchor/timeline/input key must be explicitly
qualified by the surface, including overlays, rather than only wrapping a tree.

Keep Conversation's `DraftScope → ConversationView/worker` shared. Split Chat's
current combined presentation: disclosures, edit buffer, preview and completion
selection belong to a surface/editor, whereas unknown slash/branch mutations
remain serialized by conversation owner. Completion replies need an editor and
generation token in addition to owner/query. Surface close detaches UI without
stopping the agent, repeating a send or discarding an uncertain admission.

The existing subagent child transport closes on control EOF and its owner Drop
kills the child. It cannot establish independent full-peer lifetime. In Rune,
`App::window` registers only before `run`, and secondary windows do not retain the
application after primary close. A concrete event-thread extension can queue
single-consumption `WindowSpec<T>` requests from `Tx`, using the existing native
file-dialog call pattern, then start ordinary main-lifetime hosts through the
shared GPU/event loop. It requires generation-qualified host slots, creation
receipts/errors, cleanup, local Close/global Quit, and native tooling coverage;
no fixed predeclared peer pool is an acceptable substitute.

Before full Viewers share that host, give each an exclusive durable WindowId and
share source leases, settings observation/per-window geometry and pending-admission
ownership. Existing locked settings patches are a prerequisite, not this host. The
current UUID-per-file journal's separate `.settled` check/write is insufficient
as a multi-controller claim. Notification and sound arbitration already lives
in process-wide brokers; separate-process peers would need equivalent IPC.
