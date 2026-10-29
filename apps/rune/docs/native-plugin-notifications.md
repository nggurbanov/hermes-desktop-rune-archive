# Native plugin notifications

The retained original SDK calls `hermesDesktop.notify` for OS notifications and
keeps `ctx.os.notify` callbacks in its own WebView. The Rune bridge now exposes
that method separately from the in-app `host.notify` action. Its Promise resolves
true only after the native notification daemon accepts delivery; suppression
returns false, and service failures reject. Failed requests clear only their own
`notifyId`, leaving other accepted callbacks intact.

Delivery captures the parent connection principal, fresh child window token,
source connection incarnation, requested profile and SDK callback id. Queued
requests and delayed delivery/activation recheck those owners. Source ownership
uses a weak lease guard, so a toast cannot keep a retired SSH source alive or
reattach to a replacement source with the same name. A button activation is a
typed plugin action, separate from an agent approval response. Closed or replaced
children receive no callbacks. A retained secondary source uses its own host
ownership independently of the main foreground profile. A retired body click can
only focus the parent application; it does not select a chat or execute plugin code.

Background eligibility uses actual Tao window focus/visibility, not the parent
window's focus. Renderer IPC is wrapped as a web message before reaching the
parent; it cannot forge reserved native window-state or lifecycle messages. A
valid notification activation first issues an actual native focus command, then
invokes the original SDK closure and resolves its child-local router path.
Closing/dismissing a notification, consuming it, changing its source/profile,
replacing its plugin, or retiring its daemon clears retained handlers. Daemon
retirement retains every callback id; the facade drains at most 128 exact ids per
frame and schedules another wake for the remaining backlog.

`hermesDesktop.claimAmbientCue` also joins the existing device sound worker.
Completion keys must name the child's actual owned runtime, then queue the same
native completion actor and principal/profile/runtime claim as native events.
The bridge returns false to the SDK sound continuation: the native actor plays
once using the latest device mute and preset, so stale child WebAudio state cannot
race a new mute or choose an old sound. The original child preference atoms also
mirror device mute and preset for its UI and OS-notification silence. Speech keys
retain boolean ownership and are separately namespaced by the owned runtime. This does not establish native/SDK
speech segment deduplication: native TTS still needs the same original segment
keys before that can be claimed.

The Linux broker retains its bounded ingress, pending requests and daemon-owner
fences. Actual OS delivery is currently provided by the Linux D-Bus adapter;
other platforms retain their existing explicit unavailable state.

## Verification

- Private D-Bus service contracts cover delayed acceptance, timeout, actual action
  labels, typed SDK callback ownership, parent-focused/child-background behavior,
  and per-plugin throttling. They never send production desktop notifications.
- Owned-host contracts reject replaced windows, changed parent/source/profile,
  forged renderer native facts and retired plugin navigation. A weak-lease test
  proves notifications cannot retain or revive a retired source.
- `check-plugin-notifications.mjs` drives the original SDK in a real Wry window
  using synthetic parent replies. It exercises false/error cleanup, body and
  button closures, dismissal, child routing, and actual focus/visibility commands.
- Existing original SDK and cross-source routing smoke scripts understand the
  private IPC envelope without weakening their semantic assertions.
- SDK assets are rebuilt with `build-plugin-host.mjs`. The manifest records all
  source inputs and dependencies. The combined third-party notice is LF-normalized;
  individual dependency license-file hashes remain hashes of their original bytes.
