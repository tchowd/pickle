use super::*;
use std::sync::Arc;

fn locale() -> LibrarySearchLocale {
    library_search_locale().unwrap()
}
fn search(index: &LibrarySearchIndex, query: &str) -> Vec<usize> {
    index
        .search(query, &locale(), &CancellationToken::new())
        .unwrap()
}

#[test]
fn batching_counts_exact_json_bytes_and_never_splits_a_large_row() {
    let controls = (0u8..=0x1f).map(char::from).collect::<String>();
    for text in [
        "",
        "ASCII",
        "quote\" slash\\ /",
        "🫛 cafe\u{301}",
        &controls,
    ] {
        assert_eq!(
            json_string_bytes(text).unwrap(),
            serde_json::to_vec(text).unwrap().len()
        );
    }
    let rows = vec![vec!["a field"]; 257];
    assert_eq!(
        batch_ranges(&rows).unwrap(),
        vec![0..128, 128..256, 256..257]
    );
    let large = "x".repeat(BATCH_BYTES + 1);
    assert_eq!(
        batch_ranges(&[vec!["small"], vec![&large], vec!["last"]]).unwrap(),
        vec![0..1, 1..2, 2..3]
    );
}

#[test]
fn native_search_keeps_fields_whitespace_empty_policy_and_row_order() {
    let rows = vec![
        vec!["alpha", "beta"],
        vec!["alpha beta"],
        vec!["ALPHA\nBETA"],
        vec!["alpha\tbeta"],
        vec!["alpha  beta"],
        vec![],
        vec!["", "QUESTION-END", "https://example.invalid/POINTER-END"],
        vec!["left\0right"],
    ];
    let index = LibrarySearchIndex::new(&rows, &CancellationToken::new()).unwrap();
    assert_eq!(search(&index, "alpha beta"), vec![1]);
    assert_eq!(search(&index, "alpha\nbeta"), vec![2]);
    assert_eq!(search(&index, "alpha\tbeta"), vec![3]);
    assert_eq!(search(&index, "alpha  beta"), vec![4]);
    assert_eq!(search(&index, "question-end"), vec![6]);
    assert_eq!(search(&index, "pointer-end"), vec![6]);
    assert_eq!(search(&index, "right"), vec![7]);
    assert_eq!(search(&index, "QUESTION-END https"), Vec::<usize>::new());
    assert_eq!(search(&index, ""), (0..rows.len()).collect::<Vec<_>>());
    assert_eq!(search(&index, " alpha "), Vec::<usize>::new());
}

#[test]
fn native_search_handles_canonical_forms_without_accent_or_width_flags() {
    let rows = vec![
        vec!["cafe\u{301}"],
        vec!["café"],
        vec!["cafe"],
        vec!["Ｘ"],
        vec!["X"],
    ];
    let index = LibrarySearchIndex::new(&rows, &CancellationToken::new()).unwrap();
    assert_eq!(search(&index, "CAFÉ"), vec![0, 1]);
    assert_eq!(search(&index, "cafe"), vec![2]);
    assert_eq!(search(&index, "x"), vec![4]);
}

#[test]
fn prepared_native_text_outlives_inputs_and_supports_concurrent_arc_queries() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<LibrarySearchIndex>();
    send_sync::<LibrarySearchLocale>();
    let index = {
        let source = String::from("ORIGINAL payload");
        Arc::new(LibrarySearchIndex::new(&[vec![&source]], &CancellationToken::new()).unwrap())
    };
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let index = index.clone();
            std::thread::spawn(move || {
                for _ in 0..25 {
                    assert_eq!(search(&index, "original"), vec![0]);
                }
            })
        })
        .collect();
    drop(index);
    // The final native retain can be released by a worker, never requiring a
    // GUI loop, AppKit ownership, or a thread-affine cleanup callback.
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn large_legal_field_is_retained_intact_and_stats_are_content_free() {
    let source = format!("{} UNIQUE-END", "x".repeat(2 * BATCH_BYTES));
    let rows = vec![vec![source.as_str()], vec!["last"]];
    let index = LibrarySearchIndex::new(&rows, &CancellationToken::new()).unwrap();
    let stats = index.stats();
    assert_eq!(stats.rows, 2);
    assert_eq!(stats.fields, 2);
    assert_eq!(stats.batches, 2);
    assert_eq!(stats.source_utf8_bytes, source.len() + 4);
    assert_eq!(stats.source_utf16_units, source.encode_utf16().count() + 4);
    #[cfg(target_os = "windows")]
    assert_eq!(
        stats.retained_utf16_bytes,
        Some(stats.source_utf16_units * 2)
    );
    #[cfg(target_os = "macos")]
    assert_eq!(stats.retained_utf16_bytes, None);
    assert_eq!(search(&index, "unique-end"), vec![0]);
    assert_eq!(
        search(&index, &source),
        vec![0],
        "A batch target is not a query cap"
    );
}

#[test]
fn native_chunk_offsets_are_sorted_and_empty_indexes_remain_empty() {
    let rows: Vec<_> = (0..257)
        .map(|index| vec![if index % 3 == 0 { "MATCH" } else { "other" }])
        .collect();
    let index = LibrarySearchIndex::new(&rows, &CancellationToken::new()).unwrap();
    assert_eq!(index.stats().batches, 3);
    assert_eq!(
        search(&index, "match"),
        (0..257).step_by(3).collect::<Vec<_>>()
    );
    let empty = LibrarySearchIndex::new(&[], &CancellationToken::new()).unwrap();
    assert_eq!(empty.stats().rows, 0);
    assert_eq!(empty.stats().batches, 0);
    assert!(search(&empty, "a query").is_empty());
    assert!(search(&empty, "").is_empty());
}

#[test]
fn cancellation_and_changed_locale_never_publish_partial_matches() {
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(LibrarySearchIndex::new(&[vec!["payload"]], &cancelled).is_err());
    let index = LibrarySearchIndex::new(&[vec!["payload"]], &CancellationToken::new()).unwrap();
    let actual = locale();
    assert!(index.search("payload", &actual, &cancelled).is_err());
    let mut stale = actual.clone();
    stale.identity.push(0);
    assert!(
        index
            .search("payload", &stale, &CancellationToken::new())
            .is_err()
    );
    assert_eq!(
        index
            .search("payload", &actual, &CancellationToken::new())
            .unwrap(),
        vec![0]
    );
    assert_eq!(format!("{actual:?}"), "LibrarySearchLocale(<opaque>)");
}

#[cfg(target_os = "macos")]
#[test]
fn mac_adapter_preserves_observed_legacy_component_and_casefold_semantics() {
    for (source, query, expected) in [
        ("Straße", "STRASSE", true),
        ("sß", "ss", true),
        ("ß", "s", false),
        ("ß", "ss", true),
        ("ﬀ", "ff", true),
        ("café", "caf", true),
        ("café", "cafe", false),
        ("👩‍💻", "💻", true),
        ("👨‍👩‍👧", "👩", true),
        ("🇺🇸", "🇺", true),
        ("क्ष", "ष", false),
        ("क्ष", "क", true),
        ("का", "क", true),
        ("a\u{200d}", "a", true),
        ("a\u{200d}b", "ab", false),
        ("cafe\u{301}", "\u{301}", true),
        ("é", "\u{301}", false),
        ("left\0right", "right", true),
    ] {
        let index = LibrarySearchIndex::new(&[vec![source]], &CancellationToken::new()).unwrap();
        assert_eq!(
            !search(&index, query).is_empty(),
            expected,
            "{source:?} / {query:?}"
        );
    }
}
