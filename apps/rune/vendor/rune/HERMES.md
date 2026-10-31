# Hermes native-window patch

All upstream runtime, kit, motion, Mermaid, staging and OpenCode theme sources
come from the same exact revision recorded in `HERMES_PATCH.json`. The snapshot
includes their workspace members, tests, examples, fonts and original licenses.
The manifest records each original file and every modified/added source hash.
`python3 scripts/verify-hermes-source.py` verifies it offline. `HERMES.patch` is
a zero-context source patch for that exact baseline; apply with
`git apply --unidiff-zero` only after checking the recorded original hashes.
No build modifies
Cargo's global checkout or downloads a replacement framework source.

## Public additions

The off-by-default `test-presenter` feature exposes the actual per-window file-dialog Presenter to integration tests with a controlled future. Production native dialog backends are unchanged.

`Tx::open_folders` requests ordered multiple-directory selections; scripted
headless dialogs accept `file-dialogs` paths and `file-dialog-cancel`.


`markdown::link_occurrences_cached` exposes authored parser link source byte
ranges, destinations and rendered inline label ranges from the same cached
parse used by Markdown. Bare URLs linkified separately are not parser events.

`Markdown::embed_links` optionally replaces authored link labels with freshly built
`El` children in paragraphs and headings (table cells retain text rendering).
Each replacement occupies its own row between styled text
fragments within the keyed paragraph; this is not inline typography equivalence.
Unreplaced links retain their handlers. Source offsets key the embedded children.
Empty labels can be replaced; source-copy falls back to drawn text for selected
blocks containing embedded elements so it cannot copy their hidden source.
The library test fixtures initialize the embedded-block accumulator empty, matching
the production renderer constructor.

`El::on_mount_change` runs after a committed node enters or leaves the mounted
model tree, including virtual-list culling. It does not treat an offscreen node
in overscan as removed. Returning true schedules a subsequent view commit;
handlers should only schedule work and never block the UI thread. Hermes uses
this to own independent transcript audio players.

`Tx::scroll_highlight_into_view_all_in` reveals a mounted, scoped text highlight
through its vertical ancestor viewports. Existing nearest-viewport reveal is unchanged.

`El::on_input_display_change` is opt-in and reports visible input text changes,
including transient IME composition. The existing committed-text callback keeps
its original semantics. Hermes uses it for search over the mounted editor.

`Tx::spawn_window(WindowSpec<S>) -> Result<SpawnedWindow<S>, WindowSpawnError>`
queues an event-thread local specification. The returned object exposes `app`,
`window` and `creation`; `creation.status()` reports Queued, Registered, Created
or Failed(error). Registered means a host exists; a windowless app opens later.
Created means native creation ran. `window.is_ready()` additionally checks the
first presented frame, and `app.window_error()` reports later native failures.

A state may contain `Rc` or other non-Send values. A background caller uses the
existing `AppHandle::update` to build the specification on the event thread.
Queue limits are backpressure for unprocessed creation requests, not a fixed
pool or a lifetime window limit. Requests are consumed only once across cloned
transactions, propagate through frame actions, and report an error when their
owning runtime disappears or cannot host native windows. Single-runtime
headless tooling explicitly reports unsupported. Native requests default to no
inherited command-line script/scene; explicitly offscreen child arguments are
rejected rather than counted as a native window.

Each ordinary dynamic host retains the event loop independently. Closing its
opener does not close the peer. Closing a peer drops its state, menu/shortcut
guards and native resources. A live-host map uses monotonically unique routing
identities: closed IDs are removed and are never rebound to another state,
including when a new window reuses the old public key. Keys must be nonempty
and unique among live hosts. All guards in a window group are attached before
any native creation callback runs. The existing GPU weak pool remains shared.

`WindowHandle::geometry()` snapshots physical outer position when available,
physical inner size, scale, maximization and current monitor bounds. Winit does
not provide reserved work-area bounds; Wayland generally does not expose outer
position. These fields must not be presented as a portable Dock/taskbar work-area
or placement guarantee.

`App::on_close_requested(fn(&mut S, &mut Tx) -> bool)` (also a WindowSpec builder)
runs before a local close and its existing `on_close` hook. False retains the
window and commits guard UI changes. Explicit global Quit and OS termination
retain upstream's non-veto behavior. An application-wide dirty-window Quit
preflight remains separate work; this patch does not claim to provide one.

## Local acceptance

The focused `hermes_dynamic_windows` public integration contracts run against the
production library with `CARGO_PROFILE_TEST_DEBUG=0` to omit debug symbols without
changing debug assertions or behavior. They exercise actual Runtime commits,
non-Send inputs, consumed-once transactions, bounded queues, dropped requests,
headless errors and absent native geometry. The synthetic native example
`hermes_dynamic_windows` uses real native hosts and explicit creation/first-frame
readiness. Its checker requires a private Xvfb display and xdotool. It verifies
more than 64 create/close cycles, duplicate-live-key rejection, stale-route
isolation, geometry, a local close veto, a peer surviving its opener, creation
from that surviving peer and global Quit. The example uses no agent, provider,
audio device or user state. Evidence is produced in a caller-selected private
directory. The separate windowless failure mode uses an actual process with no
Vulkan ICD: the native surface creation fails, its error is returned and the
initialized child state is dropped before Quit. There is no injected fake host
or native-error test API. Native Linux results do not establish macOS or Windows
acceptance.

The pinned upstream tooltip test invokes a macOS-only native-help method. Its
existing test now has the matching macOS cfg so Linux can compile the library
test harness; its assertions and macOS behavior remain unchanged.


Linux acceptance on 2026-10-29 passed all five public integration contracts and
the private native lifecycle checker with 80 close/recreate cycles. The latter
confirmed two real initial windows, a rejected live duplicate, old-route isolation,
physical geometry, a local veto preceding the one accepted close hook, a peer
surviving its opener, a new window from that surviving peer, and global Quit.
The no-ICD surface failure case also passed with initialized state released.
The native example binary SHA-256 was
`479c30fefcde36cad3da6a0ca269ffbc20428630c97802d8ee26271708574c37`.
The public integration target compiles the production library, rather than the
entire unrelated upstream library-unit-test harness.

Two real native failures found and fixed during this work were an action Tx
merge dropping the new request and a timer-driven close leaving stale published
visibility. These are covered by the Runtime poll contract and native opener-close
sequence respectively. Initial native creation failure also retires its owned
hosts, so an invisible failed peer cannot retain its state or the event loop.
The complete Hermes window integration has its own separate acceptance suite.
