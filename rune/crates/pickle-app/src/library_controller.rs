//! Explicit controls admit one operation per archive; accepted History writes
//! form a bounded ordered chain. Only its latest operation publishes UI state.
//! Automatic History entries wait behind an active operation so a confirmed
//! delete cannot erase an answer that arrived after the reviewed snapshot.
use super::*;
use std::sync::{Condvar, Mutex};

#[derive(Default)]
struct Completion {
    finished: Mutex<Option<Failures>>,
    ready: Condvar,
}
#[derive(Clone, Default)]
struct Failures {
    read: Option<String>,
    operation: Option<String>,
}
impl Completion {
    fn wait(&self) -> Failures {
        let mut finished = self.finished.lock().unwrap_or_else(|p| p.into_inner());
        while finished.is_none() {
            finished = self.ready.wait(finished).unwrap_or_else(|p| p.into_inner());
        }
        finished.clone().unwrap_or_default()
    }
}
struct CompleteOnDrop(Arc<Completion>);
impl Drop for CompleteOnDrop {
    fn drop(&mut self) {
        let mut finished = self.0.finished.lock().unwrap_or_else(|p| p.into_inner());
        if finished.is_none() {
            *finished = Some(Failures {
                read: Some("A library operation did not complete. Retry reading this archive before making changes.".into()),
                operation: None,
            });
        }
        self.0.ready.notify_all();
    }
}

/// Every admitted operation is submitted to the tracked blocking executor now,
/// including its predecessor wait. Runtime shutdown therefore drains queued
/// History writes without depending on another UI callback being delivered.
struct Reservation {
    id: uuid::Uuid,
    previous: Option<Arc<Completion>>,
    _completion: CompleteOnDrop,
}
impl Reservation {
    fn wait(&self) -> Failures {
        self.previous
            .as_ref()
            .map(|previous| previous.wait())
            .unwrap_or_default()
    }
    fn complete(&self, outcome: &mut LibraryOutcome, previous: Failures) {
        // A newer successful read cannot silently clear an earlier unreadable
        // result in this admitted chain. A fresh operation after that chain
        // publishes may retry the current archive; these queued writes may not.
        if outcome.entries.is_ok()
            && let Some(error) = previous.read
        {
            outcome.entries = Err(error);
        }
        outcome.operation_error = outcome.operation_error.take().or(previous.operation);
        *self
            ._completion
            .0
            .finished
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(Failures {
            read: outcome.entries.as_ref().err().cloned(),
            operation: outcome.operation_error.clone(),
        });
    }
}

#[derive(Default)]
pub struct LibraryState {
    pub(crate) operations: [Option<uuid::Uuid>; 2],
    delete_snapshot: Option<crate::library_search::EntryStore>,
    tails: [Option<Arc<Completion>>; 2],
    queued_history: usize,
    notices: [Option<String>; 2],
}

pub(crate) enum LibraryOperation {
    Append(Box<Entry>),
    Remove(String),
    RemoveAll(crate::library_search::EntryStore),
    Reload,
}

pub(crate) struct LibraryOutcome {
    pub entries: std::result::Result<Vec<Entry>, String>,
    pub operation_error: Option<String>,
}

pub(crate) fn read_library(
    repo: &Repository,
    bookmark: bool,
) -> std::result::Result<Vec<Entry>, String> {
    if bookmark {
        repo.bookmarks()
    } else {
        repo.history()
    }
}

pub(crate) fn perform_library_operation(
    repo: &Repository,
    bookmark: bool,
    operation: LibraryOperation,
) -> LibraryOutcome {
    let result = match operation {
        LibraryOperation::Append(entry) => repo.append(bookmark, *entry),
        LibraryOperation::Remove(id) => repo.remove(bookmark, &id),
        LibraryOperation::RemoveAll(expected) => repo.remove_all_if_matches(bookmark, &expected),
        LibraryOperation::Reload => Ok(()),
    };
    LibraryOutcome {
        entries: read_library(repo, bookmark),
        operation_error: result.err(),
    }
}

