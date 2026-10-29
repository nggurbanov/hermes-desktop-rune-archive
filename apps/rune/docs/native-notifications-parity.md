# Native notifications

The original authority is `apps/desktop/src/store/native-notifications.ts`,
`notify-baseline.ts`, `store/agent-notices.ts`, `electron/notification-ipc.ts`,
`notification-linux.ts`, `notification-registry.ts`, and `notification-actions.ts`.
This increment transfers Linux native delivery. It does not establish full
notifications, sounds, HUD, or cross-platform parity.

## Implemented behavior

- Device-owned `notifications.json` stores enable-all and all seven original
  kind preferences, independently of agent config. Defaults match the original.
  Writes use the existing private atomic writer, preserve unknown fields, and
  never overwrite invalid, unreadable, or newer-format preference files.
- Live accepted `message.complete`, `message.error`/`error`, approval, clarification
  and input events can request notifications. Only urgent `credits.depleted`,
  `credits.window_limit`, and `credits.restored` account notices can interrupt.
  Regular usage notices remain quiet. Account notices capture no conversation
  target, so their click focuses Hermes and their cross-window dedup key is shared.
- The actual conversation socket open/reopen establishes a four-second quiet
  baseline for that connection/profile. Hydration alone does not restart it.
  Accepted replay events never raise OS notifications. The conversation owner,
  runtime, epoch, authority, execution generation and sequence fences run before
  the notification event is emitted.
- Approval/input notices can appear for an off-screen conversation while the
  app is focused. Completion/error notices appear only while away and only for
  the active conversation. Global account notices appear while away.
- A process-wide broker deduplicates kind/principal/profile/durable session/
  runtime/tag keys for one second; concurrent duplicates share the real delivery
  result. Both ingress and outstanding deliveries are bounded to 32, duplicate
  waiters are bounded, and each client can have one service probe in flight.
  Queue saturation is reported synchronously, without a stuck test action.
- Native Linux delivery uses asynchronous zbus calls. A notification id is
  bound to a daemon unique sender and generation. D-Bus receive sequence also
  prevents a stale owner-query reply from rolling the daemon generation back.
  Optional action and sound support is read from real capabilities.
- Delivery takes at most five seconds. An uncertain timeout is never replayed;
  a ten-second failure cooldown is scoped to the addressed daemon generation.
  Service replacement does not inherit a retired daemon's cooldown. Signals
  arriving in the same batch before the Notify reply are correlated safely.
- Approval buttons capture only an offered allow/deny pair from the installed
  prompt, its id and execution generation. Only the originating live client
  receives an action; root validates current principal/runtime/prompt/generation
  and choices again before responding. A body click can fall back to the primary
  window, carrying durable profile/session identity and no executable action.
  Existing chats are revealed; another chat is opened beside the current chat.
- Handler retention is ten minutes; click, close, daemon replacement and client
  disposal release appropriate resources. Expiration asks the captured unique
  daemon to close its notification. Optional Wayland activation tokens are
  retained for click-driven window activation.
- The native Settings page reports actual service availability. Explicit
  **Send test notification** bypasses focus, preference and quiet-window policy,
  then reports whether the desktop service accepted it. Service acceptance does
  not promise the user saw a banner: desktop policy and Do Not Disturb remain
  owned by the OS.

`Notifications::dispatch` returns **queued**, never delivered. Delivery outcomes
arrive through `poll`; future plugin callback registration must wait for actual
acceptance rather than treating the queued boolean as delivery success.

## Explicit remaining work

- `backgroundDone`: the original producer is the client composer-status task
  transition queue. That producer has not been connected to this native broker.
- `plugin`: the original producer is the renderer plugin notification/callback
  bridge. Native callback ownership/deep-link navigation from that bridge is
  not transferred. Both preference values are retained, with clearly unavailable
  controls; neither is counted as implemented notification delivery.
- Completion sounds (14 presets plus None), sound preview, haptic hardware cues,
  mute synchronization, full interactive HUD and Quick Entry remain separate
  increments. The broker supports an explicit silent notice; there is no new
  fake sound or haptic toggle.
- macOS UserNotifications and Windows toast adapters, real application identity,
  permission flows, packaging and host-native acceptance are not implemented by
  this Linux adapter. Other platforms report unavailable, rather than success.
- Rune's `App::on_focus` supplies real native focus transitions, including normal
  Linux minimize/focus loss. Explicit host visibility is also supplied. Pinned
  Rune's public `AppHandle::is_visible` counts minimized/occluded windows as shown;
  it exposes no public occlusion callback. Pure occlusion without focus loss is
  therefore not claimed equivalent to Electron `document.hidden` in this slice.
- OS/compositor behavior varies: a daemon can omit actions or activation tokens,
  and Wayland may request attention rather than granting focus. An installed
  Hermes desktop-entry/icon identity still needs distribution acceptance.
- Device preference changes are local to the loaded window; complete live
  cross-process preference reconciliation is not yet transferred.

## Validation

All ten focused notification tests passed on 2026-10-29. The final frozen
341-pass Rust suite also included the corrected account-target normalization
assertion and root activation routing contracts.
Six contracts cover preference privacy/preservation, every kind/focus/session
combination, unbound socket baseline, actual approval choices, captured origin,
spoofed senders, same-batch clicks, retention and owner replacement.
Four integration tests launch a private `dbus-daemon` and fake notification
service with an explicit bus address. They exercise actual zbus Notify payloads,
silent hints, action capability, shared delivery/failure dedup, action origin,
spoof rejection, early click, daemon replacement/id reuse, invalid ids, timeout,
cooldown and no replay. They never connect to or notify the user's desktop bus.
Five native Settings checks passed against the immutable third-checkpoint
binary, including light/dark rendering, all seven categories, unavailable-source
controls, master-disable behavior, a disabled test action, minimum-height scrolling
and return to the retained conversation. Trees serialize disabled RuneKit
controls as `focusable: false`, not a separate `disabled` property; the script
checks the actual representation and confirms tapping the disabled test invokes
no preview action. Screenshots/trees are in `/tmp/hermes-notifications-third`,
with the log `/tmp/hermes-notifications-third.log`. These fixture checks never
start native notification delivery. See the functional ledger for combined
checkpoint evidence.

Protocol/API references: [Desktop Notifications Specification](https://specifications.freedesktop.org/notification/latest/protocol.html),
[zbus connection builder](https://docs.rs/zbus/5.19.0/zbus/connection/struct.Builder.html).
