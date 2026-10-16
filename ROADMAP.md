# Roadmap

Planned work for the Analyser. Tick items off (`[x]`) when they ship, and note the version. It started as SP-10's own track in the EQ Box plan (2026-10-12).

## Pipeline (proposed 2026-10-14)

Sizes: S = one session, M = two or three, L = a release of its own. Briefs from the family session go in `docs/briefs/`; TAW-Panel's `docs/TRACKER.md` tracks the family as a whole.

| Release | What | Size |
|---|---|---|
| 0.2.x | The user's feedback on 0.2.0, and tests with real sources | S |
| 0.3.0 | More ways to look: channel choice, averaging, a menu entry | M |
| 0.4.0 | A spectrogram, and saved display setups | M–L |
| later | Meters, and a Box-family face | — |

### 0.2.x: tested in practice

- [ ] Windows loopback of the default output and of another output
- [ ] Linux output monitors; an input (microphone)
- [ ] Full screen and On top on a second screen
- [ ] Fixes from the user's feedback
- [x] The family's tighter look: TAW-Panel 0.4.0, the Player's top band; the spectrum falls away in silence *(0.2.1)*

### 0.3.0: more ways to look

- [ ] Channel choice: L + R, left, right, mid or side
- [ ] Averaging: a slow, steady curve beside the live view
- [ ] Its own Start menu and desktop entry

### 0.4.0: a spectrogram

- [ ] A spectrogram (waterfall) view; if it needs new drawing, that goes in TAW-Panel as a minor version
- [ ] Saved display setups (size, view, colours) for different screens

### Later

- [ ] A meter view (the dB scale from TAW-Panel 0.3.1; VU from 0.6.0)
- [ ] A Box Analyser for the Box family: the same engine with the Jupiter face

## Done

- [x] A stand-alone analyser, moved out of EQ Box *(0.1.0)*
- [x] The TAW look, Simple and Detailed, the family switcher *(0.2.0)*
