use super::*;
use crate::storage::{EntryReference, Repository};
use rune::{Env, Runtime, TextEdit};

fn entry(id: &str, passage: &str) -> Entry {
    Entry {
        id: id.into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        app_name: "Fixture".into(),
        app_id: "fixture".into(),
        action: "Simplify".into(),
        question: None,
        passage: passage.into(),
        answer: "Controlled answer".into(),
        quality: None,
        chart: None,
        reference: None,
    }
}

#[test]
fn immutable_store_shares_snapshots_and_versions_every_mutable_access() {
    let mut store: EntryStore = vec![entry("a", "Original")].into();
    let snapshot = store.clone();
    assert!(Arc::ptr_eq(&store.rows, &snapshot.rows));
    let generation = store.generation();
    store[0].passage = "Replacement".into();
    assert_ne!(generation, store.generation());
    assert!(!Arc::ptr_eq(&store.rows, &snapshot.rows));
    assert_eq!(snapshot[0].passage, "Original");
    let generation = store.generation();
    store.push(entry("b", "Second"));
    assert_ne!(generation, store.generation());
    let generation = store.generation();
    store.clear();
    assert_ne!(generation, store.generation());
    assert_eq!(snapshot.len(), 1);
}

fn state(rows: Vec<Entry>, bookmark: bool) -> AppState {
    AppState {
        route: Route::Library(bookmark),
        entries: if bookmark { vec![] } else { rows.clone() }.into(),
        bookmarks: if bookmark { rows } else { vec![] }.into(),
        ..AppState::default()
    }
}
fn ready(state: &AppState, bookmark: bool) -> Vec<usize> {
    match state.library_search_rows(bookmark) {
        SearchRows::Ready(rows) => rows.to_vec(),
        SearchRows::All => (0..state.search_store(bookmark).len()).collect(),
        _ => panic!("Fixed search should be complete"),
    }
}
fn execute(state: &mut AppState, bookmark: bool, query: &str) -> SearchMetrics {
    state.search = TextEdit::new(query);
    let (_, job) = state.prepare_library_search(bookmark, false);
    job.map_or(SearchMetrics::default(), |job| {
        let completion = std::thread::spawn(move || job.run()).join().unwrap();
        state.finish_library_search(completion)
    })
}

#[test]
fn reopened_archives_use_native_six_field_search_without_cross_field_or_whitespace_folding() {
    for bookmark in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut record = entry("one", "Café Straße alpha  beta cross");
        record.answer = "boundary unique-answer".into();
        record.app_name = "Éditeur".into();
        record.question = Some("target-question".into());
        record.reference = Some(EntryReference {
            title: "Éléphant reference-title".into(),
            url: "https://example.invalid/reference-target".into(),
            kind: "article".into(),
        });
        let repo = Repository::new(directory.path().into()).unwrap();
        repo.append(bookmark, record).unwrap();
        drop(repo);
        let path = directory.path().join(if bookmark {
            "bookmarks.json"
        } else {
            "history.json"
        });
        let before = std::fs::read(&path).unwrap();
        let repo = Repository::new(directory.path().into()).unwrap();
        let mut state = state(
            if bookmark {
                repo.bookmarks().unwrap()
            } else {
                repo.history().unwrap()
            },
            bookmark,
        );
        for query in [
            " \tcafe\u{301}\n",
            "STRAßE",
            "unique-answer",
            "e\u{301}diteur",
            "target-question",
            "e\u{301}le\u{301}phant",
            "EXAMPLE.INVALID/REFERENCE-TARGET",
        ] {
            execute(&mut state, bookmark, query);
            assert_eq!(ready(&state, bookmark), vec![0], "Fixed field query");
        }
        // This integration checks the captured platform's native collation,
        // not an assumption that Windows NLS equals Foundation's sharp-s fold.
        let cancel = CancellationToken::new();
        let locale = pickle_platform::library_search_locale().unwrap();
        let oracle = LibrarySearchIndex::new(&[vec!["Straße"]], &cancel)
            .unwrap()
            .search("STRASSE", &locale, &cancel)
            .unwrap();
        execute(&mut state, bookmark, "STRASSE");
        assert_eq!(ready(&state, bookmark), oracle);
        for query in [
            "alpha beta",
            "cross boundary",
            "video captions",
            "ABSENT_SENTINEL",
        ] {
            execute(&mut state, bookmark, query);
            assert!(ready(&state, bookmark).is_empty());
        }
        execute(&mut state, bookmark, " \t\n");
        assert_eq!(ready(&state, bookmark), vec![0]);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(state.services.is_none() && state.handle.is_none());
    }
}

