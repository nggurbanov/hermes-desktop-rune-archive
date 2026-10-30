# Complete native peer-window acceptance

`scripts/check-peer-windows.mjs` drives the ordinary Hermes application against
strict private loopback gateways. It starts a private Xvfb display, private D-Bus
session and xfwm4 window manager; it never uses the user's display/session bus.
The application uses isolated client/agent-home paths, muted cue preferences,
null ALSA and a nonexistent Pulse endpoint. No microphone, voice action, physical
output, provider or installed agent belongs to this test.

Run from the repository root:

```sh
node apps/rune/scripts/check-peer-windows.mjs --binary /absolute/hermes-rune --out /absolute/evidence
```

Linux prerequisites are Xvfb, xfwm4, D-Bus, xdotool, xprop, xwininfo and the system Python
Gtk/Gdk bindings. The script owns only its spawned processes and temporary home.
It closes the application using real normal commands; process termination is
cleanup on failure and cannot establish accepted close/quit behavior.

The opt-in `--peer-window-proof-dir` inspector is a read-only observation seam in
the actual production renderer. It accepts only a numeric loopback source and an
absolute output directory, is not persisted, and follows the in-memory workspace
to its peers. Actual native typing and the real private persistence files
independently verify editor identity/input retention. The inspector does not
replace any UI, connection, source, conversation worker or send/stop action.

The harness requires all of these behaviors before writing a success result:

- Two real top-level native windows in one process, with ordinary full Hermes
  sidebars, composer and Settings navigation.
- Different live and persisted editor identities, independent typed drafts,
  persisted window-root/editor-slot ownership and distinct live source epochs.
- Two actual interactive conversation workers against the fake peer, simultaneous
  streaming and Stop addressed only to its own runtime/profile.
- Changing the primary source/profile does not reassign an existing peer. A later
  peer inherits the source/profile captured for its own opening.
- Opening a stored session through its context menu loads the captured history;
  opening it again focuses that qualified existing window rather than duplicating it.
- A Bot Chat context action creates a profile-pinned window, resolves its named
  registry chat and retains its source/profile for a later generic New Session.
- Actual light/dark UI interaction, local window close, a surviving interactive
  peer after primary close, and application-wide Quit.

The fake gateways reject every HTTP mutation and any RPC outside their exact
read/session-create/prompt-submit/session-interrupt allowlist. Prompts contain
only the three explicit synthetic acceptance strings. The result records the
binary SHA-256 and PNG hashes alongside exact HTTP/RPC requests and private
window metadata. Renderer bounds do not substitute for native window-manager
interaction; this harness performs both.

`--clipboard-diagnostic` additionally performs native Ctrl+A/C from each input
and asserts every exact synthetic string returned by the independent GTK
clipboard reader. In the ninth integration a mismatch fails the run; the older
seventh-checkpoint behavior below only recorded the diagnostic. These checks do
not replace the mandatory persisted ownership/input assertions. A failed copy
remains failed evidence and cannot be reported as keyboard clipboard acceptance.

## Seventh checkpoint

Linux native acceptance passed on 2026-10-30 against the frozen binary
`/tmp/hermes-rune-goal-seven-acceptance/hermes-rune`, SHA-256
`3611c41cb30d625e95adfce43ae7e6032de6ed54b4f4f65282790b30fd8d095d`.
The complete final run is
`/tmp/hermes-rune-goal-seven-peer-native-visual/result.json`; its sibling files
contain exact requests, persisted ownership, five distinct native XIDs in one
PID, window metadata and twelve PNGs. Four native windows coexist at the peak.
Settings was visually reviewed both before exposure, after a three-second
software-render settle, and after an actual one-pixel native resize. The final
run exits through the ordinary application-wide Quit command with code 0.

The optional clipboard diagnostic did **not** pass in that full run: GTK returned
empty text for both attempted copies. Independent typed inputs and their real
persisted ownership did pass. This result does not establish native keyboard
clipboard parity. The failed copy diagnostic is retained in
`clipboard-diagnostic.json`; a separate minimal native Rune/Kit copy probe is
not a substitute for the complete Hermes composer.

Framework lifecycle stress/collision tests, macOS/Windows interactions,
device-default routing, live appearance observation and whole-feature parity
remain independently tracked requirements. This acceptance creates ordinary
complete Hermes Viewers, but does not claim the separate greater-than-64 window
lifetime stress contract or exercise any live provider, microphone or audio output.

