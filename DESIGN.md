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

Preserve the Hermes shell, not OpenCode's application composition. The top
window band contains the sidebar toggle, workspace conversation tabs and system
controls. Sessions / Bots is a separate strip below that band in the left zone.
Sessions keeps the original navigation order, then search and session rows;
profile scope stays at the sidebar foot. Bots opens the named, server-resolved
Bot Chat for each identity, never its newest ordinary session. Plain clicks
replace the foreground tab; explicit Open in new tab preserves it.

Preserve Hermes's default Advanced topology: sessions left, workspace center,
Review and Files in the upper-right, Terminal below when tooling is revealed.
Fresh installs rest tooling closed, including in Advanced; DEFAULT_TREE alone
does not establish visibility. Simple shadows that visibility and leaves
connection recovery reachable. Sessions width starts at 237 logical pixels.
The existing presets, nested splits, tab stacks, focus,
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

### Composer reference override

The user supplied T3 Code composer/model-picker screenshots and explicitly
requested their composition. Use one rounded multiline editor, removable
attachment tiles above the text, and the updated 21:12 T3 toolbar: attachment
action at bottom left, compact model and reasoning summary at bottom right,
and one round settings button. Its single popover contains Reasoning, Service
Tier and a segmented Permissions selector with the selected policy description.
Hermes dictation, voice-engine and primary send/stop controls remain available.
This supersedes the earlier three separate model/reasoning/access triggers. The picker has a narrow provider/favorites rail, search,
selected rows with checkmarks, favorite toggles and provider sublabels.
Keep the actual OpenCode light/dark palette and Rune controls. Do not copy T3's
provider list or invent capabilities. Draft settings remain scoped to their
connection/profile/conversation and are not the model used by existing history.
Hermes approval modes remain Manual/Smart/Off. Its voice fan retains separate
dictation, spoken-reply and wake-word controls; unsupported audio is unavailable,
not a fake enabled toggle. Native file picks stage draft attachments only.
An empty idle composer has a separate waveform voice-conversation action and
engine chevron; payload replaces it with Send. Engine selection is profile-owned
backend configuration, never a fake draft preference. The lower-left rail and
statusbar must retain their full nested feature set with comfortable grouping;
the reference's tiny, tightly packed controls are not mandatory styling.

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

## Correction after the first implementation

The user rejected 6001795 as an OpenCode shell connected to Hermes. A matching
pane silhouette was insufficient: it omitted Sessions / Bots, the named Bot Chat
door, the original navigation and the bottom profile scope rail. Native widgets
and a live connection do not establish Hermes spatial or behavioral parity.
Compare subsequent increments with actual retained Hermes renders as well as
source contracts. Record omissions in apps/rune/docs/shell-parity.md.
