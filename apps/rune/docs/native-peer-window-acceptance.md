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

`--clipboard-diagnostic` additionally attempts native Ctrl+A/C from each input
and records the exact synthetic strings and its independent result. It does not
replace the mandatory persisted ownership/input assertions. A failed clipboard
diagnostic must not be reported as passed keyboard clipboard behavior.

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
