# Product

## Register

product

## Users

Owners of personal Hermes agents running locally or remotely. They return to
ongoing conversations, switch among bot identities and sessions, and need a
readable working surface during long desktop sessions, using both keyboard and
pointer. The application is a conversation client for existing agents.

## Product Purpose

Provide an independently installable desktop client for existing Hermes agents.
The standalone Electron implementation is the behavioral and spatial baseline.
Port its element placement, navigation, actions and state semantics to native
Rune; improve presentation using the original OpenCode Rune implementation.
Structural conversation-focused redesign is a subsequent project phase.

## Brand Personality

Calm, clear, capable. The user's conversation and agent identity have priority;
chrome supports the work. Keep Hermes identity, not OpenCode product branding.

## References

The user explicitly selected anomalyco/rune's OpenCode reference and authorized
substantial reuse of its actual implementation. Use rune-kit and the OpenCode
theme as real dependencies; adapt appropriate view code with attribution.
Examples/opencode is present at upstream 3f70cd6b675c89a8a7b64fc09b1bf2b455e0ad1a,
removed by d1634c68a4eeb1ca0d1787df266547a1ab3eccea. Its transport is not Hermes.

## Anti-references

The user rejected the current dense, cluttered, IDE-like visual styling. Do not
reproduce that styling or make a generic decorative AI dashboard. Do not change
Hermes's panel topology merely because OpenCode uses a different sidebar.
An OpenCode application shell with Hermes transport is explicitly rejected.
Reference reuse applies to controls, themes and suitable rendering functions;
Hermes owns the product navigation, bot identity model and opening semantics.

## Design Principles

1. Conversation and identity before tool chrome.
2. Preserve spatial and behavioral expectations while changing visual styling.
3. Reuse working reference components rather than approximate them from scratch.
4. Agent state belongs to its gateway; client state stays in client storage.
5. State the real capabilities of each migration increment honestly.

## Accessibility & Inclusion

Preserve keyboard focus, cancellation, text selection, scaling and reduced-motion
behavior. Aim for readable contrast and labels across light/dark appearances,
with Russian and English text and long dynamic titles. The existing desktop
DESIGN.md supplies the interaction baseline. Rune's current semantic tree does
not supply an OS screen-reader bridge; record this gap, never claim parity from
a headless element tree alone.
