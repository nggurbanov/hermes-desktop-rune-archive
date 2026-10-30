# Prepared-send ownership across windows

A saved submission retains one UUID, payload and guarded mutation sequence across
worker/UI retries. Its private file now carries a monotonically increasing local
journal revision, independent of the gateway's conversation revision. Legacy
version-1 files without this field load at revision zero. A successful private
atomic write advances only its writer's in-memory CAS stamp; cloned snapshots
keep the old stamp. Invalid/future journal files are preserved.

Each submission has a nonblocking OS claim. The UI claims before creating,
retrying or confirming a prepared send, then transfers that same guard through
the worker channel. Queued work therefore has an owner before any network side
effect. The worker retains it through mutation/upload/admission/cleanup, checks
that its queued payload exactly matches the durable revision, and rejects stale
snapshots before opening a transport. Another controller receives an explicit
busy error rather than submitting the same input. Independent UUIDs remain
independent, including legitimate queued turns for one conversation.

Short state transactions use a separate lock for revision checks, saves and
settlement. A stale save cannot roll back newer mutation receipts; stale cleanup
cannot retire a newer saved state. Settlement commits its tombstone before
removing the spent payload. Locks are never stolen using PID text or timeouts;
closing/crashing the owning process releases the OS claim. Claim/lock files are
retained so another opener cannot lock a replaced inode.

Discard questions capture owner, UUID and journal revision when opened. Model
confirmation/cancel controls receive the revision emitted with their guarded
receipt. Both paths re-claim and compare that captured revision before modifying
the journal. A late question cannot authorize a later revision. Existing draft
policy stamps remain an additional independent model-choice fence. A failed
worker-channel handoff leaves the original saved input available for explicit
retry and clears its preparing presentation.

Recovery remains display-only. No new startup outbox or automatic legacy retry
is introduced. The legacy attempted marker is checked in the worker as well as
the UI. This is client coordination; it does not reproduce or modify the agent's
execution or replace the gateway's canonical admission deduplication.

Focused acceptance on 2026-10-29: five journal tests passed, with one ignored
private helper actually exercised by its passing parent using two processes.
Seventeen worker contracts passed, including two real WebSocket workers while
the first admission acknowledgement is held: the second issues no RPC, cannot
save/discard the active input, and exactly one admission occurs. The process
contract kills only its own claimed child and verifies OS release without a PID
heuristic. Existing lost-ACK, model guards, replay/rewind and admission contracts
remain green. Logs: `/tmp/hermes-submission-journal-tests.out` and
`/tmp/hermes-submission-worker-tests.out`.

The later final integration reran these contracts, including the queued-payload
assertion, in the passing 481-test default suite on 2026-10-30. Long claims and
short transactions use the same non-Clone file-lock guard: final logical release
explicitly unlocks even while a duplicated/inherited descriptor remains open.
Two deterministic journal contracts and a window-claim sibling first failed
before this correction and then passed; it does not rely on a fork timing guess. These contracts establish the
ownership mechanism, not complete peer-window restore, cross-window live
observation or platform/window-manager acceptance.
