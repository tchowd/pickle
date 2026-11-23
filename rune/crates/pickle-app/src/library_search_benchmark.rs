//! Explicit synthetic release measurements. Never opens archives or a native window.
use super::{Entry, EntryReference};
use crate::controller::{AppState, Route};
use crate::library_search::{SearchMetrics, SearchRows};
use rune::{Env, Runtime, TextEdit};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Write, process::Command, time::Instant};

const HISTORY: usize = 1000;
const SAVED: usize = 200;
const SEARCH_SAMPLES: usize = 15;
const FRAME_SAMPLES: usize = 5;
const SETTLE_FRAMES: usize = 60;
const BODY_BYTES: usize = 12_000;

#[derive(Clone, Copy)]
struct Query {
    name: &'static str,
    text: &'static str,
    desired_count: fn(usize) -> usize,
    equivalence_case: bool,
}
const QUERIES: &[Query] = &[
    Query {
        name: "empty_whitespace",
        text: " \t\n",
        desired_count: |n| n,
        equivalence_case: false,
    },
    Query {
        name: "source_tail_all",
        text: "SOURCE-END",
        desired_count: |n| n,
        equivalence_case: false,
    },
    Query {
        name: "answer_tail_one",
        text: "answer-end-0150",
        desired_count: |_| 1,
        equivalence_case: false,
    },
    Query {
        name: "question_one",
        text: "question-0150",
        desired_count: |_| 1,
        equivalence_case: false,
    },
    Query {
        name: "reference_title_one",
        text: "title-0150",
        desired_count: |_| 1,
        equivalence_case: false,
    },
    Query {
        name: "reference_url_one",
        text: "/SOURCE/0150",
        desired_count: |_| 1,
        equivalence_case: false,
    },
    Query {
        name: "unicode_case",
        text: " CAFÉ \n",
        desired_count: |n| n,
        equivalence_case: false,
    },
    Query {
        name: "absent_full_scan",
        text: "NO-SUCH-BENCHMARK-TERM-zzz",
        desired_count: |_| 0,
        equivalence_case: false,
    },
    Query {
        name: "canonical_cafe",
        text: "cafe\u{301}",
        desired_count: |n| n,
        equivalence_case: true,
    },
    Query {
        name: "full_fold_sharp_s",
        text: "STRASSE",
        desired_count: |n| n,
        equivalence_case: true,
    },
    Query {
        name: "full_fold_sigma",
        text: "οσ",
        desired_count: |n| n,
        equivalence_case: true,
    },
    Query {
        name: "canonical_reference_title",
        text: "e\u{301}le\u{301}phant",
        desired_count: |n| n,
        equivalence_case: true,
    },
];

fn filled(bytes: usize, suffix: &str) -> String {
    let unit = "Fixed synthetic saved text. Café and Straße are controlled examples. ";
    assert!(suffix.len() < bytes);
    let mut text = String::with_capacity(bytes);
    while text.len() + unit.len() + suffix.len() <= bytes {
        text.push_str(unit);
    }
    text.extend(std::iter::repeat_n('x', bytes - text.len() - suffix.len()));
    text.push_str(suffix);
    assert_eq!(text.len(), bytes);
    text
}

fn fixture() -> Vec<Entry> {
    (0..HISTORY)
        .map(|i| Entry {
            id: format!("bench-{i:04}"),
            recorded_at: "2026-01-01T00:00:00Z".into(),
            app_name: "Synthetic library fixture".into(),
            app_id: "fixture.synthetic-library".into(),
            action: "Simplify".into(),
            question: Some(filled(1200, &format!(" Greek ΟΣ question-{i:04}"))),
            passage: filled(BODY_BYTES, &format!(" source-end-{i:04}")),
            answer: filled(BODY_BYTES, &format!(" answer-end-{i:04}")),
            quality: Some("Historical fixed fixture label".into()),
            chart: None,
            reference: Some(EntryReference {
                url: format!("https://example.invalid/source/{i:04}?fixture=true"),
                title: filled(500, &format!(" Éléphant title-{i:04}")),
                kind: "article".into(),
            }),
        })
        .collect()
}