## X11 clipboard correction after the seventh checkpoint

A subsequent private full-Viewer regression established the cause of the failed
copy diagnostic: winit's synthetic X11 focus/entry motion left its cached cursor
position stale. A returning physical motion to that cached coordinate could be
suppressed, leaving Rune to hit-test the next click at the wrong point. Actual
runtime and X11 pointer observations disagreed and the input lost focus.

The exact unpatched application
`95aab82ac646f3f1834b75f8f577a556942ada79c2ae063a62468bec60529af4`
failed six of six ordinary two-window Ctrl+A/C copies. Synchronizing the existing
winit cursor cache before that synthetic motion fixed all six copies, with exact
external GTK clipboard strings and matching OS/runtime coordinates. No pointer
wiggle, altered editor, extra motion or delay workaround belongs to the accepted
regression. See `vendor/winit-0.30.13/HERMES_PATCH.md` for the exact source archive,
Apache-2.0 notice, patch, hashes and offline verifier.

The candidate application SHA-256
`5f99ea73ab734ce4796106db0ff086af8c422d962ab3862d0fdedb073adb6eb9`
passed both the six-copy regression and the complete ordinary native peer flow,
including the two-copy diagnostic and normal application Quit0. Evidence:
`/tmp/hermes-full-copy-regression-{red,green}/clipboard-loop.json` and
`/tmp/hermes-winit-full-peer-green/result.json`. This candidate result is kept
separate from later integration binaries and from macOS/Windows acceptance.

## Eighth integrated acceptance

The final integrated binary
`/tmp/hermes-rune-goal-eight-final/hermes-rune`, SHA-256
`1407f2ff28abfa14e3272235a36ede4b972beb88d83d49be21d025a2da39365a`,
passed the complete ordinary peer-window flow and its native clipboard diagnostic.
Evidence: `/tmp/hermes-rune-goal-eight-peer-native/result.json`, with twelve
native PNGs, actual native bindings and exact requests. Every required result
flag is true, including independent typed/persisted inputs, settled Settings,
concurrent streams/scoped Stop, captured source/profile, stored session reuse,
profile-pinned New Session, the interactive peer surviving primary close,
local Close and normal global Quit. The clipboard result is true.

The separate ordinary two-window, three-round copy regression also passed on
that same binary: all six exact external GTK clipboard reads and pointer checks.
It uses direct normal clicks at returning window-local coordinates and no pointer
wiggle. Evidence: `/tmp/hermes-rune-goal-eight-copy-native/clipboard-loop.json`.
The older failed diagnostics and candidate hashes above remain historical proof;
this final integrated result supersedes their current unverified clipboard status.

The same binary's independent sixteen-case startup/default acceptance is recorded
in `native-startup-route-acceptance.md`. These proofs use private synthetic peers
and private X11/D-Bus/WM sessions. They establish no live provider, physical audio,
macOS/Windows, dirty-global-Quit, full topology/fleet or final SSH-service shutdown
acceptance.

## Ninth checkpoint regression

The frozen candidate5, SHA-256
`aaf68a8f613fb04034fff00ee5d47e26f0cb80a9eae49a005ce2c514e9983897`,
passed the complete ordinary native flow again, including every strict
`--clipboard-diagnostic` assertion. Evidence is
`/tmp/hermes-goal-nine-peer-native-final5/result.json`, exact copied harness
SHA-256 `2794b6254dceb060da6bb848dd42113c7d0d0c4924d549cb5a77be6a503e63bf`,
and its sibling request/frame/clipboard/PNG records. The source-qualified fleet
integration preserves the original five lifetime XIDs in one process, up to four
coexisting windows, twelve PNGs, independent real persisted inputs, captured
source/profile/session and Bot targets, simultaneous streams and scoped Stop,
normal local Close/global Quit and an interactive survivor after primary close.
No pointer wiggle, physical audio, provider or user agent is involved.

The existing fake peer now supplies the exact passive GET `/api/profiles` read;
other unknown operations and HTTP mutations still fail. Actual native geometry
helpers reveal source-qualified condensed rows and wait for stable presented
bounds before pressing; editor, source, cancellation, persistence and window
lifetime assertions remain mandatory. This Linux proof does not establish
complete workspace restoration, tiles or native macOS/Windows distribution.