impl AppState {
    pub fn library_error(&self, bookmark: bool) -> Option<&str> {
        self.library_errors[usize::from(bookmark)].as_deref()
    }
    pub fn library_busy(&self, bookmark: bool) -> bool {
        self.library_state.operations[usize::from(bookmark)].is_some()
    }
    fn library_entries(&self, bookmark: bool) -> &[Entry] {
        if bookmark {
            &self.bookmarks
        } else {
            &self.entries
        }
    }
    fn library_services(&self) -> Option<(Arc<Services>, AppHandle<Self>, Arc<Repository>)> {
        let services = self.services.clone()?;
        if services.shutdown.is_cancelled() {
            return None;
        }
        Some((
            services.clone(),
            self.handle.clone()?,
            services.repository.clone()?,
        ))
    }
    fn library_writable(&self, bookmark: bool) -> bool {
        !self.library_busy(bookmark) && self.library_error(bookmark).is_none()
    }
    fn library_notice(&mut self, bookmark: bool, notice: String) {
        self.library_state.notices[usize::from(bookmark)] = Some(notice.clone());
        self.notice = Some(notice);
    }
    pub fn request_remove_all(&mut self, bookmark: bool) {
        if !self.library_writable(bookmark) || self.library_entries(bookmark).is_empty() {
            return;
        }
        self.library_state.delete_snapshot = Some(if bookmark {
            self.bookmarks.clone()
        } else {
            self.entries.clone()
        });
        self.library_delete_confirmation = Some(bookmark);
    }
    pub fn cancel_remove_all(&mut self) {
        self.library_delete_confirmation = None;
        self.library_state.delete_snapshot = None;
    }
    pub fn confirm_remove_all(&mut self) {
        let Some(bookmark) = self.library_delete_confirmation else {
            return;
        };
        if !self.library_writable(bookmark) || self.library_services().is_none() {
            self.cancel_remove_all();
            return;
        }
        let Some(expected) = self.library_state.delete_snapshot.take() else {
            self.cancel_remove_all();
            return;
        };
        self.cancel_remove_all();
        self.start_library_operation(
            bookmark,
            LibraryOperation::RemoveAll(expected),
            Some("All answers in this library were deleted from this device."),
        );
    }
    pub fn reload_library(&mut self, bookmark: bool) {
        if self.library_busy(bookmark) {
            return;
        }
        self.start_library_operation(bookmark, LibraryOperation::Reload, None);
    }
    pub fn reveal_library(&mut self, bookmark: bool) {
        if !self.native_enabled || self.library_busy(bookmark) {
            return;
        }
        let Some((services, handle, repo)) = self.library_services() else {
            return;
        };
        let reservation = self.begin_library_operation(bookmark);
        services.runtime.spawn_blocking(move || {
            let previous = reservation.wait();
            let path = repo.directory().join(if bookmark {
                "bookmarks.json"
            } else {
                "history.json"
            });
            let outcome = if path.is_file() {
                crate::browser_setup::reveal(&path)
            } else {
                Err("This archive file is not present. No file was created.".into())
            };
            *reservation
                ._completion
                .0
                .finished
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = Some(Failures {
                read: previous.read,
                operation: outcome.as_ref().err().cloned().or(previous.operation),
            });
            let operation = reservation.id;
            drop(reservation);
            handle.update(move |state, _| {
                if state.library_state.operations[usize::from(bookmark)] != Some(operation) {
                    return;
                }
                state.library_state.operations[usize::from(bookmark)] = None;
                state.library_state.tails[usize::from(bookmark)] = None;
                if !bookmark {
                    state.library_state.queued_history = 0;
                }
                if let Err(error) = outcome {
                    state.library_notice(bookmark, error);
                }
            });
        });
    }
    pub(super) fn append_saved_entry(&mut self, entry: Entry) {
        if self.library_busy(true) {
            self.library_notice(
                true,
                "Saved is busy. Wait for the current operation, then save this answer again."
                    .into(),
            );
            return;
        }
        self.start_library_operation(
            true,
            LibraryOperation::Append(Box::new(entry)),
            Some("Answer saved on this device."),
        );
    }
    pub(super) fn remove_library_entry(&mut self, bookmark: bool, id: String) {
        if !self.library_writable(bookmark)
            || !self
                .library_entries(bookmark)
                .iter()
                .any(|entry| entry.id == id)
        {
            return;
        }
        self.start_library_operation(bookmark, LibraryOperation::Remove(id), None);
    }
    pub(super) fn record_history_entry(&mut self, entry: Entry) {
        if self.library_busy(false) {
            if self.library_delete_confirmation == Some(false) {
                self.cancel_remove_all();
            }
            if self.library_state.queued_history >= 16 {
                self.library_notice(false, "The knowledge base is still busy. This answer was not recorded; you can save it explicitly.".into());
                return;
            }
            self.library_state.queued_history += 1;
        }
        self.start_library_operation(false, LibraryOperation::Append(Box::new(entry)), None);
    }
    fn begin_library_operation(&mut self, bookmark: bool) -> Reservation {
        if self.library_delete_confirmation == Some(bookmark) {
            self.cancel_remove_all();
        }
        let id = uuid::Uuid::new_v4();
        let completion = Arc::new(Completion::default());
        let index = usize::from(bookmark);
        let previous = self.library_state.tails[index].replace(completion.clone());
        self.library_state.operations[index] = Some(id);
        Reservation {
            id,
            previous,
            _completion: CompleteOnDrop(completion),
        }
    }
    fn start_library_operation(
        &mut self,
        bookmark: bool,
        work: LibraryOperation,
        notice: Option<&'static str>,
    ) {
        let Some((services, handle, repo)) = self.library_services() else {
            return;
        };
        let reload = matches!(work, LibraryOperation::Reload);
        let reservation = self.begin_library_operation(bookmark);
        services.runtime.spawn_blocking(move || {
            let previous = reservation.wait();
            let mut outcome = if let Some(error) = previous.read.clone() {
                LibraryOutcome {
                    entries: Err(error),
                    operation_error: Some("This library was not changed because a preceding archive read failed. Retry reading the archive first.".into()),
                }
            } else {
                perform_library_operation(&repo, bookmark, work)
            };
            reservation.complete(&mut outcome, previous);
            let operation = reservation.id;
            drop(reservation);
            handle.update(move |state, _| {
                state.finish_library_operation(bookmark, operation, outcome, reload, notice);
            });
        });
    }
    pub(crate) fn finish_library_operation(
        &mut self,
        bookmark: bool,
        operation: uuid::Uuid,
        outcome: LibraryOutcome,
        reload: bool,
        success_notice: Option<&str>,
    ) {
        let index = usize::from(bookmark);
        if self.library_state.operations[index] != Some(operation) {
            return;
        }
        let preserve_unrelated_notice = self.library_errors[index].is_some()
            && self.notice.is_some()
            && self.notice != self.library_errors[index]
            && self.notice != self.library_state.notices[index];
        self.library_state.operations[index] = None;
        self.library_state.tails[index] = None;
        if !bookmark {
            self.library_state.queued_history = 0;
        }
        match outcome.entries {
            Ok(entries) => {
                let changed = self.library_entries(bookmark) != entries;
                if changed && self.library_delete_confirmation == Some(bookmark) {
                    self.cancel_remove_all();
                }
                if changed {
                    if bookmark {
                        self.bookmarks = entries.into();
                    } else {
                        self.entries = entries.into();
                    }
                }
                // Every operation rereads and validates the current archive.
                // Recover after a fresh append too, without replaying any
                // failed answer or bypassing a preceding chain's read failure.
                if (self.library_errors[index].is_some()
                    && self.notice == self.library_errors[index])
                    || (self.library_state.notices[index].is_some()
                        && self.notice == self.library_state.notices[index])
                {
                    self.notice = None;
                }
                self.library_state.notices[index] = None;
                self.library_errors[index] = None;
                if (reload || changed) && self.route == Route::Library(bookmark) {
                    self.library_selected = None;
                    self.library_page = 0;
                }
            }
            Err(error) => {
                self.library_errors[index] = Some(error);
                if self.library_delete_confirmation == Some(bookmark) {
                    self.cancel_remove_all();
                }
            }
        }
        if let Some(error) = outcome.operation_error {
            self.library_notice(bookmark, error);
        } else if self.library_error(bookmark).is_none()
            && !preserve_unrelated_notice
            && let Some(notice) = success_notice
        {
            self.library_notice(bookmark, notice.into());
        }
        self.refresh_library_search();
    }
    pub(super) fn import_library_legacy(&mut self) {
        if !self.library_writable(false) || !self.library_writable(true) {
            return;
        }
        let Some((services, handle, repo)) = self.library_services() else {
            return;
        };
        let Some(paths) = self.import_library_paths() else {
            return;
        };
        let history_reservation = self.begin_library_operation(false);
        let saved_reservation = self.begin_library_operation(true);
        services.runtime.spawn_blocking(move || {
            let previous_history = history_reservation.wait();
            let previous_saved = saved_reservation.wait();
            let result =
                crate::migration::import_legacy(&repo, Some(&paths.preferences), &paths.history);
            let mut history = LibraryOutcome {
                entries: read_library(&repo, false),
                operation_error: result.as_ref().err().cloned(),
            };
            let mut saved = LibraryOutcome {
                entries: read_library(&repo, true),
                operation_error: result.as_ref().err().cloned(),
            };
            history_reservation.complete(&mut history, previous_history);
            saved_reservation.complete(&mut saved, previous_saved);
            let successful = result.is_ok() && history.entries.is_ok() && saved.entries.is_ok();
            let history_operation = history_reservation.id;
            let saved_operation = saved_reservation.id;
            drop(history_reservation);
            drop(saved_reservation);
            handle.update(move |state, _| {
                // Each store publishes independently: a newer History append
                // may already be queued behind this import, while Saved is idle.
                let current = state.library_state.operations[0] == Some(history_operation)
                    || state.library_state.operations[1] == Some(saved_operation);
                state.finish_library_operation(true, saved_operation, saved, false, None);
                state.finish_library_operation(false, history_operation, history, false, None);
                if current && successful && state.library_errors.iter().all(Option::is_none) {
                    state.notice = result.ok().map(|report| report.to_string());
                }
            });
        });
    }
}

#[cfg(test)]
#[path = "library_controller_tests.rs"]
mod tests;
