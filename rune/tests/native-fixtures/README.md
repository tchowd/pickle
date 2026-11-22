# Controlled native source fixture

The macOS fixture is a separate ordinary AppKit application with a standard editable text view. It contains fixed test data and preselects only the middle paragraph, including non-ASCII text. It never requests capture permissions, reads user data, or contacts a network service. It is a test tool, not part of the shipped Pickle app.

From `rune`, run `bash scripts/build-macos-fixture.sh`, then open `artifacts/Pickle Capture Fixture.app`.

Use it to record these real native checks against an exact Pickle build:

1. With Pickle's Accessibility permission denied, invoke Pickle while the fixture has focus. Verify a clear permission failure, with no guessed or whole-document source.
2. After the tester explicitly grants Pickle Accessibility access, focus the fixture and invoke Pickle using the configured gesture/shortcut. Verify that the chooser contains only the selected paragraph, preserving `Café` and `—`, with the fixture as source. The BEFORE/AFTER paragraphs must not become the selected passage.
3. Choose a reading action. Verify that focus changes preserve this immutable source; subsequent changes to the fixture's selection must not change an existing request or follow-up.
4. Enable automatic invocation explicitly and test repeated selections, a second drag while capture is pending, dismissal, and pin suppression. A preview must not replace an existing answer until an action is chosen.
5. If the tester explicitly grants Screen Recording and enables source-window OCR, verify the context preview against this source window. OCR may include the surrounding paragraphs as labeled context; it must not replace the selected passage or supply chart evidence.
6. Test Escape, close, reopen, pause, exclusions and permission revocation. Close the fixture when done; it has no document-save flow or user content to preserve.

This fixture validates one controlled AppKit provider only. It does not replace native-editor, Chrome, Edge, protected-field, accessibility, multi-monitor, performance or minimum-OS acceptance. Record source commit, bundle hash, OS, permissions, steps and observed outcomes; building the fixture alone is not a passed capture test.
