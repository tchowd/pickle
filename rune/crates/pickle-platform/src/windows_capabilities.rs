/// The size-only GetCurrentPackageFullName query returns
/// ERROR_INSUFFICIENT_BUFFER (122) and a nonempty UTF-16 size for package identity.
/// APPMODEL_ERROR_NO_PACKAGE (15700) and other failures must not enable OCR.
pub(crate) fn identity_probe_succeeded(status: u32, length: u32) -> bool {
    status == 122 && length > 1 && length <= 65_536
}

pub(crate) fn ocr_fixture_matches(text: &str) -> bool {
    text.split(|character: char| !character.is_ascii_alphabetic())
        .any(|word| word.eq_ignore_ascii_case("PICKLE"))
}

// Native CI recognizes this desktop-sized fixture exactly; the former 96px glyphs
// yielded empty text despite valid pixels, formats, stride, DPI and English language.
pub(crate) const OCR_FIXTURE_FONT_HEIGHT: i32 = 48;

/// Diagnostics for the owned fixed-word runtime probe only. Never pass captured user text here.
pub(crate) fn ocr_fixture_diagnostic(
    recognized: &str,
    language: &str,
    width: u32,
    height: u32,
    dark_pixels: usize,
) -> serde_json::Value {
    serde_json::json!({
        "schema": 1,
        "test": "windows-ocr-fixed-fixture-diagnostics",
        "expected": "PICKLE",
        "matched": ocr_fixture_matches(recognized),
        "recognized_text": recognized.chars().take(128).collect::<String>(),
        "recognized_truncated": recognized.chars().count() > 128,
        "recognizer_language": language.chars().take(64).collect::<String>(),
        "width": width,
        "height": height,
        "dark_pixels": dark_pixels,
        "font": "Segoe UI",
        "font_height": OCR_FIXTURE_FONT_HEIGHT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_fixture_diagnostic_is_bounded_escaped_and_never_changes_the_match() {
        let diagnostic = ocr_fixture_diagnostic("P1CKLE\n\"\\", "en-US", 768, 180, 240);
        assert_eq!(diagnostic["matched"], false);
        assert_eq!(diagnostic["recognized_text"], "P1CKLE\n\"\\");
        assert_eq!(diagnostic["font_height"], 48);
        assert_eq!(diagnostic["width"], 768);
        assert_eq!(diagnostic["height"], 180);
        let serialized = diagnostic.to_string();
        assert!(!serialized.contains('\n'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serialized).unwrap(),
            diagnostic
        );
        let diagnostic = ocr_fixture_diagnostic(&"語".repeat(500), &"e".repeat(100), 768, 180, 240);
        assert_eq!(
            diagnostic["recognized_text"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            128
        );
        assert_eq!(
            diagnostic["recognizer_language"].as_str().unwrap().len(),
            64
        );
        assert_eq!(diagnostic["recognized_truncated"], true);
    }
    #[test]
    fn ocr_probe_requires_the_exact_known_fixture_word() {
        assert!(ocr_fixture_matches("PICKLE\n"));
        assert!(ocr_fixture_matches("  Pickle. "));
        for text in [
            "",
            "unrelated",
            "PICKLES",
            "PICKL E",
            "P1CKLE",
            "PREFIXPICKLE",
        ] {
            assert!(!ocr_fixture_matches(text));
        }
    }
    #[test]
    fn unpackaged_and_invalid_identity_probes_do_not_enable_ocr() {
        assert!(identity_probe_succeeded(122, 100));
        for (status, size) in [
            (15700, 0),
            (5, 100),
            (0, 100),
            (122, 0),
            (122, 1),
            (122, 65_537),
        ] {
            assert!(!identity_probe_succeeded(status, size));
        }
    }
}
