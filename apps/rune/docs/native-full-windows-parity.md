# Complete native Hermes windows

The complete Viewer is constructed by one application factory. The primary,
ordinary peers, stored-session targets and profile-pinned Bot Chat targets all
mount that same Hermes sidebar, routes, transcript, composer, theme/components
and feature controllers. A subagent spectator remains its separate read-only
surface; it is not used as a substitute for an interactive conversation window.

Rune is retained at one upstream revision with a recorded local native-host
patch (see `vendor/rune/HERMES.md`). Dynamic windows own independent host state
in the existing event loop; no finite preallocated pool or second GUI framework
implements the complete Viewer. A native creation receipt reports queued,
registered, created or failed state. The opener is not a lifetime owner.

## Identities, routing and local state

A mounted window claims one durable WindowId/root EditorId. Its per-source,
profile and conversation input slots have separate actual Composer controllers,
including their TextEdit history, files, staged choices and pending callbacks.
Each window also has a fresh process-wide source epoch: identical stored session
names in two windows do not accidentally address the same in-memory controller.
The durable window record and the current input-slot editor are different IDs.

An ordinary New Window captures the current source/profile/presentation for its
initial conversation. Later navigation by the opener does not retarget it.
A stored-session context action captures the source/profile/stored id; a Bot
context action captures the named canonical Bot Chat intent and a pinned
source/profile default. Reopening the same qualified dedicated target focuses
its existing window. An ordinary peer has no pinned New Session default merely
because it inherited its opener's initial route.

Saved sources retain their Resolver service together with a Lease. Opening a
peer from an inherited SSH source uses that retained service, rather than a new
connection manager that would allocate a different forward. The peer validates
that its captured principal/revision is still current before using it. Registry
observation retires changed sources; retiring the opener does not stop a service
still held by another peer. Saved source transactions and stale form revisions
are described in `native-window-state.md`.

Window records contain credential-free descriptors, route, presentation and
logical size/native position/maximization. Protected URL restoration requires
credentials; a missing saved revision does not silently connect somewhere else.
Bot Chat records store the named intent rather than pinning a stored id. Draft
legacy migration always belongs to the durable primary editor, even when a peer
binds its source before the primary does.

## Commands and persistence

New Window is Mod+Shift+N, Close Window is Mod+Shift+W, and Quit Hermes is Mod+Q.
The Hermes name's context menu exposes the same actions plus New Conversation
and Settings. Stored session and Bot rows expose Open in New Window in their
existing context menus. Mod is Command on macOS and Control elsewhere; the
ordinary keybinding preferences can replace these defaults.

Local Close checks owner-changing operations and forces all actual editors,
main/display/sidebar/profile preferences and the window record to save. A failed
save retains input/intent and vetoes the close. An untouched initially unreadable
preference file is preserved without trapping its window. A genuinely edited
unwritable presentation is still an error. Pending window saves respect their
retry deadline during ordinary UI polling; Close explicitly forces the latest
presentation. Global Quit and OS termination still have separate pending
coordination work below.

Prepared submissions use an OS claim held through queueing and the worker's
network operation, with durable revision/payload checks. A second window cannot
retry or discard the active revision. See `native-admission-ownership.md` for
actual process/socket contracts and recovery limits.

## Acceptance and remaining scope

The public Rune Runtime contracts, native 80-cycle lifecycle proof and actual
GPU surface creation-failure proof are framework evidence. The ordinary complete
Hermes application has a separate private-peer/native-window acceptance suite,
`scripts/check-peer-windows.mjs`; its results must identify the immutable app
binary. Neither renderer geometry nor framework-only examples establish the
complete application's accepted behavior. The integration ledger records final
run counts and binary hashes.

Still required for full parity:

- Device-default New Session/fleet startup routing (native-startup-route-parity.md),
  full fleet controls/activity,
  live shared preference observation and theme/default propagation semantics.
- Full simultaneous conversation/route/preview tiles and plugin pane geometry;
  remaining compact interactive session/window presentation and restore policy.
- Complete topology restoration, offscreen/monitor/work-area clamping and native
  placement/maximization/focus/minimize behavior across the three target systems.
- Application-wide dirty-window Quit preflight and OS shutdown policy, final
  background-service/owned SSH-child exit coordination, creation-failure record
  rollback, stable saved-source draft identity across SSH forward recreation,
  and actual inherited SSH peer acceptance.
- Native clipboard, all feature controllers across multiple surfaces, external
  host acceptance and Linux/macOS/Windows package/platform checks.

These are active requirements, not waived features or a smaller MVP definition.
