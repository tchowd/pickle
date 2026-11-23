//! Immutable, reusable native library collation. Never joins separate fields,
//! normalizes stored text, logs content, opens archives, or touches native UI.
use crate::PlatformError;
use std::{fmt, ops::Range};
use tokio_util::sync::CancellationToken;

const BATCH_ROWS: usize = 128;
const BATCH_BYTES: usize = 1024 * 1024;

/// Opaque identity for the native collation environment. Cache keys must use
/// this token rather than assume an unchanged user locale forever.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct LibrarySearchLocale {
    identity: Vec<u8>,
    #[cfg(target_os = "windows")]
    name: Vec<u16>,
}
impl fmt::Debug for LibrarySearchLocale {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LibrarySearchLocale(<opaque>)")
    }
}

/// Logical source sizes only on macOS: Foundation's immutable String storage
/// layout/capacity is not a portable allocation metric. Windows stores exactly
/// the reported boxed UTF-16 payload, plus small row/chunk descriptors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibrarySearchStats {
    pub rows: usize,
    pub fields: usize,
    pub source_utf8_bytes: usize,
    pub source_utf16_units: usize,
    pub retained_utf16_bytes: Option<usize>,
    pub batches: usize,
}

/// Owns native text copies, prepared once off the UI thread. It has no mutable
/// search state; concurrent queries can safely share it in an Arc. Dropping the
/// last owner releases only immutable strings/buffers, on any thread.
pub struct LibrarySearchIndex {
    native: native::Index,
    stats: LibrarySearchStats,
}
impl LibrarySearchIndex {
    /// Empty rows/fields remain separate rows/fields. Batch size is a target,
    /// never a field limit: a single legal large row is prepared intact.
    pub fn new(rows: &[Vec<&str>], cancel: &CancellationToken) -> Result<Self, PlatformError> {
        check_cancel(cancel)?;
        let mut stats = LibrarySearchStats {
            rows: rows.len(),
            ..Default::default()
        };
        for row in rows {
            check_cancel(cancel)?;
            stats.fields = stats.fields.checked_add(row.len()).ok_or_else(failed)?;
            for field in row {
                stats.source_utf8_bytes = stats
                    .source_utf8_bytes
                    .checked_add(field.len())
                    .ok_or_else(failed)?;
                stats.source_utf16_units = stats
                    .source_utf16_units
                    .checked_add(field.encode_utf16().count())
                    .ok_or_else(failed)?;
            }
        }
        // Each row is an independent `any` over immutable fields. Searching
        // small metadata first avoids scanning long passages before a match in
        // a title/question. Only references are sorted: no content is joined,
        // transformed, dropped or copied here, and row order stays unchanged.
        let ordered: Vec<Vec<&str>> = rows
            .iter()
            .map(|row| {
                let mut fields = row.clone();
                fields.sort_by_key(|field| field.len());
                fields
            })
            .collect();
        let batches = batch_ranges(&ordered)?;
        stats.batches = batches.len();
        #[cfg(target_os = "windows")]
        {
            stats.retained_utf16_bytes =
                Some(stats.source_utf16_units.checked_mul(2).ok_or_else(failed)?);
        }
        let native = native::Index::new(&ordered, &batches, cancel)?;
        check_cancel(cancel)?;
        Ok(Self { native, stats })
    }

    pub fn stats(&self) -> LibrarySearchStats {
        self.stats
    }

    /// The caller owns outer trimming and cache policy. All internal whitespace
    /// and embedded NULs remain literal data. An empty query matches every row.
    /// Cancellation or locale changes return an error, never partial results.
    pub fn search(
        &self,
        query: &str,
        expected_locale: &LibrarySearchLocale,
        cancel: &CancellationToken,
    ) -> Result<Vec<usize>, PlatformError> {
        check_environment(expected_locale, cancel)?;
        if query.is_empty() {
            return Ok((0..self.stats.rows).collect());
        }
        let matches = self.native.search(query, expected_locale, cancel)?;
        check_environment(expected_locale, cancel)?;
        Ok(matches)
    }
}

/// Cheap native locale token only; reads no archive, source, vault or UI state.
pub fn library_search_locale() -> Result<LibrarySearchLocale, PlatformError> {
    native::locale()
}

fn failed() -> PlatformError {
    PlatformError(
        "Native saved-answer search is unavailable. Try opening the library again.".into(),
    )
}
fn check_cancel(cancel: &CancellationToken) -> Result<(), PlatformError> {
    if cancel.is_cancelled() {
        Err(PlatformError("Saved-answer search was cancelled.".into()))
    } else {
        Ok(())
    }
}
fn check_environment(
    expected: &LibrarySearchLocale,
    cancel: &CancellationToken,
) -> Result<(), PlatformError> {
    check_cancel(cancel)?;
    if &library_search_locale()? != expected {
        return Err(PlatformError(
            "The search language changed. Search again.".into(),
        ));
    }
    Ok(())
}

