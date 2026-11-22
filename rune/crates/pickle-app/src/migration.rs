//! Explicit, read-only import from the original Swift app. No credentials or consent migrate.
use crate::storage::{Entry, EntryReference, Repository};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 32_000_000;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub history_imported: usize,
    pub bookmarks_imported: usize,
    pub history_duplicates: usize,
    pub bookmark_duplicates: usize,
    pub source_files_found: usize,
}
impl std::fmt::Display for MigrationReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.source_files_found == 0 {
            return formatter.write_str(
                "No previous Pickle data was found. Original files have not been changed.",
            );
        }
        write!(
            formatter,
            "Imported {} history answers and {} bookmarks; skipped {} existing duplicates. Original files have not been changed.",
            self.history_imported,
            self.bookmarks_imported,
            self.history_duplicates + self.bookmark_duplicates
        )
    }
}

#[derive(Debug, Clone)]
pub struct LegacyPaths {
    pub preferences: PathBuf,
    pub history: PathBuf,
}
/// Paths are resolved only for an explicit user import action, never at application startup.
pub fn default_legacy_paths() -> Option<LegacyPaths> {
    #[cfg(target_os = "macos")]
    {
        let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
        Some(LegacyPaths {
            preferences: home.join("Library/Preferences/com.pickle.reader.plist"),
            history: home.join("Library/Application Support/Pickle/KnowledgeBase/entries.json"),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Parse and preflight every source and destination before importing any entry.
/// Missing files are normal for users who never enabled history or saved a bookmark.
/// Runtime I/O failure after an append can leave a partial import; rerunning safely resumes.
pub fn import_legacy(
    repo: &Repository,
    legacy_preferences: Option<&Path>,
    legacy_history: &Path,
) -> Result<MigrationReport, String> {
    let mut report = MigrationReport::default();
    let history = match read_optional(legacy_history)? {
        Some(bytes) => {
            report.source_files_found += 1;
            parse_history(&bytes)?
        }
        None => vec![],
    };
    let bookmarks = match legacy_preferences {
        Some(path) => match read_optional(path)? {
            Some(bytes) => {
                report.source_files_found += 1;
                parse_preferences(&bytes)?
            }
            None => vec![],
        },
        None => vec![],
    };
    // A malformed source or destination must never be replaced with partial/default data.
    let current_history = repo.history()?;
    let current_bookmarks = repo.bookmarks()?;
    let (new_history, history_duplicates) = preflight(history, &current_history, 1000)?;
    let (new_bookmarks, bookmark_duplicates) = preflight(bookmarks, &current_bookmarks, 200)?;
    report.history_duplicates = history_duplicates;
    report.bookmark_duplicates = bookmark_duplicates;
    // Legacy files are newest first; append in reverse to retain their presentation order.
    for entry in new_history.into_iter().rev() {
        repo.append(false,entry).map_err(|_|format!("Import stopped after {} history answers and {} bookmarks. Original files are unchanged; retry to resume without duplicates.",report.history_imported,report.bookmarks_imported))?;
        report.history_imported += 1;
    }
    for entry in new_bookmarks.into_iter().rev() {
        repo.append(true,entry).map_err(|_|format!("Import stopped after {} history answers and {} bookmarks. Original files are unchanged; retry to resume without duplicates.",report.history_imported,report.bookmarks_imported))?;
        report.bookmarks_imported += 1;
    }
    Ok(report)
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(
                "Previous Pickle data could not be opened. No import was performed.".into(),
            );
        }
    };
    if file
        .metadata()
        .map_err(|_| "Could not inspect previous Pickle data.")?
        .len()
        > MAX_FILE_BYTES
    {
        return Err(
            "Previous Pickle data exceeds the import size limit. No import was performed.".into(),
        );
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Previous Pickle data could not be read. No import was performed.")?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(
            "Previous Pickle data exceeds the import size limit. No import was performed.".into(),
        );
    }
    Ok(Some(bytes))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyHistory {
    version: u32,
    entries: Vec<LegacyHistoryEntry>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyHistoryEntry {
    id: String,
    recorded_at: String,
    app_name: String,
    #[serde(rename = "bundleID")]
    bundle_id: String,
    action: String,
    question: Option<String>,
    passage: String,
    answer: String,
    chart: Option<String>,
    reference: Option<LegacyReference>,
    quality: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReference {
    url: String,
    title: String,
    kind: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyBookmark {
    id: String,
    saved_at: f64,
    passage: String,
    answer: String,
    source: String,
}

fn parse_history(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let archive: LegacyHistory = serde_json::from_slice(bytes).map_err(
        |_| "Previous history is damaged or has an unsupported schema. No import was performed.",
    )?;
    if archive.version != 1 {
        return Err(
            "Previous history was created by an unsupported version. No import was performed."
                .into(),
        );
    }
    if archive.entries.len() > 1000 {
        return Err(
            "Previous history exceeds its supported capacity. No import was performed.".into(),
        );
    }
    archive
        .entries
        .into_iter()
        .map(|legacy| {
            let id = valid_id(&legacy.id)?;
            let date = DateTime::parse_from_rfc3339(&legacy.recorded_at)
                .map_err(|_| "A previous history date is invalid. No import was performed.")?
                .with_timezone(&Utc)
                .to_rfc3339();
            let action = match legacy.action.as_str() {
                "simplify" => "Simplify",
                "expand" => "Explain more",
                "chart" => "Visualize",
                "follow-up" => "Follow-up",
                "adjustment" => "Adjustment",
                "explain" => "Explain",
                _ => {
                    return Err(
                        "A previous history action is unsupported. No import was performed.".into(),
                    );
                }
            };
            if legacy
                .chart
                .as_ref()
                .is_some_and(|kind| !["bar_chart", "flow_diagram"].contains(&kind.as_str()))
            {
                return Err(
                    "A previous chart type is unsupported. No import was performed.".into(),
                );
            }
            if let Some(reference) = &legacy.reference
                && (pickle_core::WebReference::public_url(&reference.url).is_none()
                    || reference.title.len() > 1000
                    || ![
                        "article",
                        "video captions",
                        "video transcript",
                        "recorded audio",
                    ]
                    .contains(&reference.kind.as_str()))
            {
                return Err("A previous reference is invalid. No import was performed.".into());
            }
            let entry = Entry {
                id,
                recorded_at: date,
                app_name: legacy.app_name,
                app_id: legacy.bundle_id,
                action: action.into(),
                question: legacy.question,
                passage: legacy.passage,
                answer: legacy.answer,
                quality: legacy.quality,
                chart: legacy.chart,
                reference: legacy.reference.map(|reference| EntryReference {
                    url: reference.url,
                    title: reference.title,
                    kind: reference.kind,
                }),
            };
            validate_entry(&entry)?;
            Ok(entry)
        })
        .collect()
}
fn parse_preferences(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let preferences = plist::Value::from_reader(Cursor::new(bytes))
        .map_err(|_| "Previous preferences are damaged. No import was performed.")?;
    let dictionary = preferences
        .as_dictionary()
        .ok_or("Previous preferences have an unsupported schema. No import was performed.")?;
    // Do not enumerate or copy any other keys: credentials, consent and destinations do not migrate.
    let Some(saved) = dictionary.get("savedAnswers") else {
        return Ok(vec![]);
    };
    let data = saved
        .as_data()
        .ok_or("Previous bookmarks have an unsupported storage type. No import was performed.")?;
    let bookmarks:Vec<LegacyBookmark>=serde_json::from_slice(data).map_err(|_|"Previous bookmarks are damaged or have an unsupported schema. No import was performed.")?;
    if bookmarks.len() > 200 {
        return Err(
            "Previous bookmarks exceed their supported capacity. No import was performed.".into(),
        );
    }
    bookmarks
        .into_iter()
        .map(|legacy| {
            // Default Swift JSONEncoder Date values are seconds since the Foundation 2001 epoch.
            let unix_seconds = legacy.saved_at + 978_307_200.0;
            if !unix_seconds.is_finite() || unix_seconds.abs() > 8_000_000_000_000.0 {
                return Err("A previous bookmark date is invalid. No import was performed.".into());
            }
            let date =
                DateTime::<Utc>::from_timestamp_millis((unix_seconds * 1000.0).round() as i64)
                    .ok_or("A previous bookmark date is invalid. No import was performed.")?;
            let entry = Entry {
                id: valid_id(&legacy.id)?,
                recorded_at: date.to_rfc3339(),
                app_name: legacy.source,
                app_id: String::new(),
                action: "Saved answer".into(),
                question: None,
                passage: legacy.passage,
                answer: legacy.answer,
                quality: None,
                chart: None,
                reference: None,
            };
            validate_entry(&entry)?;
            Ok(entry)
        })
        .collect()
}
fn valid_id(value: &str) -> Result<String, String> {
    uuid::Uuid::parse_str(value)
        .map(|id| id.to_string())
        .map_err(|_| "A previous answer identifier is invalid. No import was performed.".into())
}
fn validate_entry(entry: &Entry) -> Result<(), String> {
    if entry.passage.len() > 12000
        || entry.answer.trim().is_empty()
        || entry.answer.len() > 12000
        || entry.question.as_ref().is_some_and(|q| q.len() > 2000)
        || entry.app_name.len() > 1000
        || entry.app_id.len() > 1000
        || entry.quality.as_ref().is_some_and(|q| q.len() > 2000)
    {
        return Err("A previous answer exceeds supported limits. No import was performed.".into());
    }
    Ok(())
}
fn duplicate(a: &Entry, b: &Entry) -> bool {
    a.action == b.action
        && a.question == b.question
        && a.passage == b.passage
        && a.answer == b.answer
}
fn preflight(
    entries: Vec<Entry>,
    existing: &[Entry],
    capacity: usize,
) -> Result<(Vec<Entry>, usize), String> {
    let mut additions = Vec::new();
    let mut duplicates = 0usize;
    let mut ids = BTreeSet::new();
    for entry in entries {
        if existing
            .iter()
            .chain(additions.iter())
            .any(|other| other.id == entry.id && !duplicate(other, &entry))
        {
            return Err(
                "Previous answers contain conflicting identifiers. No import was performed.".into(),
            );
        }
        if existing
            .iter()
            .chain(additions.iter())
            .any(|other| duplicate(other, &entry))
        {
            duplicates += 1;
            continue;
        }
        if !ids.insert(entry.id.clone()) {
            return Err(
                "Previous answers contain conflicting identifiers. No import was performed.".into(),
            );
        }
        additions.push(entry);
    }
    if existing.len() + additions.len() > capacity {
        return Err(format!(
            "Import would exceed the saved-answer limit ({capacity}). Remove some entries before importing. No import was performed."
        ));
    }
    Ok((additions, duplicates))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn record(index: u64) -> serde_json::Value {
        json!({"id":format!("00000000-0000-4000-8000-{index:012}"),"recordedAt":"2026-11-22T12:30:00Z","appName":"TextEdit","bundleID":"com.apple.TextEdit","action":"simplify","passage":format!("Passage {index}"),"answer":format!("Answer {index}"),"quality":"Checked against your selection"})
    }
    fn history(records: Vec<serde_json::Value>) -> Vec<u8> {
        serde_json::to_vec(&json!({"version":1,"entries":records})).unwrap()
    }
    fn write_history(path: &Path, records: Vec<serde_json::Value>) {
        std::fs::write(path, history(records)).unwrap();
    }
    fn preferences(binary: bool) -> Vec<u8> {
        let saved=serde_json::to_vec(&json!([{"id":"00000000-0000-4000-8000-000000000123","savedAt":0.5,"passage":"A passage","answer":"A saved answer","source":"TextEdit"}])).unwrap();
        let mut dictionary = plist::Dictionary::new();
        dictionary.insert("savedAnswers".into(), plist::Value::Data(saved));
        dictionary.insert("cloudConsentV2".into(), plist::Value::Boolean(true));
        dictionary.insert(
            "accountID".into(),
            plist::Value::String("do-not-import".into()),
        );
        let mut output = vec![];
        if binary {
            plist::Value::Dictionary(dictionary)
                .to_writer_binary(&mut output)
                .unwrap();
        } else {
            plist::Value::Dictionary(dictionary)
                .to_writer_xml(&mut output)
                .unwrap();
        }
        output
    }
    #[test]
    fn imports_history_idempotently_preserving_sources_and_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.json");
        write_history(&path, vec![record(2), record(1)]);
        let original = std::fs::read(&path).unwrap();
        let repo = Repository::new(dir.path().join("new")).unwrap();
        let report = import_legacy(&repo, None, &path).unwrap();
        assert_eq!(report.history_imported, 2);
        assert_eq!(repo.history().unwrap()[0].passage, "Passage 2");
        let again = import_legacy(&repo, None, &path).unwrap();
        assert_eq!(again.history_imported, 0);
        assert_eq!(again.history_duplicates, 2);
        assert_eq!(repo.history().unwrap().len(), 2);
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
    #[test]
    fn imports_xml_and_binary_preferences_dates_without_consent() {
        for binary in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("legacy.plist");
            let source = preferences(binary);
            std::fs::write(&path, &source).unwrap();
            let repo = Repository::new(dir.path().join("new")).unwrap();
            let report =
                import_legacy(&repo, Some(&path), &dir.path().join("missing.json")).unwrap();
            assert_eq!(report.bookmarks_imported, 1);
            assert_eq!(
                repo.bookmarks().unwrap()[0].recorded_at,
                "2001-01-01T00:00:00.500+00:00"
            );
            assert!(!repo.settings().unwrap().cloud_consent);
            assert!(repo.settings().unwrap().account_id.is_empty());
            assert_eq!(std::fs::read(&path).unwrap(), source);
            assert_eq!(
                import_legacy(&repo, Some(&path), &dir.path().join("missing.json"))
                    .unwrap()
                    .bookmark_duplicates,
                1
            );
        }
    }
    #[test]
    fn damaged_preferences_prevent_partial_history_import() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.json");
        write_history(&path, vec![record(1)]);
        let prefs = dir.path().join("legacy.plist");
        std::fs::write(&prefs, b"broken").unwrap();
        let repo = Repository::new(dir.path().join("new")).unwrap();
        assert!(import_legacy(&repo, Some(&prefs), &path).is_err());
        assert!(repo.history().unwrap().is_empty());
        assert_eq!(std::fs::read(prefs).unwrap(), b"broken");
    }
    #[test]
    fn rejects_unknown_versions_dates_actions_and_oversize_before_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.json");
        let repo = Repository::new(dir.path().join("new")).unwrap();
        let mut bads = vec![json!({"version":2,"entries":[]})];
        let mut bad = record(1);
        bad["action"] = json!("unknown");
        bads.push(json!({"version":1,"entries":[record(2),bad]}));
        let mut bad = record(1);
        bad["recordedAt"] = json!("yesterday");
        bads.push(json!({"version":1,"entries":[bad]}));
        let mut bad = record(1);
        bad["answer"] = json!("a".repeat(12001));
        bads.push(json!({"version":1,"entries":[bad]}));
        for bad in bads {
            let bytes = serde_json::to_vec(&bad).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            assert!(import_legacy(&repo, None, &path).is_err());
            assert!(repo.history().unwrap().is_empty());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn damaged_destination_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.json");
        write_history(&path, vec![record(1)]);
        let repo = Repository::new(dir.path().join("new")).unwrap();
        std::fs::write(repo.directory().join("history.json"), b"damaged").unwrap();
        assert!(import_legacy(&repo, None, &path).is_err());
        assert_eq!(
            std::fs::read(repo.directory().join("history.json")).unwrap(),
            b"damaged"
        );
    }
    #[test]
    fn missing_sources_are_normal_and_conflicting_ids_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().join("new")).unwrap();
        let path = dir.path().join("missing.json");
        assert_eq!(
            import_legacy(&repo, None, &path)
                .unwrap()
                .source_files_found,
            0
        );
        let first = record(1);
        let mut second = record(1);
        second["answer"] = json!("Conflicting answer");
        write_history(&path, vec![first, second]);
        assert!(import_legacy(&repo, None, &path).is_err());
        assert!(repo.history().unwrap().is_empty());
    }
    #[test]
    fn retains_reference_pointers_and_chart_kind_without_reference_bodies() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.json");
        let mut entry = record(1);
        entry["action"] = json!("chart");
        entry["chart"] = json!("bar_chart");
        entry["reference"] = json!({"url":"https://example.com/article","title":"Original article","kind":"article"});
        write_history(&path, vec![entry]);
        let repo = Repository::new(dir.path().join("new")).unwrap();
        import_legacy(&repo, None, &path).unwrap();
        let history = repo.history().unwrap();
        assert_eq!(history[0].chart.as_deref(), Some("bar_chart"));
        let reference = history[0].reference.as_ref().unwrap();
        assert_eq!(reference.url, "https://example.com/article");
        assert_eq!(reference.title, "Original article");
        assert_eq!(history[0].action, "Visualize");
        let encoded = std::fs::read_to_string(repo.directory().join("history.json")).unwrap();
        assert!(!encoded.contains("reference_body"));
    }
    #[test]
    fn capacity_is_preflighted_before_any_new_data_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.json");
        let mut existing = Vec::new();
        for i in 1..=1000 {
            existing.push(Entry {
                id: format!("existing-{i}"),
                recorded_at: "2026-11-22T00:00:00Z".into(),
                app_name: "Existing".into(),
                app_id: "existing".into(),
                action: "Simplify".into(),
                question: None,
                passage: format!("Existing passage {i}"),
                answer: format!("Existing answer {i}"),
                quality: None,
                chart: None,
                reference: None,
            });
        }
        let repo = Repository::new(dir.path().join("new")).unwrap();
        let archive = serde_json::to_vec(&json!({"version":1,"data":existing})).unwrap();
        std::fs::write(repo.directory().join("history.json"), &archive).unwrap();
        write_history(&path, vec![record(1)]);
        assert!(
            import_legacy(&repo, None, &path)
                .unwrap_err()
                .contains("limit")
        );
        assert_eq!(
            std::fs::read(repo.directory().join("history.json")).unwrap(),
            archive
        );
    }
    #[test]
    fn invalid_bookmark_storage_type_prevents_a_partial_import() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.json");
        write_history(&path, vec![record(1)]);
        let prefs = dir.path().join("legacy.plist");
        let mut dictionary = plist::Dictionary::new();
        dictionary.insert(
            "savedAnswers".into(),
            plist::Value::String("not Data".into()),
        );
        let mut bytes = vec![];
        plist::Value::Dictionary(dictionary)
            .to_writer_binary(&mut bytes)
            .unwrap();
        std::fs::write(&prefs, &bytes).unwrap();
        let repo = Repository::new(dir.path().join("new")).unwrap();
        assert!(import_legacy(&repo, Some(&prefs), &path).is_err());
        assert!(repo.history().unwrap().is_empty());
        assert!(repo.bookmarks().unwrap().is_empty());
        assert_eq!(std::fs::read(prefs).unwrap(), bytes);
    }
}
