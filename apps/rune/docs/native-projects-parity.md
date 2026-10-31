# Native Projects parity: source contracts and verification

The retained Electron baseline defines the project-aware new-chat cwd in
`apps/desktop/src/store/projects.ts` (`resolveNewSessionCwd`): Home detaches,
a selected project's root wins, and All falls back to the configured default.
`apps/desktop/src/store/session.ts` (`workspaceCwdForNewSession` and
`$currentCwdExplicit`) makes that default client-owned and inherited, rather
than a deliberate folder selection. The default directory setting and storage
are in `apps/desktop/src/app/settings/sessions-settings.tsx`
(`DefaultProjectDirSetting`) and `apps/desktop/electron/main.ts`
(`readDefaultProjectDir`). The one-shot path/null target precedence is in
`apps/desktop/src/app/session/hooks/use-session-actions/index.ts`
(`createBackendSessionForSend`). Fresh chat identity rotates in
`apps/desktop/src/store/composer.ts` (`rotateFreshDraftKey`) and is invoked by
`apps/desktop/src/app/session/hooks/use-session-actions/index.ts`.

The focused Linux run passed 12 Projects tests (0 failed, 0 ignored), including
private loopback normal, voice, slash and Bot Chat create dispatch; see
`/tmp/hermes-projects-final-projects.log`. Routing passed 9/9 after correcting
an obsolete same-draft assertion (`/tmp/hermes-projects-routing-rerun.log`).
Selected saved-origin, editor, settings, persistence and window checks also
passed; the full common gate is separate. Native light/dark acceptance,
complete project CRUD and worktree operations remain open. This is not full
parity evidence.

The pinned Electron sidebar persists its show-all-sessions selector
(`apps/desktop/src/store/layout.ts`) and `projectTreePreviewLimit` requests
2,000 rows when enabled, versus 3 by default. Rune now stores the global
preference in its sidebar preferences, exposes the checkbox under Project
grouping, and refreshes the retained, source-bound tree request when it changes.
The project overview now mounts each tree's source-bound preview sessions under
its default-open disclosure. Its per-project Show all action hydrates that
project's lanes inline and reveals rows in pages; the global selector remains a
separate tree request. Native light/dark checkbox acceptance and complete
project parity remain open. The Light/Dark Home and All creation evidence remains
separate from tree hydration and entered-project session evidence.

Show-all focused Linux evidence: `/tmp/hermes-showall-focused-final-20261030.log`
passed 3/3, including a real Viewer action through PageNetwork and a private
loopback peer observing 3 → 2000 → 3 for profile `work`, a rapid-generation
fence, and old-preference reload/merge/reset. The Projects filter passed 14/14
(`/tmp/hermes-showall-projects-20261030.log`) and sidebar persistence passed
8/8 (`/tmp/hermes-showall-persistence-20261030.log`). This does not prove a
NativeSelf 3 → 2000 → 3 sequence, an invalidated queued read for this feature,
or a Rune Runtime checkbox interaction. Existing transport tests and the prior
retained boundary evidence remain separate. The first focused invocation had
wrong relative paths and ran zero tests; it is not evidence.


Preview rendering Linux evidence: the pre-fix Runtime test failed because the
tree preview row was absent (`/tmp/hermes-project-preview-runtime-red-20261030.log`).
The final Projects filter passed 22/22
(`/tmp/hermes-project-preview-projects-final-verified-20261030.log`): the full Sessions
sidebar's Project-only Runtime checkbox drives an authenticated private
PageNetwork fixture through 3 → 2000 → 3, mounts the fourth tree row and
dispatches its `work`/`preview-4` history action; a separate click hydrates
project lanes inline. The suite covers source/generation fencing, Home and
selected-project paths, separate recents, preference visibility and draft
preservation. This is Linux synthetic Runtime evidence, not a new native GPU
or both-theme run. NativeSelf 3 → 2000 → 3 and a feature-specific queued
cancellation have not been proved. The older independent abce native run is a
pre-fix preview RED and is not evidence for this change.

