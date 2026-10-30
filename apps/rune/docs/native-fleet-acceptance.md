# Native fleet acceptance

This page records the current source-qualified fleet integration, not completion
of the full migration. Outstanding requirements are in `native-fleet-parity.md`
and `functional-parity.md`.

## Frozen build

- Project worktree: `/home/ubuntu/Worktrees/hermes-desktop-rune/standalone-desktop`.
- Executable: `/tmp/hermes-rune-goal-nine-candidate-5/hermes-rune`.
- Executable SHA-256:
  `aaf68a8f613fb04034fff00ee5d47e26f0cb80a9eae49a005ce2c514e9983897`.
- Final formatter, locked offline Rust suite and build logs:
  `/tmp/hermes-goal-nine-{fmt,tests,build}-final5.out`.
- Rust result: 607 passed, zero failed, six ignored. The private child-helper
  tests are still run through their default parents; physical audio is excluded.
- Own source/asset/fixture/vendor and Cargo/build input hashes: `/tmp/hermes-goal-nine-source-manifest.json`.

## Actual native fleet

`/tmp/hermes-goal-nine-fleet-native-final5/result.json` passed all three cases
and 23 explicit checks, producing 14 screenshots. The exact self-copied harness
is `harness.mjs` in that evidence directory, SHA-256
`d6165d1ee58ab516a2a78850b2fee74e5b8382d04f6f795f1ac3ae7ca7ae3bfa`.
It drives ordinary native Viewers with a private Xvfb/D-Bus/xfwm session and real
pointer/keyboard input. Each screenshot has its actual frame/window/source/editor
snapshot. There are no runtime state setters, pointer wiggles, live providers,
user agents, physical microphone/speaker operations or user keychain reads.

Two private gateways deliberately reuse profile, stored/runtime and compression
root IDs while returning different installation identities and histories. Their
HTTP/RPC allowlists reject unknown operations. Only B's three explicit
revision/generation/UUID-fenced edits and A's one prompt/one interrupt are
permitted; the fixture itself has no agent execution or provider.

1. **Ownership and streaming.** Pin, Unread and Rename B at rest preserve A's
   actual editor, epoch, text and history. Its menu closes after opening a full
   B session window with the clicked title. A/B same-ID inputs survive source
   return, private persistence and real GTK Ctrl+A/C. A's retained conversation
   receives real streamed events while B is foreground; its presented text is
   checked and photographed on return. A real Settings-to-foreign-B session
   click also opens Chat with its captured history and actual original B
   editor/epoch/draft before A is restored. Stop reaches A once and B never, and the
   fixture persists the partial answer. The whole-fleet transition from 13 to
   14 uses explicit Refresh and retains both themes. Thirteen constructed rail
   choices can be clipped by the horizontal viewport; a real overflow picker
   click proves access instead of claiming all thirteen fit simultaneously.
2. **Hidden rail.** The replacement status picker retains all source-qualified
   choices, explicit refresh and both appearances while the rail stays hidden.
3. **Raw/Saved aliases.** The case waits for the actual Raw and Saved backend
   identities and presented frame, then proves exactly two source zones,
   thirteen choices and one current A profile/root row. Actual current actions
   retain their Raw source; deduplication is display-only.

## Regressions caught before acceptance

Separate failed evidence is retained rather than counted as successful proof:

- Wrong actual source-cache adoption on parking, parked fresh-send binding,
  source-less tab-index callbacks and dropped read/background state were covered
  by actual controller/socket contracts.
- `/tmp/hermes-page-route-intent-{red,green}.out`: internal page-owner reload
  called public navigation, cancelling an explicit pending fleet destination.
  Two contracts went RED then GREEN for Settings/Cron before acknowledgement
  and preserved explicit user cancellation. The native ownership case then
  failed on candidate4 with an empty Settings workspace, and passed unchanged
  on candidate5 with B's actual Chat/history/editor. Exact native RED is in
  `/tmp/hermes-goal-nine-settings-fleet-native-red4`.
- `/tmp/hermes-page-owner-ready-{red-clean,green-final}.out`: two actual
  loopback contracts prove no page reads before a correlated source/profile
  receipt or after exact rejection, then same-owner initialization and accepted
  history-error recovery. The initial fixture abort and premature async oracle
  are separate failed setup, not counted as the clean RED.
