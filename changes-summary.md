# Custom Zed Tweaks Summary

This document captures the experiments and adjustments we made during the investigation of
hover/tooltip flicker, minimap behaviour, and theme overrides.

## Rendering and Antialiasing
- Added `crates/gpui/src/render_prefs.rs` and wired `GlyphKind` through the text rendering
  pipeline so editor and UI glyphs can use different antialiasing profiles.
- Buffer glyphs disable subpixel positioning by default; UI text keeps the smoother profile.
- Forced `ZED_PATH_SAMPLE_COUNT` values of `0` or unset to resolve to `1` sample in the Blade
  renderer, effectively disabling MSAA for text paths unless explicitly overridden.

## Hover/Minimap Flicker Mitigation
- Suppressed hover popover handling for minimap editors and removed the minimap drop shadow to
  reduce opacity pulses during hover updates.
- Avoid redundant minimap scroll synchronisation, only updating when the scroll offset actually
  changes.
- Documented the practical workaround of keeping minimap width ≤ 40 columns to minimise repaint
  cost.

## Title Bar and Menu Adjustments
- Prevented application menus from opening immediately on first hover when `show_menus` is enabled.
- Rewrote the menu construction logic so menus only switch on hover when another menu is already
  open, matching traditional menu-bar behaviour.

## Theme Overrides (user settings)
- Demonstrated how to use `theme_overrides` in `settings.json` instead of per-theme JSON files to
  adjust:
  - `editor.background`
  - `panel.background`
  - `terminal.background`
  - `editor.gutter.background`
  - tab strip and tab colours
  - `title_bar.background` and `status_bar.background`
- Added notes about hiding the minimap thumb via transparent colours and narrowing the minimap to
  avoid flicker.

## LTEX & Diagnostics Tips
- Ensured the LTeX language server is listed under `languages.Python.language_servers` and added
  `"python"` to `lsp.ltex.settings.ltex.languageIds` so docstrings receive diagnostics.
- Reminder: diagnostics and inline diagnostics can be toggled with keybindings mapped to
  `editor::ToggleDiagnostics` and `editor::ToggleInlineDiagnostics`.

## Remaining Observations
- The residual hover flicker stems from minimap repaints inside the Blade renderer; eliminating it
  entirely would require a deeper redesign of the minimap rendering pipeline.
- Edit Predictions (Cursor-style Tab completions) remain tied to Zed’s hosted APIs; replacing them
  with a local provider (e.g. Ollama) would require implementing a new `EditPredictionProvider` that
  speaks the same request/response format.