Project rename UI (2026-10-30): explicit projects have a hover-revealed
Actions menu in the overview and entered-project header; right-clicking the
header opens the same Rename… action. The Rune modal starts with the project
label, focuses the input, prevents outside dismissal, trims the submitted name,
and supports Enter, Escape and Cancel. Blank names and pending submissions are
disabled. Failures remain in the dialog. The action captures owner and
selection generation when the menu opens, and the form captures its binding and
scope before dispatch. A retained worker sends `projects.update` with the
profile and typed provenance; current-owner success refreshes the authoritative
tree. Late receipts cannot repaint another source, and uncertain responses are
not automatically replayed.

The pre-fix Runtime menu-path failure is retained in
`/tmp/hermes-project-rename-ui-red-20261030.log`. Focus-return and intermediate
failures are retained in `/tmp/hermes-project-rename-focus-return-20261030.log`
and `/tmp/hermes-project-rename-ui-focused-final-20261030.log`. The final
locked/offline Projects filter passed 21/21 in
`/tmp/hermes-project-rename-ui-final5-20261030.log`, including overview and
scoped menu entry, context click, focus return, blank/Enter/Escape/Cancel,
retained pending write, same-ID source changes and lost response. The fixture
uses a Backend connection. The isolated locked/offline common gate passed
884/884 with 14 ignored in `/tmp/hermes-rename-search-common-final-20261030.log`.
It does not prove NativeSelf rename, native light or
dark rendering, or a new installed binary. Create, appearance, folders, delete,
set-active, Undo and Git worktrees remain open, as does full Projects, audio
and desktop parity.

Project Create source integration (2026-10-30, check-only): the Project sidebar
entry points mount a Create modal backed by a captured owner, selection generation,
source binding and picker identity. The source includes a retained Create receipt,
local multiple-folder and remote single-folder picker routes, a best-effort IDEA.md
writer, all 18 pinned Idea templates and a separate sessionless generation request.
This is an implementation in progress, not evidence of working Create or parity.
The Create model has four registered but unrun tests; the previously registered
structured/malformed directory-listing tests also remain unrun. The vendor patch
adds unrun multiple-folder presenter and parser tests. Offline vendor manifest and
forward/reverse patch applicability checks passed against reconstructed pinned inputs.

The Linux app `cargo check --locked --offline -j 1 --manifest-path apps/rune/Cargo.toml --tests` passed after moving the best-effort IDEA write outside the current-source UI branch, so a confirmed retained receipt attempts the write through its captured binding even after a source switch. This checks app and app test source; it executes no tests and does not establish runtime behavior.

A subsequent static review identified stale remote-picker callbacks, mixed Create/IDEA terminal replies, name edits during generation, navigation after dialog closure, and uncertain-outcome recovery. Source now guards mounted picker actions with the captured scope and picker identity, filters Create terminal replies by operation, bumps the generation revision on name changes, requires the original dialog for foreground navigation, and presents an explicit retry with a duplicate-risk warning. The app `--tests` check passed again after these changes (`/tmp/rune-create-check-review-fixes-20261030.log`); this is typechecking only. At that review point, the retained-mounted-callback and held/interleaved authenticated transport regression definitions had not been completed or run; later source-only definitions are recorded below. The uncertain-outcome recovery remains unverified.

A separate offline Rune package `--tests` check reached the Rune library but was terminated at a sampled 1,390,690,304 bytes available to preserve the requested reserve; it did not complete vendor test-source coverage (`/tmp/rune-create-check-vendor-rune-tests-20261030.log`). No vendor source was changed in this check-only pass. The offline vendor manifest verifier passed.

Focused and common tests, Runtime mounted-callback and two-source
transport cases, headless driver end-to-end cancellation/ordering, native picker,
both-theme visual acceptance and the full Linux gate remain outstanding. In
particular, recheck authoritative entry and refresh sequencing, exact error recovery,
source-bound remote picker and writer, generation edit races and native dialog
lifecycle before asserting behavior. The old 894-pass/14-ignored frozen baseline
is not a result for this post-freeze source. No installed binary was built here.

