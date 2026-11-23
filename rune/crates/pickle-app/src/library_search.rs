//! Immutable archive snapshots and bounded, native, off-thread library search.
use crate::{
    controller::{AppState, Route},
    storage::Entry,
};
use pickle_platform::{LibrarySearchIndex, LibrarySearchLocale, LibrarySearchStats};
use std::{
    fmt,
    ops::{Deref, DerefMut},
    sync::Arc,
    time::Instant,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub struct EntryStore {
    generation: Uuid,
    rows: Arc<Vec<Entry>>,
}
impl Default for EntryStore {
    fn default() -> Self {
        Vec::new().into()
    }
}
impl From<Vec<Entry>> for EntryStore {
    fn from(rows: Vec<Entry>) -> Self {
        Self {
            generation: Uuid::new_v4(),
            rows: Arc::new(rows),
        }
    }
}
impl fmt::Debug for EntryStore {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.debug_struct("EntryStore")
            .field("generation", &self.generation)
            .field("rows", &self.len())
            .finish()
    }
}
impl Deref for EntryStore {
    type Target = Vec<Entry>;
    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}
impl DerefMut for EntryStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.generation = Uuid::new_v4();
        Arc::make_mut(&mut self.rows)
    }
}
impl PartialEq for EntryStore {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
    }
}
impl PartialEq<Vec<Entry>> for EntryStore {
    fn eq(&self, other: &Vec<Entry>) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl PartialEq<EntryStore> for Vec<Entry> {
    fn eq(&self, other: &EntryStore) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl<'a> IntoIterator for &'a EntryStore {
    type Item = &'a Entry;
    type IntoIter = std::slice::Iter<'a, Entry>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl EntryStore {
    pub fn generation(&self) -> Uuid {
        self.generation
    }
}

#[derive(Clone, PartialEq, Eq)]
struct SearchKey {
    generation: Uuid,
    query: String,
    locale: LibrarySearchLocale,
}
#[derive(Clone)]
struct Request {
    key: SearchKey,
    snapshot: EntryStore,
}
struct Active {
    attempt: Uuid,
    key: SearchKey,
    cancel: CancellationToken,
}
struct CachedIndex {
    generation: Uuid,
    index: Arc<LibrarySearchIndex>,
}
struct Ready {
    key: SearchKey,
    rows: Arc<Vec<usize>>,
}
struct Failed {
    key: SearchKey,
    message: &'static str,
}
#[derive(Default)]
struct StoreSearch {
    desired: Option<SearchKey>,
    active: Option<Active>,
    queued: Option<Request>,
    index: Option<CachedIndex>,
    ready: Option<Ready>,
    failed: Option<Failed>,
}
#[derive(Default)]
pub struct LibrarySearchState {
    stores: [StoreSearch; 2],
    locale: Option<LibrarySearchLocale>,
    locale_error: Option<&'static str>,
    suspended: bool,
}
impl Drop for LibrarySearchState {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

pub(crate) enum SearchRows<'a> {
    All,
    Ready(&'a [usize]),
    Pending,
    Failed(&'a str),
}

pub(crate) struct SearchJob {
    bookmark: bool,
    attempt: Uuid,
    request: Request,
    index: Option<Arc<LibrarySearchIndex>>,
    cancel: CancellationToken,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct SearchMetrics {
    pub index_build_ms: f64,
    pub search_ms: f64,
    #[cfg(test)]
    pub reused_index: bool,
    pub native: Option<LibrarySearchStats>,
}
pub(crate) struct SearchCompletion {
    bookmark: bool,
    attempt: Uuid,
    key: SearchKey,
    index: Option<Arc<LibrarySearchIndex>>,
    rows: Result<Vec<usize>, &'static str>,
    metrics: SearchMetrics,
}
const NATIVE_FAILURE: &str =
    "Library search could not finish. Retry the search or clear it to browse all answers.";
const SERVICE_FAILURE: &str =
    "Library search is unavailable. Restart Pickle or clear search to browse all answers.";
const LOCALE_FAILURE: &str =
    "The system search language could not be read. Retry search or clear it to browse all answers.";

impl SearchJob {
    /// This exact CPU path is used by production workers and the explicit
    /// synthetic benchmark. No archive reads or Rune/UI operations occur here.
    pub(crate) fn run(self) -> SearchCompletion {
        let mut metrics = SearchMetrics {
            #[cfg(test)]
            reused_index: self.index.is_some(),
            ..SearchMetrics::default()
        };
        let started = Instant::now();
        let index = if self.cancel.is_cancelled() {
            None
        } else if let Some(index) = self.index {
            Some(index)
        } else {
            let rows: Vec<Vec<&str>> = self
                .request
                .snapshot
                .iter()
                .map(|entry| entry.search_fields().to_vec())
                .collect();
            LibrarySearchIndex::new(&rows, &self.cancel)
                .ok()
                .map(Arc::new)
        };
        metrics.index_build_ms = started.elapsed().as_secs_f64() * 1000.0;
        metrics.native = index.as_ref().map(|index| index.stats());
        let started = Instant::now();
        let rows = index
            .as_ref()
            .ok_or(NATIVE_FAILURE)
            .and_then(|index| {
                index
                    .search(
                        &self.request.key.query,
                        &self.request.key.locale,
                        &self.cancel,
                    )
                    .map_err(|_| NATIVE_FAILURE)
            })
            .and_then(|rows| {
                if rows.iter().any(|row| *row >= self.request.snapshot.len())
                    || rows.windows(2).any(|pair| pair[0] >= pair[1])
                {
                    Err(NATIVE_FAILURE)
                } else {
                    Ok(rows)
                }
            });
        metrics.search_ms = started.elapsed().as_secs_f64() * 1000.0;
        SearchCompletion {
            bookmark: self.bookmark,
            attempt: self.attempt,
            key: self.request.key,
            index,
            rows,
            metrics,
        }
    }
}

impl StoreSearch {
    fn invalidate(&mut self, generation: Uuid) -> bool {
        let mut changed = false;
        if self
            .index
            .as_ref()
            .is_some_and(|index| index.generation != generation)
        {
            self.index = None;
        }
        if self
            .ready
            .as_ref()
            .is_some_and(|ready| ready.key.generation != generation)
        {
            self.ready = None;
            changed = true;
        }
        if self
            .failed
            .as_ref()
            .is_some_and(|failed| failed.key.generation != generation)
        {
            self.failed = None;
            changed = true;
        }
        if self
            .desired
            .as_ref()
            .is_some_and(|key| key.generation != generation)
        {
            changed |= self.cancel();
        }
        changed
    }
    fn cancel(&mut self) -> bool {
        let changed = self.desired.take().is_some() || self.queued.is_some();
        self.queued = None;
        if let Some(active) = &self.active {
            active.cancel.cancel();
        }
        changed
    }
    fn request(
        &mut self,
        bookmark: bool,
        request: Request,
        force: bool,
    ) -> (bool, Option<SearchJob>) {
        if self
            .index
            .as_ref()
            .is_some_and(|index| index.generation != request.key.generation)
        {
            self.index = None;
        }
        let changed = self.desired.as_ref() != Some(&request.key);
        self.desired = Some(request.key.clone());
        if !force
            && (self
                .ready
                .as_ref()
                .is_some_and(|ready| ready.key == request.key)
                || self
                    .failed
                    .as_ref()
                    .is_some_and(|failed| failed.key == request.key))
        {
            return (changed, None);
        }
        self.ready = None;
        self.failed = None;
        if let Some(active) = &self.active {
            if active.key == request.key && !active.cancel.is_cancelled() && !force {
                self.queued = None;
                return (changed, None);
            }
            active.cancel.cancel();
            self.queued = Some(request);
            return (changed || force, None);
        }
        let request = self
            .queued
            .take()
            .filter(|queued| queued.key == request.key)
            .unwrap_or(request);
        let attempt = Uuid::new_v4();
        let cancel = CancellationToken::new();
        self.active = Some(Active {
            attempt,
            key: request.key.clone(),
            cancel: cancel.clone(),
        });
        let index = self.index.as_ref().map(|cached| cached.index.clone());
        (
            true,
            Some(SearchJob {
                bookmark,
                attempt,
                request,
                index,
                cancel,
            }),
        )
    }
}
impl LibrarySearchState {
    pub fn cancel_all(&mut self) {
        for store in &mut self.stores {
            store.cancel();
        }
    }
    fn rows(&self, bookmark: bool, entries: &EntryStore, query: &str) -> SearchRows<'_> {
        let query = query.trim();
        if query.is_empty() || entries.is_empty() {
            return SearchRows::All;
        }
        if let Some(message) = self.locale_error {
            return SearchRows::Failed(message);
        }
        let Some(locale) = &self.locale else {
            return SearchRows::Pending;
        };
        let store = &self.stores[usize::from(bookmark)];
        let current = |key: &SearchKey| {
            key.generation == entries.generation && key.query == query && &key.locale == locale
        };
        if let Some(ready) = &store.ready
            && current(&ready.key)
        {
            return SearchRows::Ready(&ready.rows);
        }
        if let Some(failed) = &store.failed
            && current(&failed.key)
        {
            return SearchRows::Failed(failed.message);
        }
        SearchRows::Pending
    }
}

impl AppState {
    fn search_store(&self, bookmark: bool) -> &EntryStore {
        if bookmark {
            &self.bookmarks
        } else {
            &self.entries
        }
    }
    pub(crate) fn library_search_rows(&self, bookmark: bool) -> SearchRows<'_> {
        self.library_search
            .rows(bookmark, self.search_store(bookmark), &self.search.text)
    }
    /// Polling also catches in-place fixture/app mutations and locale changes,
    /// without hashing or comparing full archive text on the UI thread.
    pub(crate) fn refresh_library_search(&mut self) -> bool {
        self.refresh_search(false)
    }
    pub fn pause_library_search(&mut self) {
        self.library_search.suspended = true;
        self.library_search.cancel_all();
    }
    pub fn resume_library_search(&mut self) {
        self.library_search.suspended = false;
        self.refresh_search(false);
    }
    pub fn retry_library_search(&mut self) {
        self.library_search.suspended = false;
        self.refresh_search(true);
    }
    fn refresh_search(&mut self, force: bool) -> bool {
        let invalidated = self.library_search.stores[0].invalidate(self.entries.generation())
            | self.library_search.stores[1].invalidate(self.bookmarks.generation());
        if self.library_search.suspended {
            return invalidated;
        }
        let Route::Library(bookmark) = self.route else {
            let mut changed = false;
            for store in &mut self.library_search.stores {
                changed |= store.cancel();
            }
            return changed || invalidated;
        };
        self.library_search.stores[usize::from(!bookmark)].cancel();
        if self.search.text.trim().is_empty() || self.search_store(bookmark).is_empty() {
            return self.library_search.stores[usize::from(bookmark)].cancel() || invalidated;
        }
        let (changed, job) = self.prepare_library_search(bookmark, force);
        if let Some(job) = job {
            self.launch_search(job);
        }
        changed || invalidated
    }
    /// Admission and worker execution are separate so deterministic headless
    /// checks can run the production worker and publish its exact completion.
    pub(crate) fn prepare_library_search(
        &mut self,
        bookmark: bool,
        force: bool,
    ) -> (bool, Option<SearchJob>) {
        let generation = self.search_store(bookmark).generation();
        let invalidated = self.library_search.stores[usize::from(bookmark)].invalidate(generation);
        if self.library_search.suspended {
            return (invalidated, None);
        }
        if self.search.text.trim().is_empty() || self.search_store(bookmark).is_empty() {
            return (
                self.library_search.stores[usize::from(bookmark)].cancel() || invalidated,
                None,
            );
        }
        let locale = match pickle_platform::library_search_locale() {
            Ok(locale) => locale,
            Err(_) => {
                let changed = self.library_search.locale_error != Some(LOCALE_FAILURE);
                self.library_search.locale = None;
                self.library_search.locale_error = Some(LOCALE_FAILURE);
                self.library_search.cancel_all();
                return (changed || invalidated, None);
            }
        };
        let locale_changed = self.library_search.locale.as_ref() != Some(&locale)
            || self.library_search.locale_error.is_some();
        self.library_search.locale = Some(locale.clone());
        self.library_search.locale_error = None;
        let request = Request {
            key: SearchKey {
                generation: self.search_store(bookmark).generation,
                query: self.search.text.trim().into(),
                locale,
            },
            snapshot: self.search_store(bookmark).clone(),
        };
        let (changed, job) =
            self.library_search.stores[usize::from(bookmark)].request(bookmark, request, force);
        (changed || locale_changed || invalidated, job)
    }
    fn launch_search(&mut self, job: SearchJob) {
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            self.search_service_failed(job);
            return;
        };
        if services.shutdown.is_cancelled() {
            self.search_service_failed(job);
            return;
        }
        let failed = SearchCompletion {
            bookmark: job.bookmark,
            attempt: job.attempt,
            key: job.request.key.clone(),
            index: None,
            rows: Err(NATIVE_FAILURE),
            metrics: SearchMetrics::default(),
        };
        services.runtime.spawn_blocking(move || {
            let completion = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.run()))
                .unwrap_or(failed);
            handle.update(move |state, _| {
                state.finish_library_search(completion);
                state.refresh_library_search();
            });
        });
    }
    fn search_service_failed(&mut self, job: SearchJob) {
        let store = &mut self.library_search.stores[usize::from(job.bookmark)];
        if store
            .active
            .as_ref()
            .is_some_and(|active| active.attempt == job.attempt)
        {
            store.active = None;
            store.failed = Some(Failed {
                key: job.request.key,
                message: SERVICE_FAILURE,
            });
            store.queued = None;
        }
    }
    pub(crate) fn finish_library_search(&mut self, completion: SearchCompletion) -> SearchMetrics {
        let index = usize::from(completion.bookmark);
        let current_generation = self.search_store(completion.bookmark).generation;
        let store = &mut self.library_search.stores[index];
        let Some(active) = &store.active else {
            return completion.metrics;
        };
        if active.attempt != completion.attempt {
            return completion.metrics;
        }
        let cancelled = active.cancel.is_cancelled();
        store.active = None;
        if current_generation == completion.key.generation
            && let Some(native) = completion.index
        {
            store.index = Some(CachedIndex {
                generation: completion.key.generation,
                index: native,
            });
        }
        if !cancelled
            && !self.library_search.suspended
            && self.route == Route::Library(completion.bookmark)
            && current_generation == completion.key.generation
            && self.search.text.trim() == completion.key.query
            && self.library_search.locale.as_ref() == Some(&completion.key.locale)
            && store.desired.as_ref() == Some(&completion.key)
        {
            match completion.rows {
                Ok(rows) => {
                    store.ready = Some(Ready {
                        key: completion.key,
                        rows: Arc::new(rows),
                    });
                    store.failed = None;
                }
                Err(message) => {
                    store.failed = Some(Failed {
                        key: completion.key,
                        message,
                    });
                    store.ready = None;
                }
            }
        }
        completion.metrics
    }
}

pub(crate) fn poll(state: &mut AppState, _: f64, _: &mut rune::Tx) -> bool {
    state.refresh_library_search()
}

#[cfg(test)]
impl AppState {
    /// Deliberate headless fixture publication, never a synchronous UI fallback.
    pub(crate) fn complete_library_search_fixture(&mut self, bookmark: bool) -> SearchMetrics {
        self.library_search.suspended = false;
        let (_, job) = self.prepare_library_search(bookmark, true);
        job.map_or(SearchMetrics::default(), |job| {
            self.finish_library_search(job.run())
        })
    }
}

#[cfg(test)]
#[path = "library_search_tests.rs"]
mod tests;