#[test]
fn rapid_queries_keep_one_active_and_only_latest_queued_then_discard_stale_callbacks() {
    let mut state = state(
        vec![entry("a", "one"), entry("b", "two"), entry("c", "three")],
        false,
    );
    state.search = TextEdit::new("one");
    let (_, first) = state.prepare_library_search(false, false);
    let first = first.unwrap();
    let first_attempt = first.attempt;
    let first_key = first.request.key.clone();
    state.search = TextEdit::new("two");
    assert!(state.prepare_library_search(false, false).1.is_none());
    state.search = TextEdit::new("three");
    assert!(state.prepare_library_search(false, false).1.is_none());
    let store = &state.library_search.stores[0];
    assert_eq!(store.active.as_ref().unwrap().attempt, first_attempt);
    assert!(first.cancel.is_cancelled());
    assert_eq!(store.queued.as_ref().unwrap().key.query, "three");
    assert!(Arc::ptr_eq(
        &store.queued.as_ref().unwrap().snapshot.rows,
        &state.entries.rows
    ));
    state.finish_library_search(first.run());
    assert!(matches!(
        state.library_search_rows(false),
        SearchRows::Pending
    ));
    let (_, latest) = state.prepare_library_search(false, false);
    let latest = latest.unwrap();
    let latest_attempt = latest.attempt;
    state.finish_library_search(SearchCompletion {
        bookmark: false,
        attempt: first_attempt,
        key: first_key,
        index: None,
        rows: Ok(vec![0]),
        metrics: SearchMetrics::default(),
    });
    assert_eq!(
        state.library_search.stores[0]
            .active
            .as_ref()
            .unwrap()
            .attempt,
        latest_attempt
    );
    state.finish_library_search(latest.run());
    assert_eq!(ready(&state, false), vec![2]);
    assert!(state.library_search.stores[0].queued.is_none());
}

#[test]
fn same_query_cache_and_native_index_reuse_never_copy_archive_on_keystrokes() {
    let mut state = state(vec![entry("a", &"Café ".repeat(1800))], false);
    let snapshot = state.entries.clone();
    let first = execute(&mut state, false, "Café");
    assert!(!first.reused_index);
    assert_eq!(ready(&state, false), vec![0]);
    let index = state.library_search.stores[0]
        .index
        .as_ref()
        .unwrap()
        .index
        .clone();
    assert!(state.prepare_library_search(false, false).1.is_none());
    state.search = TextEdit::new("absent");
    let (_, job) = state.prepare_library_search(false, false);
    let job = job.unwrap();
    assert!(Arc::ptr_eq(job.index.as_ref().unwrap(), &index));
    assert!(Arc::ptr_eq(&job.request.snapshot.rows, &snapshot.rows));
    let metrics = state.finish_library_search(job.run());
    assert!(metrics.reused_index);
    assert!(ready(&state, false).is_empty());
    assert!(Arc::ptr_eq(&state.entries.rows, &snapshot.rows));
}

