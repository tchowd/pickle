use pickle_core::contains_source_term;

#[test]
fn canonical_equivalence_full_casefold_and_original_grapheme_boundaries() {
    // Positive and negative outcomes were checked against Foundation's
    // localizedCaseInsensitiveContains in the controlled default-locale probe.
    for (source, term, expected) in [
        ("cafe\u{301}", "café", true),
        ("café", "cafe\u{301}", true),
        ("Straße", "STRASSE", true),
        ("STRASSE", "Straße", true),
        ("sß", "ss", true),
        ("ß", "ss", true),
        ("ß", "s", false),
        ("café", "cafe", false),
        ("café", "caf", true),
        ("é", "e", false),
        ("ΟΣ", "ος", true),
        ("ﬀ", "ff", true),
        ("ﬀ", "f", false),
        ("I", "ı", false),
        ("İ", "i", false),
        ("Cell\n\tENERGY", "  cell   energy  ", true),
        ("The molecule", "mole", true),
        ("", "", false),
        ("A source", " \n ", false),
        ("A source", "not supplied", false),
    ] {
        assert_eq!(
            contains_source_term(source, term),
            expected,
            "{source:?} / {term:?}"
        );
    }
}

#[test]
fn canonical_matching_keeps_accents_width_punctuation_and_compatibility_distinctions() {
    for (source, term) in [
        ("côté", "cote"),
        ("Ａ", "A"),
        ("①", "1"),
        ("x-y", "xy"),
        ("a*b", "ab"),
        ("cell energy", "cell biology"),
    ] {
        assert!(!contains_source_term(source, term), "{source:?} / {term:?}");
    }
    // Reordered combining marks are canonically equivalent, but losing a mark
    // or matching only a fragment of the original grapheme is not allowed.
    assert!(contains_source_term("a\u{301}\u{323}", "a\u{323}\u{301}"));
    assert!(!contains_source_term("a\u{301}\u{323}", "a\u{301}"));
    assert!(contains_source_term("prefix cafe\u{301} suffix", "CAFÉ"));
    assert!(!contains_source_term("prefix cafe\u{301} suffix", "CAFE"));
}