// Exact serde_json string size without allocating a copy of each field just
// to choose batches. Standard JSON escapes controls/quotes/backslashes only.
fn json_string_bytes(value: &str) -> Result<usize, PlatformError> {
    value.bytes().try_fold(2usize, |size, byte| {
        let bytes = match byte {
            b'"' | b'\\' | b'\x08' | b'\x0c' | b'\n' | b'\r' | b'\t' => 2,
            0..=0x1f => 6,
            _ => 1,
        };
        size.checked_add(bytes).ok_or_else(failed)
    })
}
fn batch_ranges(rows: &[Vec<&str>]) -> Result<Vec<Range<usize>>, PlatformError> {
    let mut batches = vec![];
    let mut start = 0;
    let mut bytes = 2usize;
    for (index, row) in rows.iter().enumerate() {
        let mut row_bytes = 2usize
            .checked_add(row.len().saturating_sub(1))
            .ok_or_else(failed)?;
        for field in row {
            row_bytes = row_bytes
                .checked_add(json_string_bytes(field)?)
                .ok_or_else(failed)?;
        }
        let next = bytes
            .checked_add(row_bytes)
            .and_then(|n| n.checked_add(usize::from(index > start)))
            .ok_or_else(failed)?;
        if index > start && (index - start >= BATCH_ROWS || next > BATCH_BYTES) {
            batches.push(start..index);
            start = index;
            bytes = 2;
        }
        bytes = bytes
            .checked_add(row_bytes)
            .and_then(|n| n.checked_add(usize::from(index > start)))
            .ok_or_else(failed)?;
    }
    if start < rows.len() {
        batches.push(start..rows.len());
    }
    Ok(batches)
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::{
        ffi::{CStr, c_char, c_void},
        ptr::NonNull,
    };

    unsafe extern "C" {
        fn pickle_library_search_create(data: *const u8, length: usize) -> *mut c_void;
        fn pickle_library_search_release(index: *mut c_void);
        fn pickle_library_search_find(
            index: *mut c_void,
            query: *const u8,
            length: usize,
            flags: *mut u8,
            count: usize,
        ) -> i32;
        fn pickle_library_search_locale() -> *mut c_char;
        fn pickle_library_search_string_free(value: *mut c_char);
    }
    struct Chunk {
        handle: NonNull<c_void>,
        range: Range<usize>,
    }
    // SAFETY: Swift's retained object contains only immutable let [[String]].
    // Searches create only local temporary data inside an autoreleasepool.
    // Rust &self/Arc ownership keeps the object alive during every call; ARC
    // destruction and immutable Foundation string reads have no thread affinity.
    unsafe impl Send for Chunk {}
    unsafe impl Sync for Chunk {}
    impl Drop for Chunk {
        fn drop(&mut self) {
            unsafe { pickle_library_search_release(self.handle.as_ptr()) };
        }
    }
    pub(super) struct Index {
        chunks: Vec<Chunk>,
    }
    impl Index {
        pub(super) fn new(
            rows: &[Vec<&str>],
            batches: &[Range<usize>],
            cancel: &CancellationToken,
        ) -> Result<Self, PlatformError> {
            let mut chunks = vec![];
            for range in batches {
                check_cancel(cancel)?;
                let data = serde_json::to_vec(&rows[range.clone()]).map_err(|_| failed())?;
                if data.len() > isize::MAX as usize {
                    return Err(failed());
                }
                // SAFETY: data is valid for this synchronous copy/JSON decode;
                // the returned object owns its strings after the call returns.
                let handle = NonNull::new(unsafe {
                    pickle_library_search_create(data.as_ptr(), data.len())
                })
                .ok_or_else(failed)?;
                chunks.push(Chunk {
                    handle,
                    range: range.clone(),
                });
            }
            Ok(Self { chunks })
        }
        pub(super) fn search(
            &self,
            query: &str,
            expected: &LibrarySearchLocale,
            cancel: &CancellationToken,
        ) -> Result<Vec<usize>, PlatformError> {
            let data = serde_json::to_vec(query).map_err(|_| failed())?;
            if data.len() > isize::MAX as usize {
                return Err(failed());
            }
            let mut matches = vec![];
            for chunk in &self.chunks {
                check_environment(expected, cancel)?;
                let mut flags = vec![0; chunk.range.len()];
                // SAFETY: immutable retained handle, synchronous borrowed query,
                // and exactly one writable byte for each native row.
                let success = unsafe {
                    pickle_library_search_find(
                        chunk.handle.as_ptr(),
                        data.as_ptr(),
                        data.len(),
                        flags.as_mut_ptr(),
                        flags.len(),
                    )
                };
                if success != 1 || flags.iter().any(|flag| *flag > 1) {
                    return Err(failed());
                }
                matches.extend(
                    flags
                        .into_iter()
                        .enumerate()
                        .filter(|(_, flag)| *flag == 1)
                        .map(|(index, _)| chunk.range.start + index),
                );
            }
            Ok(matches)
        }
    }
    pub(super) fn locale() -> Result<LibrarySearchLocale, PlatformError> {
        let pointer = unsafe { pickle_library_search_locale() };
        if pointer.is_null() {
            return Err(failed());
        }
        // SAFETY: native returns its own NUL-terminated JSON allocation; copy
        // before releasing it exactly once, including on malformed responses.
        let bytes = unsafe { CStr::from_ptr(pointer).to_bytes().to_vec() };
        unsafe { pickle_library_search_string_free(pointer) };
        let _: Vec<String> = serde_json::from_slice(&bytes).map_err(|_| failed())?;
        Ok(LibrarySearchLocale { identity: bytes })
    }
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use windows::{
        Win32::{
            Foundation::{ERROR_SUCCESS, GetLastError, LPARAM},
            Globalization::{
                COMPARE_STRING, FIND_FROMSTART, FindNLSStringEx, GetNLSVersionEx,
                GetUserDefaultLocaleName, LINGUISTIC_IGNORECASE, NLSVERSIONINFOEX,
                NORM_LINGUISTIC_CASING,
            },
        },
        core::PCWSTR,
    };

    // Ignore actual case, not every tertiary distinction (NORM_IGNORECASE can
    // also ignore CJK width). Explicit linguistic casing respects Turkish/Azeri
    // user-locale rules. No nonspace/width/symbol/kana ignore flags are added.
    const FLAGS: u32 = FIND_FROMSTART | LINGUISTIC_IGNORECASE.0 | NORM_LINGUISTIC_CASING.0;
    struct Chunk {
        rows: Vec<Vec<Box<[u16]>>>,
        range: Range<usize>,
    }
    pub(super) struct Index {
        chunks: Vec<Chunk>,
    }
    impl Index {
        pub(super) fn new(
            rows: &[Vec<&str>],
            batches: &[Range<usize>],
            cancel: &CancellationToken,
        ) -> Result<Self, PlatformError> {
            let mut chunks = vec![];
            for range in batches {
                check_cancel(cancel)?;
                let mut encoded = vec![];
                for row in &rows[range.clone()] {
                    check_cancel(cancel)?;
                    let mut fields = vec![];
                    for field in row {
                        let units: Box<[u16]> = field.encode_utf16().collect();
                        if units.len() > i32::MAX as usize {
                            return Err(failed());
                        }
                        fields.push(units);
                    }
                    encoded.push(fields);
                }
                chunks.push(Chunk {
                    rows: encoded,
                    range: range.clone(),
                });
            }
            Ok(Self { chunks })
        }
        pub(super) fn search(
            &self,
            query: &str,
            expected: &LibrarySearchLocale,
            cancel: &CancellationToken,
        ) -> Result<Vec<usize>, PlatformError> {
            let query: Vec<u16> = query.encode_utf16().collect();
            if query.len() > i32::MAX as usize {
                return Err(failed());
            }
            let mut matches = vec![];
            for chunk in &self.chunks {
                check_environment(expected, cancel)?;
                for (index, row) in chunk.rows.iter().enumerate() {
                    check_cancel(cancel)?;
                    let mut matched = false;
                    for field in row {
                        if field.is_empty() {
                            continue;
                        }
                        // SAFETY: immutable UTF-16 buffers use explicit checked
                        // lengths, preserving NULs. Locale is NUL terminated;
                        // reserved fields are null/zero as required by NLS.
                        let found = unsafe {
                            FindNLSStringEx(
                                PCWSTR(expected.name.as_ptr()),
                                FLAGS,
                                field,
                                &query,
                                None,
                                None,
                                None,
                                LPARAM(0),
                            )
                        };
                        if found >= 0 {
                            matched = true;
                            break;
                        }
                        if unsafe { GetLastError() } != ERROR_SUCCESS {
                            return Err(failed());
                        }
                    }
                    if matched {
                        matches.push(chunk.range.start + index);
                    }
                }
            }
            Ok(matches)
        }
    }
    pub(super) fn locale() -> Result<LibrarySearchLocale, PlatformError> {
        // LOCALE_NAME_MAX_LENGTH from the documented Win32 locale-name ABI.
        let mut name = vec![0u16; 85];
        let count = unsafe { GetUserDefaultLocaleName(&mut name) };
        if count <= 1 || count as usize > name.len() || name[count as usize - 1] != 0 {
            return Err(failed());
        }
        name.truncate(count as usize);
        let mut version = NLSVERSIONINFOEX {
            dwNLSVersionInfoSize: std::mem::size_of::<NLSVERSIONINFOEX>() as u32,
            ..Default::default()
        };
        unsafe { GetNLSVersionEx(COMPARE_STRING.0 as u32, PCWSTR(name.as_ptr()), &mut version) }
            .map_err(|_| failed())?;
        let mut identity = Vec::with_capacity(name.len() * 2 + 28);
        for unit in &name {
            identity.extend_from_slice(&unit.to_le_bytes());
        }
        for value in [
            version.dwNLSVersion,
            version.dwDefinedVersion,
            version.dwEffectiveId,
        ] {
            identity.extend_from_slice(&value.to_le_bytes());
        }
        identity.extend_from_slice(&version.guidCustomVersion.to_u128().to_le_bytes());
        Ok(LibrarySearchLocale { identity, name })
    }
}

#[cfg(test)]
#[path = "library_search_tests.rs"]
mod tests;
