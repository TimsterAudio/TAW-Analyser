# Handoff: TAW Analyser (SP-001)

Written 2026-10-14 when the family's work moved from one shared chat to a chat per app. Read CLAUDE.md, then this.

## Where it stands

- `main` is 0.1.0, released (Jupiter panel).
- Branch `claude/focused-ritchie-veg4m9`, unmerged: **the new TAW look** (`f0dc9af`, `d6330cd`). Spool finish by default (Light, Amber), Porcelain alternative; display fills the window; controls in a bottom bar (H hides it); no system title bar (own window buttons, drag, resize; "Use the system title bar" in Settings); Simple and Detailed; family switcher. The spectrum draws in the display's ink by default; colour schemes are in the Colour menu. CI is green on the branch.
- TAW-Panel is pinned at `ce561c2` (unreleased, on TAW-Panel's branch). Before a release, TAW-Panel needs its 0.3.0 version recorded and this pin moved.

## Next

1. The user's review of the look (CI builds on the branch).
2. Then a release (0.2.0): move Unreleased into a version section, bump Cargo.toml and README, merge.

## How to test here

Checks as in CLAUDE.md. Run it under Xvfb with PipeWire playing something (see TAW-Player's `docs/ARCHITECTURE.md`, "Testing without a sound card"; `pw-play --target testsink noise.wav`) so the spectrum has a signal.
