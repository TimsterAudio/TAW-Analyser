# Handoff: TAW Analyser (SP-001)

Written 2026-10-14 when the family's work moved from one shared chat to a chat per app. Read CLAUDE.md, then this.

## Where it stands

- **0.2.0 released on 2026-10-14** (merged to `main`; release.yml publishes `v0.2.0`): the new TAW look. Spool finish by default (Light, Amber), Porcelain alternative; display fills the window; controls in a bottom bar (H hides it); no system title bar (own window buttons, drag, resize; "Use the system title bar" in Settings); Simple and Detailed; family switcher. The spectrum draws in the display's ink by default; colour schemes are in the Colour menu.
- TAW-Panel pinned at its 0.3.0 commit `1b8162d`, the same as the Equaliser.

## Workup, 2026-10-14

Checked with live audio at 1280 × 720 and 800 × 500: Simple and Detailed; Curve, Bars and LEDs; Freeze; Spool and Porcelain, Light and Dark; the settings drawer; H to hide the controls and double-click to bring them back; the family switcher's "isn't installed" message. Two fixes came out of it:

- The window's thin outline now runs round the control bar too (the bar used to cover it).
- Messages sit on the display's top line, right-aligned (left of FROZEN), instead of over the frequency scale.

Matches the Equaliser: same TAW-Panel pin, nameplate, window buttons, family switcher, Simple/Detailed, gear and settings drawer (Look, Window).

## Next

1. The user's feedback on 0.2.0, picked up in this app's own chat.

## How to test here

Checks as in CLAUDE.md. Run it under Xvfb with PipeWire playing something (see TAW-Player's `docs/ARCHITECTURE.md`, "Testing without a sound card"; `pw-play --target testsink noise.wav`) so the spectrum has a signal. Loop it (`while true; do pw-play …; done`) or it stops after one pass. Under Xvfb, keys only reach the window after a click on it; stop the Analyser with `pkill -x taw-analyser` (a `pkill -f` pattern can match your own shell). Killing it skips the save, but eframe also saves every 30 s, so a later start may come up in whatever look was last saved.
