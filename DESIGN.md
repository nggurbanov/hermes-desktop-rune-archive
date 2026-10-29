# Rune design direction

## Source and scope

User-confirmed on 2026-10-29: preserve Hermes element placement, navigation and
logic; use a substantially reused OpenCode Rune visual system. This document
applies to the native Rune port. apps/desktop/DESIGN.md documents the Electron
baseline and remains authoritative for behaviors being ported.

## Physical context and appearance

The same person reads and resumes long conversations at a desk in daylight and
returns in a dim room in the evening. Support OpenCode's real light and dark
appearances and follow the OS preference unless explicitly selected. Do not
choose dark merely because this is an agent tool.

## Color strategy

Restrained: semantic neutral surfaces, quiet hairlines, accent for active intent
and status. Consume rune-theme-opencode's palette and rune-kit tokens directly.
No independently guessed hex palette, gratuitous gradients, glass, nested cards
or colorful terminal-like chrome. App-specific color decisions use semantic
roles; the reference theme owns concrete color conversion and values.

## Typography

Reuse OpenCode's bundled Inter variable family and IBM Plex Mono for code.
Use reference component type roles for controls and metadata, and comfortable
prose leading and a bounded reading measure. Typography clarifies transcript,
identity and secondary metadata without oversized dashboard headings.

## Layout

Preserve Hermes's default Advanced topology: sessions left, workspace center,
Review and Files in the upper-right, Terminal below. Sessions width starts at
237 logical pixels. The existing six presets, nested splits, tab stacks, focus,
resize and keep-alive belong to eventual parity, not an invented fixed layout.
Reference styling does not authorize moving actions or replacing Hermes's tree
with OpenCode's simpler shell. On small windows retain ownership and provide
usable overflow/collapsed panes rather than hide active conversation state.

## Components and surfaces

Prefer real rune-kit buttons, inputs, pickers, Markdown, scroll/virtual rows and
split primitives. Adapt original OpenCode transcript, tool-group, picker and
composer view functions where their assumptions fit Hermes. Preserve Hermes
branding. Empty/loading/error/reconnecting/read-only states are distinct; an
unavailable feature must not appear to execute through a dummy handler.

## Interaction and motion

Use reference hover, focus, selection and motion tokens. No decorative loading
choreography. Reduced motion settles directly. Keyboard shortcuts follow focus,
Escape handles only the topmost active interaction, and background updates never
switch conversations. Networking begins after the first frame and only wakes
rendering when visible state changes. Long lists are virtualized.

## Confirmed first increment

Production native Rune code, real external connection, profiles, session list,
and readable history. Loading, empty history, network/auth error, profile switch
and long transcript states are in scope. Send/stream/stop, terminal, previews and
other screens are subsequent increments, not simulated functionality.

## Visual gate

Shape brief explicitly confirmed by the user. Direction probes and a new
north-star mock are skipped: this is a reference-led port with a fixed Hermes
composition and user-selected real OpenCode theme/components, not an unresolved
new visual composition. Upstream reference renders and native screenshot
inspection are the visual evidence. Do not rasterize UI text or controls.
