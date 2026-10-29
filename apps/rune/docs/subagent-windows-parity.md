# Native subagent windows

The retained Hermes desktop opens a child stored session with `watch: true`.
`primaryResumeParams` sends `session.resume` with `lazy: true` and full history,
so observing a child never builds an idle agent or admits a new turn. Its window
registry reuses an existing child window instead of duplicating it.

The Rune client now retains a roster per connection epoch, profile and parent
stored conversation. Accepted `subagent.*` events update it; an owner-bound
`subagent.list` read hydrates it and refreshes every five seconds while the chat
is visible and parent or child work is running. Settled sessions stop polling
after hydration or their conclusion readback. Newer live events beat an in-flight roster read. Terminal identities
remain retired across new-turn pruning, so late frames cannot revive old work.
Older gateways with no roster RPC retain their event-fed rows; reads stop after
three failures.

Selecting a child opens a separate native Rune window. The pinned Rune revision
only registers in-process windows before starting its event loop, so dynamic
child windows run as native Rune processes. A private stdio launch descriptor
carries the immutable connection and child identity. No token, URL, agent home or
transcript is written to command-line arguments or a launch file. The parent
registry keys reuse/focus by connection epoch, profile and child stored ID.
Theme changes propagate over that private control pipe. Window creation is
acknowledged; startup failures surface in the parent.

The child attaches using a dedicated read-only `SessionTransport::watch` method.
It reuses the main conversation's event projection for live deltas, reasoning,
tools, final response, UTF-16 offsets, replay epoch and authority fences. A
reconnect replays a contiguous canonical tail when possible, otherwise it reads
an authoritative lazy snapshot. Failed attachment never creates a session or
falls back to a different child. Retry keeps the same immutable owner.

There is no composer, prompt submission, approval answer or stop operation in
the spectator. Closing its window or retiring the source/profile only closes
its viewer process and socket; the child and parent agent keep running.

Proof uses synthetic sockets and actual native renders only. Native scene:
`hermes-rune --subagent-scene --theme dark --size 560x800 --snap child.png`.
The scene is explicitly synthetic and performs no network access. Six focused roster/socket contracts passed, including a real lazy attach,
streaming final response, child identity refusal, retirement races, and idle
polling shutdown. `xvfb-run -a node apps/rune/scripts/check-subagent-windows.mjs`
passed with an actual native child window and production window registry. The
controlled peer received exactly one `session.resume`; reopening reused the
same live PID/window, focus and theme receipts arrived, and profile retirement
closed the viewer socket without sending any gateway action. Both native
light/dark captures were inspected.

Remaining rich-chat parity applies to both main and child transcripts: child
Markdown, reasoning and tool text are native, but the richer attachment/action
surface must continue to share the main chat's rendering work as that port is
completed. Cross-platform window-manager acceptance remains to be run on macOS
and Windows; Linux proof does not establish those hosts.
