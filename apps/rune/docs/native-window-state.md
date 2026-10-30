# Native window identity and persistence

A native window has a stable WindowId, an independent root EditorId and a
non-cloneable WindowLease. The lease holds an OS file lock for the entire mount.
A second process or another controller receives an explicit Busy result when it
tries to mount that identity; it must open a distinct peer or focus the owner.
A PID string or an old marker file is never treated as a live claim. The kernel
releases the claim if its process exits, including a crash.

The primary keeps the existing `primary-editor.json` identity so its saved
multimodal drafts/owner slots are not silently abandoned. Each new peer mints its
own WindowId and root editor. Restoring a released window reuses that exact pair.
The existing Composer slot store then derives stable per-source/profile/session
editor identities under the claimed root, without mounting the device primary
in two windows. Invalid legacy identity is an error, not a random fallback.

`windows.json` contains versioned credential-free records and per-window geometry,
last route and presentation. A source descriptor is either saved registry
identity/revision, an absolute local home, or a canonical URL with a boolean
indicating that authentication is required. No token, ephemeral SSH forward port,
live source epoch or runtime id is serialized. Saved/credentialed sources require
the current connection resolver or explicit credentials before restoration;
`connection_without_secret` cannot downgrade a protected URL to anonymous access.
Bot Chat routes retain the named registry intent rather than a pinned stored id.

Bootstrap kind and current navigation differ. A profile-pinned window retains an
immutable initial source/profile default in `pinned_route`, while `route` follows
its current visible conversation. Inherited peers have no pinned default. A
session bootstrap needs a captured stored conversation initially but may later
navigate to a fresh chat. Those records do not authorize a new submission.

Each transaction locks and validates the latest document, changes only its named
window and advances a monotonically ordered revision. A stale mount/snapshot
cannot replace a newer revision. Other window records and unknown root/record
fields survive. A normal close marks its record closed; merely dropping the
lease releases ownership and leaves the last successful state available for
crash recovery. Claim files stay on disk: unlinking a live lock inode would allow
a second owner to bypass the first.

A failed valid save retains its complete pending candidate and schedules a retry;
its successful baseline changes only after atomic private replacement. Invalid or
future documents are preserved. A failed close marker leaves the previous durable
record available for recovery. Saving the same successful snapshot is a no-op.
This service does not by itself establish a dynamic native host, window-manager
acceptance, source-lease restoration, automatic reopen policy or full peer parity.
The parent application owns those integrations.

Behavior contracts in `client/windows/tests.rs` cover legacy continuity and exact
restoration, independent peer editors, credential-free descriptors, concurrent
window updates/unknown fields, stale revisions, failure repair/retry, pinned
defaults and a real second process holding the primary claim then crashing. Their
run results are recorded by the integration checkpoint; this document is not a
claim that unexecuted contracts passed.


Focused Linux verification on 2026-10-29 passed all seven window-identity
contracts, zero failures. The one ignored child entry point was launched by
the passing actual-process parent contract, which observed Busy while the child
held the primary lease and reclaimed the same identity after killing that owned
child process. Log: `/tmp/hermes-window-identity-tests.out`. No agent, provider,
GUI host or physical device was used by these persistence/ownership contracts.

## Retained source service

Full peers retain both a Resolver and their authenticated source Lease. The
original connection UI's reply receiver does not own that service's lifetime:
a lost UI reply only detaches that sink. Resolver senders keep the worker alive,
including its SSH watchdog; dropping the final work sender closes the service
and retires its leases/client forwards. No agent lifecycle operation is implied.

While a source is active or retained, the worker observes its local saved-source
metadata at most once per second. A changed/removed revision expires the old
lease incarnation and closes its owned SSH forward. Invalid/unreadable metadata
also retires sources and is retained untouched. Saved UI operations/resolution
read current metadata before using it, with explicit one-time connections kept
independent of the saved registry. Observation does not fetch credentials, probe
an agent or start a connection. A later repair or same-id replacement creates a
new lease; it cannot revive an expired guard.

The application must also observe retained lease validity in each peer. The
source worker's original Waker may address a window that has already closed;
that is separate from retaining the worker and its source resources. The
persistence and actor contracts do not establish full peer/window-manager or
cross-platform acceptance by themselves.

## Registry mutation transactions and final focused verification

Saved-registry operations use a private OS-file transaction guard across the
latest reload, row mutation, provisional-keychain rollback/cleanup and any
activation probe. Direct Resolver acquisition has the same guard and a separate
locked inner path, so it cannot read credentials halfway through another window's
mutation. The lock wait is bounded to three seconds on the background worker;
contention returns an explicit retryable error before credentials or metadata are
changed. One-time connections stay independent of the saved registry. Read-only
watchdog observation uses atomic-file snapshots and never writes the registry.

Legacy draft migration also has a stable authority: the durable primary root
editor from the window registry (or its original identity file before the first
window record exists). Slot allocation, binding, shared-text seed and commits
carry that owner. A peer binding first cannot take the primary's legacy files or
replay payload. A restored v1–3 backup during a peer commit uses the same owner.

Final focused Linux runs on 2026-10-29 passed **18 connection contracts**,
**26 composer contracts** and **7 window contracts**, all with zero failures.
The one ignored private helper in each of the composer/window groups is exercised
by its passing default parent with actual child processes. Logs:
`/tmp/hermes-retained-connections-final-tests.out`,
`/tmp/hermes-peer-migration-final-tests.out`, and
`/tmp/hermes-window-identity-final-tests.out`.
The exact pre-fix worker loop from `181acb3036`, adapted only into the extracted
worker entry point, produced the two expected retained-source actor failures;
its fixed source was then restored before the green runs. RED evidence:
`/tmp/hermes-connections-lifetime-red.out`.

These contracts include three concurrent real JSON-registry Core writers,
unchanged metadata/keychain while another OS guard is held, preserving unrelated
rows and credential references, and a resolver probe holding the transaction
before another writer can rotate its credentials. They use private files, fake
keychain/probe/tunnel services and no real SSH, agent, provider or physical device.
Full peer rendering and platform acceptance remain the parent integration's gate.


## Saved connection editor revision guard

Opening a saved connection captures its owner revision in the editor. Save carries
that exact revision into the already-locked registry mutation; a changed or missing
capture is rejected before destination normalization, keychain access or a probe.
The rejected form keeps the user's entered label, destination and token, with an
explicit error asking them to reopen the latest connection. New connections and
one-time connections retain their existing behavior. A label-only save from a
current capture preserves the owner revision; changing credentials with a current
capture creates the usual new revision.

Focused Linux verification on 2026-10-29 passed **20 connection contracts**, zero
failures: `/tmp/hermes-saved-editor-green.out`. The two added contracts first failed
without the guard (`/tmp/hermes-saved-editor-red.out`, 14 passing Core contracts and
two expected failures). They include two independent JSON-registry instances and
the actual Connections form/reply flow: an anonymous destination changed in
another window cannot be silently restored by a stale form, with or without a
newly entered token. Rejection preserves the registry bytes, performs no keychain
operation/probe/tunnel creation and keeps the entered form. Explicitly reloading
and reopening the current row then saves successfully and clears the error. A
separate contract verifies missing capture rejection before URL validation and a
successful credential rotation with the current captured revision. All use private
state and fake external services.
