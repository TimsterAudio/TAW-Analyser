# Handoff: TAW Analyser (SP-001)

Written 2026-10-14 when the family's work moved from one shared chat to a chat per app. Read CLAUDE.md, then this.

## Where it stands

- **0.2.0 released on 2026-10-14** (merged to `main`; release.yml publishes `v0.2.0`): the new TAW look. Spool finish by default (Light, Amber), Porcelain alternative; display fills the window; controls in a bottom bar (H hides it); no system title bar (own window buttons, drag, resize; "Use the system title bar" in Settings); Simple and Detailed; family switcher. The spectrum draws in the display's ink by default; colour schemes are in the Colour menu.
- **0.2.1 released on 2026-10-16** (merged to `main`; release.yml publishes `v0.2.1`): the family's tighter look and the spectrum falling away in silence (below).
- TAW-Panel pinned at its 0.4.0 commit `d030ffd`, the same as the Player and Equaliser.

## Workup, 2026-10-14

Checked with live audio at 1280 × 720 and 800 × 500: Simple and Detailed; Curve, Bars and LEDs; Freeze; Spool and Porcelain, Light and Dark; the settings drawer; H to hide the controls and double-click to bring them back; the family switcher's "isn't installed" message. Two fixes came out of it:

- The window's thin outline now runs round the control bar too (the bar used to cover it).
- Messages sit on the display's top line, right-aligned (left of FROZEN), instead of over the frequency scale.

Matches the Equaliser: same TAW-Panel pin, nameplate, window buttons, family switcher, Simple/Detailed, gear and settings drawer (Look, Window).

## 2026-10-16: lined up with the Player and Equaliser

- **TAW-Panel 0.4.0** (`d030ffd`): the shorter nameplate (badge and model line), one corner radius, the heavier chosen segment, the drawer's section hairlines, corner resizing (0.3.2). Meter colours aren't used: the Analyser has no meters.
- **Top band at the Player's measurements:** plate 66 (was 96), nameplate at (24, 10), window buttons at 6 from the top, the key row (family switcher, Simple/Detailed, gear) at 46; the display starts at plate + 8, so it gets 26 px more height.
- **The Equaliser's spectrum fix, where it applies.** The Equaliser's flicker came from hiding the spectrum and its dB scale whenever `Analyzer::active` was false; the Analyser never checked `active` (its scale is drawn with the grid), so it never flickered. The other half does apply: with no samples (Windows loopback while the output is silent) the bins stood still. `fall_when_unfed` in `app.rs` lets them fall 60 dB/s after 0.25 s without samples, peaks at their own fall rate; Freeze still holds the picture. Under Xvfb the spectrum fell to the floor within 3 s of stopping playback, but a PipeWire monitor may keep sending silence, so that doesn't prove the no-samples path; check on Windows.
- Checks pass (fmt, clippy for Linux and Windows, tests); screenshots under Xvfb at 1280 × 720 and 800 × 500, Spool and Porcelain, light and dark, and the settings drawer. README screenshots replaced.

## Next

1. The user's visual test of 0.2.1 against the Player and Equaliser, and of the spectrum falling away on Windows.
2. The rest of 0.2.x in ROADMAP: tests with real sources on Windows and Linux.

## How to test here

Checks as in CLAUDE.md. Run it under Xvfb with PipeWire playing something (see TAW-Player's `docs/ARCHITECTURE.md`, "Testing without a sound card"; `pw-play --target testsink noise.wav`) so the spectrum has a signal. Loop it (`while true; do pw-play …; done`) or it stops after one pass. Under Xvfb, keys only reach the window after a click on it; stop the Analyser with `pkill -x taw-analyser` (a `pkill -f` pattern can match your own shell). Killing it skips the save, but eframe also saves every 30 s, so a later start may come up in whatever look was last saved.