#[test]
fn in_place_mutation_and_removed_rows_invalidate_results_before_worker_publication() {
    let mut state = state(
        vec![entry("a", "needle"), entry("b", "other needle")],
        false,
    );
    execute(&mut state, false, "needle");
    assert_eq!(ready(&state, false), vec![0, 1]);
    let snapshot = state.entries.clone();
    let generation = state.entries.generation();
    state.entries[0].passage = "replacement".into();
    assert_ne!(generation, state.entries.generation());
    assert!(matches!(
        state.library_search_rows(false),
        SearchRows::Pending
    ));
    let (_, job) = state.prepare_library_search(false, false);
    let job = job.unwrap();
    assert!(job.index.is_none());
    state.entries.remove(1);
    state.finish_library_search(job.run());
    assert!(matches!(
        state.library_search_rows(false),
        SearchRows::Pending
    ));
    execute(&mut state, false, "needle");
    assert!(ready(&state, false).is_empty());
    assert_eq!(snapshot.len(), 2);
    assert_eq!(snapshot[0].passage, "needle");
    state.entries.clear();
    state.refresh_library_search();
    assert!(state.library_search.stores[0].index.is_none());
    assert!(state.library_search.stores[0].ready.is_none());
    assert!(matches!(state.library_search_rows(false), SearchRows::All));
}

#[test]
fn cancelled_constructor_does_not_poison_retry_or_publish_partial_rows() {
    let mut state = state(vec![entry("a", "Café")], false);
    state.search = TextEdit::new("cafe\u{301}");
    let (_, job) = state.prepare_library_search(false, false);
    let job = job.unwrap();
    job.cancel.cancel();
    state.finish_library_search(job.run());
    assert!(state.library_search.stores[0].index.is_none());
    assert!(matches!(
        state.library_search_rows(false),
        SearchRows::Pending
    ));
    let (_, retry) = state.prepare_library_search(false, false);
    let metrics = state.finish_library_search(retry.unwrap().run());
    assert!(!metrics.reused_index);
    assert_eq!(ready(&state, false), vec![0]);
}

#[test]
fn mutations_of_every_searchable_field_invalidate_the_same_query_cache() {
    for field in 0..6 {
        let mut record = entry("a", "first");
        record.reference = Some(EntryReference {
            title: "title".into(),
            url: "https://example.invalid/".into(),
            kind: "article".into(),
        });
        let mut state = state(vec![record], false);
        execute(&mut state, false, "needle");
        assert!(ready(&state, false).is_empty());
        let generation = state.entries.generation();
        match field {
            0 => state.entries[0].passage = "needle".into(),
            1 => state.entries[0].answer = "needle".into(),
            2 => state.entries[0].app_name = "needle".into(),
            3 => state.entries[0].question = Some("needle".into()),
            4 => state.entries[0].reference.as_mut().unwrap().title = "needle".into(),
            _ => {
                state.entries[0].reference.as_mut().unwrap().url =
                    "https://example.invalid/needle".into()
            }
        }
        assert_ne!(generation, state.entries.generation());
        assert!(matches!(
            state.library_search_rows(false),
            SearchRows::Pending
        ));
        let metrics = execute(&mut state, false, "needle");
        assert!(!metrics.reused_index);
        assert_eq!(ready(&state, false), vec![0]);
    }
}

#[test]
fn switching_stores_cancels_old_work_without_publishing_or_changing_archives() {
    let mut state = state(vec![entry("history", "Café")], false);
    state.bookmarks = vec![entry("saved", "Saved passage")].into();
    state.search = TextEdit::new("Café");
    let (_, history) = state.prepare_library_search(false, false);
    let history = history.unwrap();
    state.route = Route::Library(true);
    state.search = TextEdit::new("SAVED PASSAGE");
    state.refresh_library_search();
    assert!(history.cancel.is_cancelled());
    let (_, saved) = state.prepare_library_search(true, true);
    let saved = saved.unwrap();
    state.finish_library_search(history.run());
    assert!(state.library_search.stores[0].ready.is_none());
    state.finish_library_search(saved.run());
    assert_eq!(ready(&state, true), vec![0]);
    assert_eq!(state.entries[0].id, "history");
    assert_eq!(state.bookmarks[0].id, "saved");
}

