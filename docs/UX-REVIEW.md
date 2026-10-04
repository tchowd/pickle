# Pickle UX refresh

The reading experience now centers on three actions: Simplify, Explain more, and Visualize.

## Review and changes

- **Floating actions:** The previous wide card gave character counts equal weight to actions. The new compact bar keeps a one-line passage preview, source app, and three clear actions. A native drag handle moves the window without interfering with controls or selectable text. The reader header is a 32-point strip containing only the drag handle and settings. Branding, pin, and close controls are removed; Escape dismisses the reader, including from text fields.
- **Reading:** Model names, elapsed times, capture methods, bundle IDs, and evaluator diagnostics are removed from the main experience. The answer check section and supplementary technical notes are removed. Answers use a readable rounded sans-serif with generous line spacing, paired with bold rounded headings and occasional monospaced accents.
- **Welcome:** One path for highlighted text and one for pasted text, with a sample available before connection setup.
- **Settings:** Reading, Privacy, and Connection replace a long numbered setup form. Essential account fields remain available; advanced model and credential controls are collapsed. Privacy disclosures remain explicit about recipients.
- **Menu bar:** Reading actions and preferences replace provider status readouts.
- **Appearance:** A dark sci-fi palette uses native macOS behind-window glass, acid-green controls, cyan accents, subtle portal glow, and native vector pickle artwork. The header and chat input have no horizontal separator lines. Glass uses NSVisualEffectView and respects the system Reduce Transparency preference. Settings, charts, input fields, and the menu-bar icon share the theme. The compact header remains free of branding.
- **Positioning:** Expansion preserves the moved panel’s bottom-center anchor where space allows. Screen-edge clamping keeps the expanded panel visible. Position and expanded size are remembered across launches.

## References

- [PopClip](https://www.popclip.app/guide/): compact actions for selected text.
- [Raycast Notes](https://www.raycast.com/core-features/notes): a focused floating companion window.

## Validation

- 46 deterministic checks pass.
- Native smoke checks cover opening, closing, reopening, moved-anchor expansion, screen-edge clamping, and remembering position.
- Native visual review covered the compact bar, example result, welcome screen, and Reading, Privacy, and Connection tabs. Exercised the drag handle in an isolated preview.
- No live provider requests or saved credential changes were needed for this review.

The generated app is in `dist/Pickle.app`. An already-running copy must be restarted to load the new interface. Position and expanded size are now saved across launches. The dark portal theme was visually reviewed in the native action bar, reader, welcome screen, and settings.

## Glass consistency pass

Settings now use the same translucent inset cards as conversation replies, notices, context editors, paste fields, chart nodes, and follow-up input. Secondary buttons use a translucent fill instead of an opaque green block. Connection fields and settings toggles share consistent spacing and styling. Native visual review covered Reading and Connection settings; all 46 checks and native window/session checks passed.


## Reading feature expansion

Added compact answer adjustments, native selected-text explanations plus a word-entry alternative, a lower-right resize handle, opt-in searchable saved answers, streaming drafts, and Appearance preferences. The header remains slim; settings and bookmarks are its only actions. Saved answers never replace the fresh-session shortcut behavior. Validation now includes 50 deterministic checks and native smoke checks for saved-answer round trips, deduplication/removal, window geometry, appearance persistence, contextual questions, and answer adjustments. Native previews covered the reading controls, saved library, and Appearance screen.

## Page context update

A compact disclosure shows whether page context is available, with a source-window preview, extracted text, capture time, and removal. Capture happens once per selection session; follow-ups reuse context. Local OCR precedes normal reading, with a five-second selection-only fallback. Settings exposes page capture and screen permission. Image upload is a separately labeled action with its recipient and potential charge shown before use. New-session and disabled-feature cleanup are covered by native checks; synthetic OCR avoids capturing private screen content during testing. All 54 core checks and native smoke checks passed. Live screen permission behavior and live image inference remain unverified.

## Web and video references

A compact page/video disclosure exposes the source title, timestamp, plain text excerpt, open-source link and removal. Page references default off until enabled in Privacy settings. A dedicated browser setup disclosure keeps extension details outside the reading flow. Page-only invocation is supported; automatic mouse-selection UI still requires selected text. Video captions use the current moment or an explicitly bounded transcript. Audio fallback has a visible recording state, source-app scope explanation and Cancel. Reading actions wait for bounded context and never restart when a late reference arrives.

## Answer formatting

Writers often return light Markdown even when asked for plain text, so answers showed literal `**` markers and numbered items whose wrapped lines ran back under the number. Answers, follow-ups, streaming drafts and saved answers now render bold, italic, inline code, headings, bulleted and numbered lists (with hanging indents and acid-green markers), quotes and code blocks. Paragraphs and list items have their own spacing rhythm. Links show only their label and are never clickable; inline HTML stays literal text. The original passage is always shown exactly as captured. Copy places readable text without Markdown syntax on the clipboard, and Explain selection accepts words chosen from the formatted answer. Answers also start directly: the writer is told not to announce its answer, and an opening line such as “Here is a simplified explanation of the problem statement:” is removed when a model adds one anyway. Lead-ins that carry content, like “Here are the three causes:”, stay.

Answer text could overlap the Shorter / More detail / Give an example row. SwiftUI measures the text at several trial widths, and each trial resized the visible text layout, so text could be drawn narrower (and taller) than the space reserved for it. Measurement now uses a separate layout. The adjustment row uses smaller secondary chips with clear space above them. All 68 core checks and native smoke checks passed.