Source-only regression additions (2026-10-30): `create::integration_tests` now registers
`closed_dialog_retains_create_receipt_without_entering_project`,
`uncertain_create_requires_explicit_retry_and_preserves_payload`, and
`create_dispatches_while_sessionless_generation_is_held`. They define Runtime modal
button dispatch and authenticated private loopback PageNetwork/poll scenarios; form
setup and generation dispatch use controller actions rather than text or Generate
input events. They have not been compiled or run. The held-generation assertion is
expected to expose a source-level gap: submitting does not change the form revision,
so `poll_project_idea` appears able to replace its Idea while Create is pending.
This is not a runtime reproduction. The retry button has a stable Runtime key for
its test. Rustfmt (edition 2024, skip_children) and `git diff --check` exited 0.

Cross-source captured IDEA writes, actual generation input/focus/template
races and retry warning rendering still lack the requested registered integration
tests. A later uncompiled mounted stale remote-picker definition is recorded below. The tests above do not cover those cases, and full Create remains unverified. No Rust compiler, test, native
process, service, or picker was run in this source-only pass.

Source-only receipt regression (2026-10-30): registered
`app::fleet::projects::create::integration_tests::idea_write_terminal_does_not_consume_next_create_receipt`
in `apps/rune/src/app/fleet/projects/create_tests.rs` defines mounted Create A/B
submits, an authenticated loopback RPC and HTTP peer, a held failing IDEA.md write,
and a held B reply. It drives `Viewer::poll_project_create` and `poll_projects`,
checking the captured profile, authoritative A writer path and body, both Create
payloads, B pending after the writer terminal, and B's entered-project and tree
refresh requests and results. It checks for another connection after completion.
This definition is **UNCOMPILED and UNRUN**; its intended coverage is not runtime
evidence. It covers a same-source, closed-A variant, not a cross-source writer
variant. Full Runtime, common, native and cross-source gates remain required.

Narrow SHA-256 fingerprints for this source-only pass: before → after
`create_tests.rs` `d716e096529c82c9f6891da14a5088bb58e9451b03da357e80bd78bc95ca9a9a` →
`cb1fec6cd4d8d6b79110c6b24b69129406678dec2a5257f40c35bbd49835da60`;
`create.rs` unchanged `4a933e2504738a6beb2c52f7c303e1ab186b2a3b8b8ef617e2ed22e2801061a7`;
`desktop-baseline.json` unchanged `2c70e392db36d2e6b567ec47b5949c0900deec96c93ec464c1e88961474e12d4`.
Scoped rustfmt edition 2024 with skip_children and `git diff --check` passed;
these checks do not compile or execute the definitions.

Source-only multi-folder driver regressions (2026-10-30): four registered tests in
`tooling::multifolder_driver_tests` call the actual `headless_configured` and
`desktop::run` loops for ordered selection and cancellation followed by a key.
They also expect the busy-request error to leave the script running. The existing
`file_dialog::multifolder_tests` cover controlled Presenter/Runtime ownership,
drop and reentry. These vendor tests have not been compiled or run; the earlier
app `--tests` check does not check the vendor unit-test harness. Native picker and
platform chooser behavior remain unverified.


Source-only mounted picker regression (2026-10-30): registered
`app::fleet::projects::create::integration_tests::mounted_remote_picker_callbacks_cannot_act_on_reopened_picker`
in `apps/rune/src/app/fleet/projects/create_tests.rs`. It defines a Runtime scene
with remote picker A, closes/reopens Create to B before the next scene commit,
and dispatches A's queued pointer Use, Cancel, parent and child actions or focused
path paste and Enter. It defines both unchanged owner and changed source, profile,
profile intent and epoch cases, with equal path text and distinct dialog/pick IDs;
it checks B's form/path and pending state, then drives Use on a committed B scene.
The fixture constructs the remote picker state without sending its initial listing.
This test is **UNCOMPILED and UNRUN**: callback dispatch, input focus, navigation
request absence and current B success remain unproved until executed. It does not
exercise a real peer, native picker or portal, and full Create, common, native and
macOS/Windows parity remain open. No Rust compiler, tests, native process or
service was run in this pass.


### Verification limits

Linux tests have exercised project creation, local folder selection, and remote
folder workflows. Native chooser and broader platform acceptance remain open;
this work does not establish full Projects parity.