#[test]
fn native_failure_is_explicit_not_an_empty_result_and_retry_can_build_again() {
    let mut state = state(vec![entry("a", "Café")], false);
    state.search = TextEdit::new("Café");
    let (_, job) = state.prepare_library_search(false, false);
    let mut failure = job.unwrap().run();
    failure.index = None;
    failure.rows = Err(NATIVE_FAILURE);
    state.finish_library_search(failure);
    assert!(matches!(
        state.library_search_rows(false),
        SearchRows::Failed(_)
    ));
    let mut runtime = Runtime::new(
        state,
        crate::views::view,
        (560.0, 900.0),
        Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    assert!(runtime.find("library-search-error").is_some());
    assert!(runtime.find("library-search-no-matches").is_none());
    assert!(runtime.find("library-empty-store").is_none());
    runtime.update(|state, _| {
        state.complete_library_search_fixture(false);
    });
    runtime.frame(0.0);
    assert_eq!(ready(&runtime.state, false), vec![0]);
    assert!(runtime.find("library-search-error").is_none());
    assert!(runtime.find("entry-a").is_some());
}

#[test]
fn pending_and_empty_store_are_distinct_from_no_matches_without_native_or_service_fallback() {
    let mut state = state(vec![entry("a", "Café")], false);
    state.search = TextEdit::new("Café");
    let mut runtime = Runtime::new(
        state,
        crate::views::view,
        (560.0, 900.0),
        Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    assert!(runtime.find("library-search-pending").is_some());
    assert!(runtime.find("library-search-no-matches").is_none());
    runtime.update(|state, _| {
        state.refresh_library_search();
    });
    runtime.frame(0.0);
    assert!(runtime.find("library-search-error").is_some());
    assert!(runtime.find("library-search-no-matches").is_none());
    runtime.update(|state, _| {
        state.entries.clear();
        state.refresh_library_search();
    });
    runtime.frame(0.0);
    assert!(runtime.find("library-empty-store").is_some());
    assert!(runtime.find("library-search-error").is_none());
    assert!(runtime.find("library-search-pending").is_none());
}

#[test]
fn leaving_library_and_dropping_state_cancel_owned_searches() {
    let mut state = state(vec![entry("a", "Café")], false);
    state.search = TextEdit::new("Café");
    let (_, job) = state.prepare_library_search(false, false);
    let job = job.unwrap();
    state.route = Route::Reader;
    state.refresh_library_search();
    assert!(job.cancel.is_cancelled());
    state.finish_library_search(job.run());
    assert!(state.library_search.stores[0].ready.is_none());
    state.route = Route::Library(false);
    let (_, job) = state.prepare_library_search(false, false);
    let job = job.unwrap();
    let cancel = job.cancel.clone();
    drop(state);
    assert!(cancel.is_cancelled());
    assert!(job.run().rows.is_err());
}

#[test]
fn closing_reader_cannot_rearm_search_from_poll_or_late_completion_until_reopened() {
    let mut state = state(vec![entry("a", "Café")], false);
    state.search = TextEdit::new("Café");
    let (_, job) = state.prepare_library_search(false, false);
    let job = job.unwrap();
    state.prepare_window_close();
    assert!(job.cancel.is_cancelled());
    state.finish_library_search(job.run());
    for _ in 0..3 {
        state.refresh_library_search();
    }
    assert!(state.library_search.stores[0].active.is_none());
    assert!(state.library_search.stores[0].queued.is_none());
    assert!(state.prepare_library_search(false, false).1.is_none());
    state.resume_library_search();
    // There are no services in this owned fixture; real reopen admission is
    // explicit and fails visibly instead of falling back to synchronous search.
    assert!(matches!(
        state.library_search_rows(false),
        SearchRows::Failed(_)
    ));
    state.complete_library_search_fixture(false);
    assert_eq!(ready(&state, false), vec![0]);
}
