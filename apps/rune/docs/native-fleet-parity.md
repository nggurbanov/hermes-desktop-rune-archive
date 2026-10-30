# Native fleet and source-owned sidebars

Implementation against the retained Electron baseline in `desktop-baseline.json`.
This is one part of the full migration; the legacy/project plan remains in
`native-fleet-next-slices.md`, and exact legacy precedence is in
`native-legacy-routing-contract.md`.

## Source ownership

`source.rs` distinguishes a logical source/version from its authenticated runtime
stamp. A saved source has its registry UUID and configuration revision; its
runtime also includes the actual principal/forward endpoint. A sidebar row gains
native provenance from the fetch, never from a backend-provided `connection_id`.
A session action carries source, profile and stored ID. Same-named profiles,
compression roots and stored/runtime IDs on independent agents cannot share row
keys, edits, live status or navigation ownership.

A saved binding retains its actual source lease and original connection actor.
The concrete actor keeper also follows a queued/sent page request and background
worker through its final response; a strong lease alone cannot keep an SSH
actor alive. This avoids a lifetime cycle through the actor's own primary lease. Source retirement clears obsolete visible
data while preserving unresolved edits for reconciliation on the original
version. Durable edit receipts use source-qualified namespaces and real OS
claims, held by the actual in-flight request even if its UI controller closes.
A changed credential revision cannot inherit the old edit. A new SSH forward
for the same saved revision may reconcile the original UUID/payload/preconditions
only after it has a fresh verified binding.

## Passive inventory

`fleet` is shared by native windows while each Viewer has its own subscription.
The cache is independent of the foreground editor. Descriptor changes, mount,
focus, visibility, successful foreground attach and explicit refresh are edges;
cache expiry alone does not issue network calls. Reads coalesce in flight, with
one forced follow-up, 60-second successful freshness and 5-second partial retry.
Known backend identities have a 5-minute positive and 1-minute negative TTL.
Transient identity failures preserve a known identity. Only a proved matching
`install_id` and profile can deduplicate a display row; authentication and leases
remain attached to their actual source.

Existing Local attachment uses a read-only ticket without `gateway ensure`.
Deferred Local remains connect-on-demand. Undialed SSH uses bounded metadata
inventory with no forward, install, gateway start, dashboard or provider call.
Local Linux/macOS inventory processes use owned groups with unreaped-leader
identity and nonblocking deadline-aware pipes, including inherited helper
stderr. Linux process-tree contracts pass; macOS execution and Windows
Job-object ownership remain open. A never-seen unavailable URL invents no profiles. Remote/SSH previous successful
names survive temporary failure; Local failure does not reuse stale names.

Background credential retrieval does not use the active keyring API, which can
unlock or prompt. The Linux adapter pins an already-running Secret Service,
reads exactly one unlocked matching item and bounds the entire transaction.
Missing, locked, ambiguous and expired credentials remain explicit failures.
Other hosts require a retained authenticated lease until their noninteractive
adapter is implemented; this is not cross-platform credential acceptance.

Foreground page data initializes only after the correlated source/profile
receipt is accepted. Installing a Connection is not proof that its requested
profile still exists. Owner changes synchronously clear old page data; deferred
initialization resumes for the same owner after acceptance. Explicit reconnect
revokes acceptance, while an ordinary history-read failure does not. Automatic
loaders, polling and explicit page operations share the same gate; local
Settings/Connections navigation remains available. Internal owner-data reload
also preserves an explicit pending destination; only public user navigation
supersedes it. Rejected owners show unavailable/connection copy rather than
claiming Ready or an in-flight Cron load.

## Native presentation and navigation

Fleet rail and replacement picker use Rune/OpenCode components in the original
Hermes sidebar/footer locations. Whole-fleet square count switches at 13 to 14.
Groups have stable source-owned order/aliases/collapse. Profile order and colors
are intentionally shared by profile name, as in the baseline. Simple mode may
force a rail for multiple profiles/sources; Advanced respects its rail setting.
An offline or connect-on-demand source keeps its own status/action door.

A known display alias may show the actual active choice in the canonical
backend slot while preserving its actual source, keys and actions. Display
canonicalization never authorizes a different saved connection. Context actions
capture their source without selecting it. Opening a profile/session reuses the
existing resolve, exact-profile proof, cancellable commit and owner/input fences.
At-rest Direct routes use their retained captured binding, without substituting
the current connection or registry primary. Re-tapping the actual current profile
outside All preserves its open chat and editor. A changed profile, return from
All or at-rest source/profile pick starts an ordinary fresh draft after exact
activation; canonical Bot Chat is a separate action, not the rail's default. New peer windows capture the same
source-qualified target and retain the owning actor.

Each source has a separate sidebar controller, actual network scope and edit
receipt. At-rest session actions do not activate that source. A source switch
parks actual tabs/editors and retains live conversation workers. Returning to
that source restores its epoch, selected session and real input controller.
A delayed native file chooser finishes in its original input; a stale text event
from an old frame cannot type into another source. A fresh send which receives
its stored ID while parked rehomes only its original input/tab. A later fresh
input never aliases that stored session. A cache-only source retention guard
preserves an inactive Direct route without registering another UI or inventing
successful roster metadata. Restoration adopts that exact guard before it drops.
Tab events capture source/profile/session rather than a reused index. Parked
catalog/completion replies settle only their actual original chat, and background
process monitoring retains validated owners while another source is foreground.

## Verification

Controller and real private socket/SSH/bus contracts are followed by the actual
native `scripts/check-fleet.mjs` acceptance harness. It uses two strict loopback
peers with colliding names/IDs, different history and installation identities,
private client stores, a private X11 window manager and native input. Opt-in
inspection observes real editor bindings and the actually presented frame; it
provides no runtime setter. The evidence is recorded with the tested executable
and harness hashes. The ninth frozen executable passed 607 Rust tests, 97
renderer/script-input cases, 16 startup cases, three fleet cases (23 checks),
complete peer/strict clipboard acceptance and the rich read-only subagent
miniwindow. Exact evidence and limits are in `native-fleet-acceptance.md`.

## Remaining full-parity work

- Full legacy null-source resolver, durable global/per-profile remote overrides,
  Cloud/OAuth/header envelopes, credential import/re-entry and backend profile
  alias translation. `SourceId::Legacy` is a distinct domain value, not a guessed
  synonym for the actual Saved/Direct connection.
- Entered projects/Home/cwd precedence, simultaneous session tiles and complete
  persisted multi-window topology. Per-source parked workspaces are not tiles.
- Attention beyond loaded sidebar rows: proved-owner unlisted live runtimes,
  hidden Bot Chat and persisted unread markers; suppress the active indicator
  and exclude cron-only completion exactly as in the baseline.
- At-rest identity image loading and the remaining fleet/GUI SDK shared consumers.
- Full source-qualified completion notifications for parked sources. The existing
  broker has one current principal, so parked background completion delivery is
  explicitly suppressed until its owner-aware routing is implemented; it must
  never dispatch A's completion under B's principal.
- Remaining browser CSS color paint, skin/font migration and all remaining drag/hold/context,
  narrow-window, screen-reader and multi-window acceptance.
- macOS/Windows noninteractive credential access, local/SSH inventory, native
  input/window-manager behavior and actual packaged distributions. Linux tests
  and the private bus are not evidence for these platforms.

No user agent/provider, physical microphone/speaker or user keychain is used by
these isolated acceptance lanes. The full migration goal remains open.
