# Native subagent windows

The retained Hermes desktop opens a child stored session with `watch: true`.
`primaryResumeParams` sends `session.resume` with `lazy: true` and full history,
so observing a child never builds an idle agent or admits a new turn. Its window
registry reuses an existing child window instead of duplicating it.

The Rune client now retains a roster per connection epoch, profile and parent
stored conversation. Accepted `subagent.*` events update it; an owner-bound
`subagent.list` read hydrates it and refreshes every five seconds while the chat
is visible and parent or child work is running. Settled sessions stop polling
after hydration or their conclusion readback. Newer live events beat an in-flight
roster read. Terminal identities
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

A lagging window retains exactly one latest display projection. Cosmetic updates
replace that mailbox entry, while terminal and error snapshots retain their final
messages and status. Closing the viewer stops publication. This bounds the number
of full transcript/image carriers retained by the display queue. Producing a new
snapshot still clones the current display data; reducing that CPU cost awaits the
shared immutable transcript-model work and is not claimed here.

The child attaches using a dedicated read-only `SessionTransport::watch` method.
It reuses the main conversation's event projection for live deltas, reasoning,
tools, final response, UTF-16 offsets, replay epoch and authority fences. A
reconnect replays a contiguous canonical tail when possible, otherwise it reads
an authoritative lazy snapshot. Failed attachment never creates a session or
falls back to a different child. Retry keeps the same immutable owner.

There is no composer, prompt submission, approval answer or agent interrupt in
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

The child transcript now consumes the same `chat::projection` parser as the main
chat for durable text, images, files, reasoning and tool results. Live activities
use the same tool projection. Copy writes displayed text to the native clipboard;
manual Read aloud and Stop reading use the existing audio player and never
interrupt the agent. These are client actions, not new child turns.

Image previews also share the main chat's authenticated loader. Each transcript
keeps its latest 24 image sources, with an explicitly selected older image pinned;
the cache holds at most 64 entries globally and runs at most four fetches in
parallel. Request identities reject evicted, retried or foreign-owner results.
Raster decoding enforces 8192-pixel dimensions and a 64 MiB allocation limit,
then normalizes to at most 1000 pixels per side before entering Rune's cache.
Compressed inline data is limited to 32 MiB. Oversized previews leave the original
Download action available. SVG previews use Rune's bounded 1000-pixel rasterizer.
Older images can be loaded explicitly, and failed previews can be retried.

Each attachment offers explicit Open and Download. Agent paths use the existing
authenticated filesystem download with the frozen profile and child session;
HTTP(S) sources go through the owning gateway's media proxy. An Open action
first obtains a private native copy and then opens that copy. A Download action
writes only the user-selected absolute destination. No agent path is treated as
a path on the client, and fetching an image or receiving a message never opens
an OS application. Closing the window cleans its private open copies.

The richer native lane is
`xvfb-run -a node apps/rune/scripts/check-subagent-windows.mjs --rich-media`.
It supplies a controlled image HTTP response and real child image/file/tool
parts, waits for decoded pixels, clicks the native Copy control and verifies the
X11 clipboard, then clicks a tool disclosure and checks its expanded state. It
retains the same lazy/reuse/focus/theme/detach checks. Screenshots are direct captures of the actual 560×800 native
window. The final rich native lane passed and all three light/dark/tool-output
captures were inspected. Ten child roster/socket/file/mailbox contracts and four
shared image-loader contracts passed, including the valid highly compressed
8192² PNG refusal before allocating its 192 MiB decoded image. Tests also exercise authenticated owner-scoped image and file reads,
private explicit-open copies, stale image replies and preview resource limits.

Remaining platform acceptance must be run on macOS and Windows; Linux proof
does not establish those hosts. Static image previews and explicit native media
open are available; inline animated image/video/PDF playback and full child
window chrome parity remain part of the wider desktop migration. Provider-backed
manual TTS is reused from the audio feature but not exercised against a live
provider in this child-window acceptance lane.
