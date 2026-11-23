//! Matching helpers never replace the original source or the user's question.
use icu_normalizer::DecomposingNormalizerBorrowed;
use unicase::UniCase;
use unicode_segmentation::UnicodeSegmentation;

/// Whitespace-folded, canonically equivalent, case-insensitive substring search.
/// Uses Unicode's default (non-Turkic) full case folding, not the operating
/// system's current locale or an assertion of identical locale collation.
/// No accent stripping or compatibility normalization is applied; full case
/// folding itself still includes mappings such as ligature `ﬀ` to `ff`.
/// Match endpoints must
/// correspond to original grapheme boundaries: `ss` can match `ß`, but a single
/// `s` cannot match half of it, nor can `cafe` match part of `café`.
pub fn contains_source_term(source: &str, term: &str) -> bool {
    let (needle, _) = folded_graphemes(term);
    if needle.is_empty() {
        return false;
    }
    let (source, boundaries) = folded_graphemes(source);
    // Search every original boundary rather than non-overlapping matches.
    // In `sß` / `ss`, an invalid hit at 0 overlaps the valid complete ß at 1.
    boundaries.iter().copied().any(|start| {
        source[start..].starts_with(&needle)
            && boundaries.binary_search(&(start + needle.len())).is_ok()
    })
}

fn folded_graphemes(text: &str) -> (String, Vec<usize>) {
    let spaced = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let nfd = DecomposingNormalizerBorrowed::new_nfd();
    let mut folded = String::new();
    let mut boundaries = vec![0];
    for grapheme in spaced.graphemes(true) {
        let canonical = nfd.normalize(grapheme);
        let caseless = UniCase::new(canonical.as_ref()).to_folded_case();
        folded.push_str(&nfd.normalize(&caseless));
        boundaries.push(folded.len());
    }
    (folded, boundaries)
}
