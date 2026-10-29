# Rune and OpenCode provenance

## Dependencies

`rune`, `rune-kit`, `rune-theme-opencode`: anomalyco/rune at
`bfb8facae2e4665e5e6d17afe85561884a7fd402`, MIT, copyright Christopher Langton.
The lockfile pins the dependency graph. The renderer, text editing, layout,
virtualization, Markdown, widgets, theme, fonts and icons are real dependencies,
not approximate replacement implementations.

## Adapted reference sources

`examples/opencode` was removed upstream by
`d1634c68a4eeb1ca0d1787df266547a1ab3eccea`. Its last containing repository tree is
`3f70cd6b675c89a8a7b64fc09b1bf2b455e0ad1a`.

`src/view/palette.rs` and `assets/opencode-theme.json` reuse its palette source
and compact theme data. `src/view/transcript.rs` adapts its Markdown construction,
virtualized/keyed timeline, user bubble and composer frame. The original
`view.rs` also supplies patterns for identity/avatar rendering. The source
modules record these origins; the original MIT license is in `licenses`.

OpenCode's REST/SSE transport, Basic auth, auto-discovered local server, silent
synthetic fallback, session-ID-only ownership, Beta drafts.sqlite storage and
fixture-only live pickers are not reused as Hermes domain behavior. The Hermes
adapter speaks its own profile-scoped JSON-RPC and read-only history HTTP APIs.
No prompt or session mutation is part of this increment.

## Fonts

Inter and IBM Plex Mono are embedded by the upstream theme dependency. Their
SIL OFL license notices are retained in `licenses/Inter-OFL.txt` and
`licenses/IBMPlexMono-OFL.txt`. Preserve these notices when distributing a binary.

## Reuse accounting

The reference binary contains about 3,199 lines of Rust, including comments and
tests. `view.rs` and `palette.rs` contain 1,324 of those lines and are a strong
presentation reuse pool; that is not a measure of the much larger Hermes app.
The shared Rune/kit/theme code is reused by dependency. A claim that 70% of the
entire Hermes application has been ported or reused would not be supported.