- `/tmp/hermes-inventory-process-tree-{red,green}.out`: successful SSH leader
  exit, cancellation and deadline all hung on a helper-held stderr pipe. All
  three contracts went RED then GREEN; captured pidfds and a private subreaper
  prove helper exit/reaping. Owned groups plus nonblocking pipes replace
  reader-thread joins; last-UI shutdown still passes. These are Linux proofs,
  not macOS execution or Windows Job-object acceptance.
- `/tmp/hermes-fleet-request-origin-{red,green}.out`: actual pending loopback POST
  and controlled SSH tunnel ownership. A lease alone lost its actor when other
  handles dropped; the request now keeps the original Resolver to its response.
  Explicit registry revision retirement still invalidates it.
- `/tmp/hermes-fleet-profile-pick-{red,green}.out`: public rail picks wrongly
  opened canonical Bot Chat. Three contracts were RED, then all 13 app fleet
  contracts passed with current-profile no-op and changed/All fresh semantics.
- `/tmp/hermes-live-session-read-{red,green}.out`: exact `session.list` read was
  blocked by the native workspace allowlist, and a retired queued live read was
  treated as a write. Both contracts now pass; unrelated session mutations stay
  rejected and captured profile wins over body-provided profile.
- `/tmp/hermes-fleet-alias-entry-{red,green}.out`: the real rendered entry treated
  the exact active version as replaced, leaving a phantom Raw group. The entry
  contract went RED then nine view contracts passed; a changed Saved revision
  remains fenced.
- Native runs `/tmp/hermes-goal-nine-fleet-native-{1,2,3,4}` retain exact scripts,
  frames and diagnostics. Production findings were the blocked live read,
  foreign menu left open, discarded peer title and phantom alias group. Harness
  findings were missing actual scroll/reveal and an identity/frame readiness
  race; the final harness waits for the real evidence, with no waived ownership
  or mutation assertions.

The same executable passed all sixteen startup/default native cases with
twenty-eight PNGs in `/tmp/hermes-goal-nine-startup-native-final5/result.json`.
The exact copied startup harness SHA-256 is
`54c640f2e3f3c665bcc6044da85c1eadd517a8afc7dc0f618ff56414e8aca147`.
The complete peer-window flow also passed with strict exact GTK clipboard
assertions, five lifetime XIDs in one process (four coexisting), twelve PNGs and
normal local Close/global Quit. Its record is
`/tmp/hermes-goal-nine-peer-native-final5/result.json`, copied harness SHA-256
`2794b6254dceb060da6bb848dd42113c7d0d0c4924d549cb5a77be6a503e63bf`.

The same binary passed the rich read-only subagent miniwindow regression with
actual Copy, mislabeled SVG, scoped original image read, file/tool output,
reuse/focus/theme and detach-only close. Three PNGs and the exact binary/harness
hashes, private environment/command and parsed result are in
`/tmp/hermes-goal-nine-subagent-native-final5/run.json`; canonical harness SHA-256
`811b6c63976d933747ca2b5d62c0467a2fd1b0a06754ed0068670cb7c5be6f73`. These four native lanes produced 57 PNGs.

The same executable passed all 97 renderer/script-input cases: editors 5,
composer 19, client settings 15, shell 10, feature pages 24, rich chat 13 and
full Viewer audio navigation 11. Exact commands, logs, individual harness hashes
and screenshots are indexed in
`/tmp/hermes-goal-nine-regressions97-final5/result.json`. The first six unchanged
lanes supply 86 cases. The old audio script used a source-less row key and
failed before its first behavior flow; its exact bytes/failure are retained in
`/tmp/hermes-goal-nine-audio-navigation-oldkey-failed5`. The updated script
first captures the actual typed source/profile/lineage row from one uncounted
ordinary Viewer render, then passes the original eleven owner/selection/return/
replacement/retirement/no-alert cases without changing the binary or assertions.
Its twelve renders and exact executed harness/command are recorded separately
in `/tmp/hermes-goal-nine-audio-navigation-final5/run.json`. No Play, physical
audio, provider or installed agent is involved.

The durable ninth-checkpoint evidence archive is
`/home/ubuntu/Artifacts/hermes-rune-ninth-checkpoint-2026-10-30.zip`. Its manifest
records the source commit, tested executable and every archived file hash;
pre-fix failures remain separate from the successful final lanes. It is an
evidence archive, not an application distribution. No cross-platform/distribution acceptance
is inferred from this private Linux evidence.
