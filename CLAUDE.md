# TAW Analyser: working conventions

The Analyser (SP-001) is Timster's Audio Works' stand-alone spectrum analyser, written in Rust with egui (via eframe). Its look (the `hifi` module: finishes, nameplate, window buttons and controls), spectrum drawing and FFT come from the shared `taw-panel` crate (TimsterAudio/TAW-Panel, feature `analyser`), pinned by commit in Cargo.toml. The window has no system title bar unless the user turns it on in Settings. Change those there, then move the pin.

## Hard rules

- **Native, not HTML.** No web views or browser shells.
- **Listen only.** The Analyser never plays audio, so it can never feed back.
- **No network requests at runtime.** Fonts and the icon are built into the binary.
- **Keep the capture callbacks real-time safe.** They must not lock, allocate, do file I/O or log; they only push into the `AnalyzerTap`.
- **Never break saved settings.** `Saved` in `app.rs` uses `#[serde(default)]`. Add new fields with defaults, and never rename or repurpose existing ones. Spectrum `Scheme` and `Look` are stored by name.

## Files

| File | What it does |
|---|---|
| `src/main.rs` | The window. |
| `src/app.rs` | The nameplate and family switcher, the display (curve, bars, LED columns, grid), the control bar (scales to fit narrow windows; H hides it) and the settings drawer. |
| `src/capture.rs` | Listen-only input: an output's monitor (PipeWire) or loopback (WASAPI), or an input device (cpal). |
| `src/family.rs` | Finding and starting the Library, Player and Equaliser. |
| `src/icon.rs` | The window icon. |

## Checks before pushing

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo clippy --target x86_64-pc-windows-gnu -- -D warnings   # Windows still compiles
```

For panel changes, screenshot it under Xvfb at 1280 × 720 and at a narrow size such as 800 × 500, in Anodised and Porcelain, light and dark.

## Workflow

- Work on a feature branch. `main` always holds a working version.
- Open a pull request when the user asks, or merge when they ask.
- For every user-visible change, add a line under **Unreleased** in `CHANGELOG.md`.
- For a release: move Unreleased into a section headed `## X.Y.Z – YYYY-MM-DD`, and bump `version` in `Cargo.toml` and the version in `README.md`. Merging that to `main` publishes the release with the Linux and Windows packages (`.github/workflows/release.yml`).