struct EncodedCounter(usize);
impl Write for EncodedCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .expect("Bounded fixture byte count");
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encoded_bytes(entries: &[Entry]) -> usize {
    #[derive(Serialize)]
    struct Archive<'a> {
        version: u32,
        data: &'a [Entry],
    }
    let mut counter = EncodedCounter(0);
    serde_json::to_writer_pretty(
        &mut counter,
        &Archive {
            version: 1,
            data: entries,
        },
    )
    .unwrap();
    assert!(
        counter.0 <= 32_000_000,
        "Fixture must fit the real encoded archive limit"
    );
    counter.0
}

fn payload_capacity(entries: &Vec<Entry>) -> usize {
    entries.capacity() * std::mem::size_of::<Entry>()
        + entries
            .iter()
            .map(|entry| {
                entry.id.capacity()
                    + entry.recorded_at.capacity()
                    + entry.app_name.capacity()
                    + entry.app_id.capacity()
                    + entry.action.capacity()
                    + entry.passage.capacity()
                    + entry.answer.capacity()
                    + entry.question.as_ref().map_or(0, String::capacity)
                    + entry.quality.as_ref().map_or(0, String::capacity)
                    + entry.chart.as_ref().map_or(0, String::capacity)
                    + entry.reference.as_ref().map_or(0, |reference| {
                        reference.url.capacity()
                            + reference.title.capacity()
                            + reference.kind.capacity()
                    })
            })
            .sum::<usize>()
}

