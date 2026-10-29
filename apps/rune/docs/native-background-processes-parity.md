# Background process feedback

Authority: retained `apps/desktop/src/store/composer-status.ts`, its tests,
`app/chat/composer/status-stack/index.tsx`, and message-stream tools/status
handlers. These are `terminal(background=true)`/process-registry jobs, not agent
subtasks. A subagent final or invented task-complete event never implies a
background process ended.

## Transferred behavior

- A bounded two-worker lane issues authenticated `process.list`/`process.kill`
  against the captured connection, profile and actual runtime. Upstream handlers
  select the process-owning session by runtime; no same-socket attach is required.
  Kill checks the process's session ownership on the backend. The generic native
  workspace RPC allowlist now admits exactly these two methods, with snapshot
  classification remaining read-only and kill remaining mutating.
- Each owner/runtime binding has independent snapshots, dismissed ids, pending
  request ids and a generation. Old readbacks cannot alter a new binding.
  Source retirement/runtime replacement invalidates queued I/O before it opens
  the retired endpoint, removes rows/expanded output and never generates a stop.
- Only an already observed `running` row changing to exact registry status
  `exited` emits BackgroundDone. First-seen exited rows, disappearance, repeated
  snapshots, dismissed rows and other terminal-like status aliases do not emit.
  Exit 0/undefined is done; nonzero is failed. The notice uses the command's
  trimmed first line, optional exit code and last three trimmed output lines.
  Notification device preferences/focus/session/quiet policy still decides
  whether an OS notice is submitted.
- Existing rows keep their order, new processes append. Completed rows linger
  four seconds; failures linger twelve. Repeated snapshots do not restart the
  timer. Dismissed ids stay hidden while reported and are released after a
  snapshot no longer reports the id. A process becoming running again cancels
  its finished deadline.
- Opening a binding seeds its snapshot. Accepted live terminal/process
  `tool.complete` and process-kind `status.update` request refreshes. A five-second
  safety poll runs only for known-running rows in the visible conversation.
  `Tx::wake_after` services refresh/backpressure/linger deadlines when no other
  animation or sidebar activity exists; an idle empty controller does not poll.
  Event-driven refresh remains available while the app is away.
- Native process rows appear above the composer with state, expandable/selectable
  output, Stop and Dismiss. Their scroll height is bounded so the composer remains
  usable. Stop preserves a row on failure/unconfirmed response and removes it
  after confirmed killed/already-exited/not-found. A lost acknowledgement does
  not enqueue another kill; later snapshots reconcile the actual outcome.
- A confirmed edit/retry/restore rewind marks known ids dismissed and clears
  rows before asynchronous kills, preventing an in-flight snapshot from restoring
  abandoned rows. Ordinary turn Stop leaves background processes running.
- A genuine runtime-not-found verdict latches polling and requests owned
  conversation recovery. Actual socket reopen/runtime rebinding can rearm it;
  simple remount cannot. Process RPC errors preserve a sanitized reason token:
  explicit `not_found` wins, another/malformed present reason does not become
  missing, reasonless legacy 4001 retains its contract. 4004/auth/timeouts do not
  become permanently-gone sessions.

## Evidence and limits

Ten focused background tests passed, including actual controlled WebSocket
snapshot/kill ownership, lost kill ACK followed by read-only convergence, reason
priority from actual gateway errors, exact transition rules, stable order,
linger/dismissal, rewind fencing, visibility/dead-runtime timers, and cancellation
of queued retired-source I/O. The separate page classifier contract passed.
No live agent or real process was changed.

The `background-processes` native scene uses readonly fixture rows for running,
success and failure; Stop is disabled. All ten `scripts/check-feedback.mjs` native
checks passed on immutable binary SHA256
`056c315e07a355a98414199b844aca6567ba7ed2ff0c4cbc62662f213af68ff8`.
This includes both themes, output collapse/reopen, local dismissal and native
sound settings without opening audio hardware. In a 1024×520 window with an
18-line draft, wheel scrolling reaches the last process row, the input viewport
and toolbar remain inside the workspace, and the complete composer dock ends at
the workspace's lower edge. The bounded process group shrinks before clipping
the composer. Screenshots and element trees were inspected in
`/tmp/hermes-feedback-fourth-final`, including `background-short-draft.png`.

The 374-test passing Rust run and isolated audio proof used the preceding
immutable binary `3c06534d7a645ee72f43e131d8a0c85de8eb149f77f42eccd82e36dd75056ed6`.
The only production Rust change between these binaries is the process group's
44-point minimum and flex-shrink; the new binary passed a fresh build and the
ten native checks above. Native notification checks passed 5/5 on the preceding
binary; that screen and its behavior did not change with the layout fix.

The five-second visibility predicate uses pinned Rune's public shown-state API;
pure occlusion/minimize without its focus transitions is not exposed as an exact
Electron document-visibility equivalent. Actual external-host/version acceptance,
full original process popout/window interactions and the sidebar's dedicated
background-running indicator remain open. This module does not imply complete
parity for the merged goal/todo/subagent/background status stack.

After this geometry correction, the parent reran the complete default Rust suite:
374 passed, zero failed and one explicitly ignored private-device case.
