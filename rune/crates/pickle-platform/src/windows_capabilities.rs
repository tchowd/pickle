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

#[cfg(test)]
mod tests {
    use super::*;
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