fn stats(samples: &[f64]) -> Value {
    assert!(!samples.is_empty());
    let mut ordered = samples.to_vec();
    ordered.sort_by(f64::total_cmp);
    let percentile =
        |p: f64| ordered[((ordered.len() as f64 * p).ceil() as usize).saturating_sub(1)];
    json!({"samples":samples.len(), "p50_ms":percentile(0.50), "p95_ms":percentile(0.95),
        "worst_ms":ordered.last().unwrap(), "total_ms":samples.iter().sum::<f64>()})
}
fn millis(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
fn result_count(state: &AppState, bookmark: bool) -> usize {
    match state.library_search_rows(bookmark) {
        SearchRows::All => {
            if bookmark {
                state.bookmarks.len()
            } else {
                state.entries.len()
            }
        }
        SearchRows::Ready(rows) => rows.len(),
        SearchRows::Pending => panic!("Production search worker left this fixed request pending"),
        SearchRows::Failed(_) => panic!("Production native search failed for the fixed fixture"),
    }
}
fn native_stats(metrics: SearchMetrics) -> Value {
    metrics.native.map_or(Value::Null, |stats| {
        json!({"rows":stats.rows,"fields":stats.fields,
        "source_utf8_bytes":stats.source_utf8_bytes,"source_utf16_units":stats.source_utf16_units,
        "retained_utf16_bytes":stats.retained_utf16_bytes,"batches":stats.batches})
    })
}
fn execute(state: &mut AppState, bookmark: bool, query: &str) -> SearchMetrics {
    state.route = Route::Library(bookmark);
    state.search = TextEdit::new(query);
    let (_, job) = state.prepare_library_search(bookmark, true);
    job.map_or(SearchMetrics::default(), |job| {
        state.finish_library_search(job.run())
    })
}
fn search_measurements(state: &mut AppState, bookmark: bool) -> Value {
    let count = if bookmark {
        state.bookmarks.len()
    } else {
        state.entries.len()
    };
    let mut samples: Vec<Vec<f64>> = (0..QUERIES.len()).map(|_| vec![]).collect();
    let mut observed = vec![0; QUERIES.len()];
    let mut construction = vec![];
    let mut native = Value::Null;
    // Interleave queries rather than timing one class only after another heats the host.
    for _ in 0..SEARCH_SAMPLES {
        for (i, query) in QUERIES.iter().enumerate() {
            let started = Instant::now();
            let metrics = execute(state, bookmark, query.text);
            let worker_ms = millis(started);
            if metrics.native.is_some() {
                native = native_stats(metrics);
                if !metrics.reused_index {
                    construction.push(metrics.index_build_ms);
                }
            }
            samples[i].push(if query.text.trim().is_empty() {
                worker_ms
            } else {
                metrics.search_ms
            });
            observed[i] = result_count(state, bookmark);
        }
    }
    let rows: Vec<_> = QUERIES
        .iter()
        .enumerate()
        .map(|(i, query)| {
            let desired = (query.desired_count)(count);
            if !query.equivalence_case {
                assert_eq!(observed[i], desired, "{}", query.name);
            }
            json!({"query":query.name,"query_text":query.text,"observed_matches":observed[i],
            "desired_matches":desired,"desired_equivalence_case":query.equivalence_case,
            "correctness_matches_desired":observed[i]==desired,"timing":stats(&samples[i])})
        })
        .collect();
    let all: Vec<_> = samples.into_iter().flatten().collect();
    json!({"method":"actual production SearchJob::run with reused immutable native index; no unary Entry::matches calls",
        "aggregate":stats(&all),"queries":rows,"index_build_ms":construction,"native_index":native})
}

fn cards(tree: &Value) -> usize {
    usize::from(
        tree["key"]
            .as_str()
            .is_some_and(|key| key.starts_with("entry-bench-")),
    ) + tree["children"]
        .as_array()
        .map_or(0, |children| children.iter().map(cards).sum())
}
fn runtime_measurements(state: AppState) -> Value {
    let started = Instant::now();
    assert!(!state.native_enabled && state.services.is_none() && state.snapshot.is_none());
    let mut runtime = Runtime::new(
        state,
        crate::views::view,
        (560.0, 900.0),
        Env::new(true, 1.0),
    );
    runtime.frame(1.0 / 60.0);
    let cold_ms = millis(started);
    for _ in 0..SETTLE_FRAMES {
        runtime.frame(1.0 / 60.0);
    }
    let mut stores = vec![];
    for bookmark in [false, true] {
        let mut rows = vec![];
        for query in QUERIES {
            let mut updates = vec![];
            let mut frames = vec![];
            let mut transitions = vec![];
            let mut workers = vec![];
            let mut completions = vec![];
            let mut idle = vec![];
            for _ in 0..FRAME_SAMPLES {
                let transition = Instant::now();
                let update = Instant::now();
                // update() marks the view dirty; direct state mutation would
                // falsely measure only an unchanged retained-tree frame.
                let mut job = None;
                runtime.update(|state, _| {
                    state.route = Route::Library(bookmark);
                    state.search = TextEdit::new(query.text);
                    state.library_page = 0;
                    state.library_selected = None;
                    job = state.prepare_library_search(bookmark, true).1;
                });
                updates.push(millis(update));
                let frame = Instant::now();
                runtime.frame(1.0 / 60.0);
                frames.push(millis(frame));
                let worker = Instant::now();
                let completion = job.map(|job| job.run());
                workers.push(millis(worker));
                let completion_frame = Instant::now();
                if let Some(completion) = completion {
                    runtime.update(|state, _| {
                        state.finish_library_search(completion);
                    });
                }
                runtime.frame(1.0 / 60.0);
                completions.push(millis(completion_frame));
                transitions.push(millis(transition));
                let steady = Instant::now();
                runtime.frame(1.0 / 60.0);
                idle.push(millis(steady));
            }
            for _ in 0..SETTLE_FRAMES {
                runtime.frame(1.0 / 60.0);
            }
            assert_eq!(runtime.state.search.text, query.text);
            assert!(runtime.find("search-library").is_some());
            let card_count = cards(&runtime.accessibility_json());
            let expected = result_count(&runtime.state, bookmark);
            assert_eq!(
                card_count,
                expected.min(20),
                "Actual library filter/pagination: {}",
                query.name
            );
            rows.push(json!({"query":query.name,"observed_matches":expected,"rendered_cards":card_count,
                "update":stats(&updates),"first_frame":stats(&frames),"query_transition":stats(&transitions),
                "worker":stats(&workers),"completion_frame":stats(&completions),"unchanged_frame":stats(&idle)}));
        }
        stores.push(json!({"store":if bookmark {"saved"} else {"history"},"queries":rows}));
    }
    assert!(
        runtime.state.services.is_none()
            && runtime.state.handle.is_none()
            && !runtime.state.native_enabled
    );
    json!({"cold_runtime_and_first_frame_ms":cold_ms,"viewport_logical":[560,900],
        "method":"Production admission plus pending Runtime frame, SearchJob::run executed separately, explicit completion plus Runtime frame; no AppHandle queue drain, GPU submission or native input. query_transition includes worker CPU time, not async queue wait.",
        "settle_frames_excluded_from_query_timing":SETTLE_FRAMES,"stores":stores})
}

#[test]
#[ignore = "Explicit synthetic release-only library search and headless Runtime performance baseline"]
fn release_library_search_baseline() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("Run this explicit ignored test with cargo test --release");
    }
    let wall = Instant::now();
    let preparation = Instant::now();
    let history = fixture();
    let saved = history[..SAVED].to_vec();
    assert_eq!(history.len(), HISTORY);
    assert_eq!(saved.len(), SAVED);
    for entry in history.iter().chain(&saved) {
        entry.validate().unwrap();
    }
    let history_encoded = encoded_bytes(&history);
    let saved_encoded = encoded_bytes(&saved);
    assert!(history_encoded + saved_encoded <= 32_000_000);
    let fixture_info = json!({"history_entries":history.len(),"saved_entries":saved.len(),
        "history_encoded_pretty_archive_bytes":history_encoded,
        "saved_encoded_pretty_archive_bytes":saved_encoded,
        "combined_encoded_pretty_archive_bytes":history_encoded+saved_encoded,
        "passage_bytes_per_entry":BODY_BYTES,"answer_bytes_per_entry":BODY_BYTES,
        "estimated_owned_entry_heap_bytes":payload_capacity(&history)+payload_capacity(&saved),
        "memory_estimate_scope":"Vec<Entry> allocation plus owned String capacities only; excludes allocator overhead, temporary lowercase strings, Runtime/font caches, process RSS and GPU memory"});
    let preparation_ms = millis(preparation);
    let searches = Instant::now();
    let mut state = AppState {
        route: Route::Library(false),
        entries: history.into(),
        bookmarks: saved.into(),
        ..AppState::default()
    };
    let history_search = search_measurements(&mut state, false);
    let saved_search = search_measurements(&mut state, true);
    let search_wall_ms = millis(searches);
    let render = Instant::now();
    let runtime = runtime_measurements(state);
    let runtime_wall_ms = millis(render);
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| {
            matches!(text.len(), 40 | 64) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
    let report = json!({"schema":1,"test":"release-library-search-baseline","source_commit":commit,
        "build_reported_commit":env!("PICKLE_BUILD_COMMIT"),
        "compiled_storage_source_sha256":hex::encode(Sha256::digest(include_bytes!("storage.rs"))),
        "compiled_view_source_sha256":hex::encode(Sha256::digest(include_bytes!("views.rs"))),
        "compiled_app_search_source_sha256":hex::encode(Sha256::digest(include_bytes!("library_search.rs"))),
        "compiled_native_search_source_sha256":hex::encode(Sha256::digest(include_bytes!("../../pickle-platform/src/library_search.rs"))),
        "compiled_swift_search_source_sha256":hex::encode(Sha256::digest(include_bytes!("../../../native/macos/PickleLibrarySearch.swift"))),
        "compiled_benchmark_source_sha256":hex::encode(Sha256::digest(include_bytes!("library_search_benchmark.rs"))),
        "source_provenance":"Git HEAD with compiled-source hashes; may include uncommitted development changes, not a clean-commit attestation",
        "configuration":"release","os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
        "available_parallelism":std::thread::available_parallelism().map(|n|n.get()).ok(),
        "native_window":false,"repository_reads":0,"provider_requests":0,"fixture":fixture_info,
        "preparation_ms":preparation_ms,"search_wall_ms":search_wall_ms,"runtime_wall_ms":runtime_wall_ms,
        "search":{"history":history_search,"saved":saved_search},"runtime":runtime,"wall_ms":millis(wall),
        "interpretation":"Timing completion is not Unicode correctness. Equivalence desired counts describe Foundation-reference behavior; actual Windows NLS differences are reported rather than assumed failures. No hardware-dependent performance pass threshold or frame-rate guarantee."});
    println!(
        "PICKLE_LIBRARY_SEARCH_BENCHMARK {}",
        serde_json::to_string(&report).unwrap()
    );
    Ok(())
}
