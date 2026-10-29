# Independent native composer editors

The editor is a mounted input, not a backend conversation. `DraftScope` still
owns the connection epoch, profile and optional stored session used by gateway
requests. `EditorId` is a private client UUID and never becomes a gateway RPC
parameter. Each `Composer::with_editor(id)` owns its TextEdits, attachments,
policy identities, popup/search state, catalog worker and pending readback fence.
The application must never mount one restored EditorId in two live surfaces.

`Composer::primary()` restores the private primary UUID, creating it once under
an OS file lock. A malformed identity file is retained and reported. The real
application bootstrap uses this path; fixture editors use fresh random UUIDs.
Existing tab controllers restore their explicit UUID through a locked per-principal
slot registry keyed by primary window identity, profile and optional session.
Fresh-to-stored binding moves the registry entry, and profile rename/delete
updates its durable ownership without deriving a new UUID from a renamed label.
Future independently restored surfaces retain their explicit layout identity.
Closing or moving a surface must transfer or retain the actual Composer where
appropriate, not clone its TextEdit/history/attachment state.

`fork_editor(scope)` creates a fresh editor and text model with a new policy
identity. Text and model choices seed the explicit second view; local attachment
and replay buffers, selection/IME/undo state, popup and network handles do not.
`seed_stored_draft(scope)` is the cross-window seed when the source editor is not
available: it reads only the latest stored-session text stash, only into a missing
local draft on the bound epoch. It never imports another editor's attachments or
seeds a fresh `session: None` input. A latest clear/tombstone prevents an older
text from being adopted. Existing live buffers remain untouched.

## Shared persistence

Version 4 stores entries by `(EditorId, profile, stored-or-fresh session)` inside
the existing per-connection-principal file. All editors use the same private
lock file for a read/validate/patch/atomic-save transaction. This serializes
separate processes too. The lock is separate from the atomically replaced data
file. Directories and created files retain the client 0700/0600 privacy policy.
There are no gateway tokens, live epochs or SSH forward ports in this data.

A composer compares its new snapshot only with its own acknowledged baseline.
Only changed entries and changed favorite keys become patches. Every commit
allocates monotonically ordered revisions; removed entries leave tombstones.
Each patch carries its observed revision. A stale writer cannot replace a newer
entry, clear newer unsent text, resurrect a removed draft or reverse a newer
favorite choice, even when an unrelated local key was saved in between. A
conflict retains the live unsaved buffer and reports the issue instead of
claiming a successful save. Unchanged stale entries never become writes.

A failed write retains the latest pending snapshot and retries without requiring
another edit. Rebinding first flushes only the previous principal/epoch; data
from another live source cannot enter that file. Debounce scans retain the
existing local revision gate. Delayed policy acknowledgements still use the
captured draft UUID and field revisions; a sibling editor's policy stamp cannot
acknowledge the active editor's choices.

Versions 1, 2 and 3 migrate once under the same lock into the restoring editor.
Draft policy stamps, text, model choices, attachment paths and frozen replay
payloads retain their existing representation. A subsequent secondary editor
receives no migrated local attachment buffer. Unsupported, malformed, mixed or
ambiguous revisions refuse writes and retain the original file. The version-4
container preserves unknown top-level fields. Downgrade to clients that only
understand version 3 is deliberately read-only through their existing version
check; this implementation does not emit a stale lossy version-3 mirror.

## Verification and limits

The focused contracts exercise independent fresh and same-session inputs,
policy-stamp isolation, exact restoration under new live epochs, real temporary
storage merges, stale clear and tombstone resurrection, concurrent favorites,
all three legacy migrations, corrupt/future refusal, failed-write retry,
source rebinding, stable primary identity and explicit shared-text seeding.
A parent test also launches two actual test processes, gates both after their
initial read, releases concurrent edits to one private file and verifies both
owned entries survive. Its ignored child entry point is exercised by that parent
test; it is not an unverified optional acceptance case.

The real application now parks a complete Composer per existing tab controller
and restores it when that tab is selected. Inactive controllers drain their own
catalog replies and draft writes. Policy truth reaches every mounted draft of
the same conversation; receipts acknowledge only the exact originating policy
identity and field revisions. File-chooser callbacks capture EditorId and scope,
including a fresh-to-stored alias, so a late result cannot attach into the newly
selected input. A closed tab retains its controller for pending acknowledgements.
A missing/failed editor binding blocks the mismatched input rather than writing
into the previous controller. The application bootstrap retains its explicit
read-only flag when the primary identity could not be restored.

The legacy primary Draft transfers as the actual object, retaining files, replay
and policy identity. The target entry becomes durable before the source is
removed; failed target save rolls the object back. This permits a recoverable
duplicate between writes but never an empty crash gap.

This remains the state/tab-controller prerequisite. It does not establish actual
simultaneous pane rendering or dynamic peer hosts. The existing tab topology
still deduplicates a source/profile/stored session and permits one fresh backend
owner per source/profile. Concurrent fresh conversation lifecycles need their
own conversation ownership identity, not only different Composer objects.
Shared live draft buffers are intentionally not overwritten by stash writes.
Cross-process live change notification, surface-layout restore and global
preference reconciliation remain the corresponding application-service work.

Focused acceptance on 2026-10-29: 25 composer contracts passed, plus four actual
Viewer/tab-controller contracts. One ignored private process entry point is
executed by the passing default parent test, which launches two processes.
Logs: `/tmp/hermes-editors-pool-compile.out` and `/tmp/hermes-editor-app-tests.out`.
The production native script `scripts/check-editors.mjs` adds five real Rune
render/interaction cases for text/files across tabs and late file-dialog returns
in both themes; the fresh-binary parent result is recorded below.

2026-10-29 parent acceptance: immutable binary
`02bca4b8963a81fa79c1b59b57dbd5216f531f7f7e76520c94d36a7f70829d77`
passed all five `check-editors.mjs` native cases. The first harness attempt
overlapped an 80 ms attachment-button press with the next tab click; an explicit
20 ms hold corrected the input script and the full run passed. The parent added
a real delayed text-input callback fence and a fifth application contract: old
frame input updates its original parked editor, while a retired profile receives
no live draft change. Returning to a healthy active editor clears a failed
activation's blocked presentation. The complete default suite passed 421 tests
with zero failures and the two documented ignored entry points.


The sixth preference integration reran all five native editor cases successfully
on immutable binary `1ccda04a1714fb312aeb4fa4f404e6f7c714bbe609517368f22248a2083896dc`.
Its complete default suite passed 439 tests; safe preference persistence is
separate from the remaining simultaneous-surface/window ownership work.
