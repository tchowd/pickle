# Pickle UX refresh

The reading experience now centers on three actions: Simplify, Explain more, and Visualize.

## Review and changes

- **Floating actions:** The previous wide card gave character counts equal weight to actions. The new compact bar keeps a one-line passage preview, source app, and three clear actions. A native drag handle moves the window without interfering with controls or selectable text. The reader header is a 32-point strip containing only the drag handle and settings. Branding, pin, and close controls are removed; Escape dismisses the reader, including from text fields.
- **Reading:** Model names, elapsed times, capture methods, bundle IDs, and evaluator diagnostics are removed from the main experience. The answer check section and supplementary technical notes are removed. Answers use a readable rounded sans-serif with generous line spacing, paired with bold rounded headings and occasional monospaced accents.
- **Welcome:** One path for highlighted text and one for pasted text, with a sample available before connection setup.
- **Settings:** Reading, Privacy, and Connection replace a long numbered setup form. Essential account fields remain available; advanced model and credential controls are collapsed. Privacy disclosures remain explicit about recipients.
- **Menu bar:** Reading actions and preferences replace provider status readouts.
- **Appearance:** A dark sci-fi palette uses native macOS behind-window glass, acid-green controls, cyan accents, subtle portal glow, and native vector pickle artwork. The header and chat input have no horizontal separator lines. Glass uses NSVisualEffectView and respects the system Reduce Transparency preference. Settings, charts, input fields, and the menu-bar icon share the theme. The compact header remains free of branding.
- **Positioning:** Expansion preserves the moved panel’s bottom-center anchor where space allows. Screen-edge clamping keeps the expanded panel visible. Its position is remembered for subsequent invocations during the current app session.

## References

- [PopClip](https://www.popclip.app/guide/): compact actions for selected text.
- [Raycast Notes](https://www.raycast.com/core-features/notes): a focused floating companion window.

## Validation

- 46 deterministic checks pass.
- Native smoke checks cover opening, closing, reopening, moved-anchor expansion, screen-edge clamping, and remembering position.
- Native visual review covered the compact bar, example result, welcome screen, and Reading, Privacy, and Connection tabs. Exercised the drag handle in an isolated preview.
- No live provider requests or saved credential changes were needed for this review.

The generated app is in `dist/Pickle.app`. An already-running copy must be restarted to load the new interface. Position memory currently lasts for the app session; it is not saved across launches. The dark portal theme was visually reviewed in the native action bar, reader, welcome screen, and settings.

## Glass consistency pass

Settings now use the same translucent inset cards as conversation replies, notices, context editors, paste fields, chart nodes, and follow-up input. Secondary buttons use a translucent fill instead of an opaque green block. Connection fields and settings toggles share consistent spacing and styling. Native visual review covered Reading and Connection settings; all 46 checks and native window/session checks passed.
