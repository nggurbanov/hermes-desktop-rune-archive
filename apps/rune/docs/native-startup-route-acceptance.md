# Native startup and default-route acceptance

The harness is `scripts/check-startup-routing.mjs`. It launches the ordinary
complete Viewer against two private, anonymous numeric-loopback HTTP/WebSocket
fixtures in a private Xvfb, D-Bus session and xfwm window manager. Every case has
its own HOME, state, config, data and cache directories. Saved connection rows
use fixed UUID identities and no credential references, so these cases do not
access the OS keyring. Audio output is disabled and no microphone or physical
audio device is used.

Run against an immutable compiled binary, with a fresh evidence directory:

```sh
node apps/rune/scripts/check-startup-routing.mjs \
  --binary /absolute/path/to/immutable/hermes-rune \
  --out /tmp/hermes-startup-native-evidence
```

Use `--case <name>` to run a single case. The source currently defines:

- `default-precedence`: B/research wins over primary A and B's remembered archive
  profile, including last-used mode.
- `primary-policy`, `last-used-policy`, `missing-last-used`: no-default boot
  selection and the defined missing-last-used fallback.
- `window-default-intents`: device-default observation preserves the current
  draft; profile context actions save/clear an exact source/profile without
  selecting it; ordinary peers inherit only for boot; later generic New Session
  follows the device default; a ProfilePinned Bot window retains its own route.
  A delayed return to the pin is held under the real registry OS lock, then an
  already-current profile click cancels it without moving source or editor; a
  subsequent New Session still honors the immutable pin. Explicit session/profile
  actions keep their current source. Old typed input is
  checked in its exact source's durable draft file.
- `profile-memory-hooks`: native Bot opening, reopening a stored tab across
  profiles and a profile-only rail pick update the confirmed profile for A.
  Another window's last-used B and the separate device default remain unchanged.
- `pending-new-input`: a private target validation response is held while the
  actual outgoing composer receives newer text. Releasing it cannot navigate,
  change last-used or lose the new input; a fresh explicit New Session then works.
- `single-source-controls`: primary and startup controls remain present with one
  saved source, mode writes stay separate from the current workspace, and the
  native Clear Default action persists.
- `policy-write-error`: actual Make Primary and startup-mode clicks reach the
  bounded contention error while a private second process holds the real registry
  OS lock. They retain file bytes, preferences and the live source; releasing the
  guard permits an explicit policy write without navigating. Invalid-registry
  retirement remains covered by the registry contracts rather than requiring a
  live source to survive unreadable ownership metadata.
- `compact-profile-light`, `compact-profile-dark`: the condensed picker retains
  default and named-profile doors and their context actions in both appearances.
- `invalid-default`, `future-default`: fresh invalid/unsupported device defaults
  do not fall back to either gateway, retain the exact file and show a real
  native window. When loopback gating correctly leaves the inspector disabled,
  the evidence contains a native window capture and a zero-request proof.
- `missing-default-profile`, `profile-disappears-before-connect`: only the named
  source may be validated; absent profiles and a profile removed between the
  successful resolver/commit probes and the final Viewer connection cannot fall
  back to another source, profile or session. Failed final connections cannot
  continue to Viewer session RPCs; already scheduled background reads may remain
  explicitly scoped to the requested profile.
- `default-write-error`: a native Set Default action rejects a future-schema
  preference file while preserving its bytes, source, profile and entered text.
  Repair retries that exact user intent without navigating, proving the native
  click actually reached the failed write path. The matching startup-owned
  error banner must also clear after successful repair.

Input uses real X11 focus and pointer events. Each click moves directly to the
actual rendered target and holds the button for 40 ms. The harness compares
Rune's presented Runtime pointer with the X11 pointer before pressing. It does
not move away and back to work around a cursor-cache defect. These checks require
the read-only loopback proof's complete keyed element geometry from
`on_frame_drawn`; cached pre-layout rectangles are not used as click targets.

Both fixtures reject unexpected HTTP paths, all HTTP mutations and unlisted RPC
methods. There is no `session.create`, `prompt.submit`, interrupt, provider request,
agent configuration change or running external agent. Existing history attachment
is read-only. The fake gateways validate source/profile/session ownership.

The evidence records the immutable binary SHA-256, every fixture request, native
XID/PID bindings, input gestures, screenshots and the private state files. Normal
Quit is part of acceptance; owned-process termination is cleanup only. A failed
case keeps its log and captures. The private HOME directories are removed after
resources have stopped.

This harness establishes saved remote-route behavior and the existing condensed
current-source picker. It does not establish legacy per-profile remote overrides,
full cross-source fleet enumeration/status/order, Cloud/OAuth or macOS/Windows
native acceptance; those remain separate ledger items.

Pre-fix native regression evidence: `pending-new-input` failed on immutable Linux
binary `5f99ea73ab734ce4796106db0ff086af8c422d962ab3862d0fdedb073adb6eb9`.
After actual newer text was entered in A while B's profile validation was held,
releasing B navigated to B/archive and another editor. Pointer/focus assertions
had passed. Evidence: `/tmp/hermes-startup-pending-input-5f99-red/`.
The later same-owner pin-cancellation extension failed on predecessor
`fc70f73f6ed54680a134e87aca8ddf525d062b3502f2e8f02d300529033ea274`: after
explicit B/archive and a held return to pin A, clicking already-current archive
still let the delayed result navigate to A/research. Actual native hit checks
passed. Evidence: `/tmp/hermes-startup-pin-noop-fc70-red/`.

Verified on immutable Linux binary
`1407f2ff28abfa14e3272235a36ede4b972beb88d83d49be21d025a2da39365a`:
all 16 native cases passed, including both regressions above. The harness SHA-256
was `62fe13fed7cc71842347b28c11f4b58f486750ffd05fdeba74ab97167eca5791`.
Evidence is `/tmp/hermes-startup-routing-1407-proof/result.json`, with 28 native
screenshots and full request and pointer logs alongside it. Light and dark menus,
the delayed pin-cancellation result and recoverable error states were visually
reviewed. All cases exited through normal Quit; the run made zero agent mutations
or provider requests and used no physical audio or live agents. This acceptance
has the scope and platform limits stated above.
