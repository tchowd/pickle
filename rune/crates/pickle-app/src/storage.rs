//! Versioned, user-local preferences and explicit answer stores. Never stores credentials.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[cfg(test)]
#[path = "library_search_benchmark.rs"]
mod library_search_benchmark;

const MAX_ARCHIVE_BYTES: usize = 32_000_000;

/// Count encoded bytes, including JSON escapes, before touching durable data.
struct ArchiveEncoder {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}
impl ArchiveEncoder {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            exceeded: false,
        }
    }
}
impl Write for ArchiveEncoder {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(std::io::Error::other(
                "The local archive byte limit was exceeded.",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub level: pickle_core::ReadingLevel,
    pub background_opacity: f32,
    pub theme_intensity: f32,
    pub shortcut: String,
    pub modifier_gesture: bool,
    pub automatic_menu: bool,
    pub mode: String,
    pub local_address: String,
    pub local_model: String,
    pub local_vision_model: String,
    pub same_device: bool,
    pub account_id: String,
    pub cloud_model: String,
    pub jev_enabled: bool,
    pub strict_offline: bool,
    pub cloud_consent: bool,
    pub jev_consent: bool,
    pub local_consent: String,
    pub history_enabled: bool,
    pub streaming: bool,
    pub screen_context_enabled: bool,
    pub web_context_enabled: bool,
    pub context_consent_destination: String,
    pub transcriber_executable: String,
    pub transcriber_model: String,
    pub transcriber_language: String,
    pub text_size: f32,
    pub exclusions: Vec<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            level: pickle_core::ReadingLevel::Automatic,
            background_opacity: 1.0,
            theme_intensity: 1.0,
            shortcut: "ctrl+alt+p".into(),
            modifier_gesture: cfg!(target_os = "macos"),
            automatic_menu: false,
            mode: "Local".into(),
            local_address: "http://127.0.0.1:11434".into(),
            local_model: "llama3.2:1b".into(),
            local_vision_model: String::new(),
            same_device: false,
            account_id: String::new(),
            cloud_model: "@cf/meta/llama-3.3-70b-instruct-fp8-fast".into(),
            jev_enabled: false,
            strict_offline: false,
            cloud_consent: false,
            jev_consent: false,
            local_consent: String::new(),
            history_enabled: false,
            streaming: true,
            screen_context_enabled: false,
            web_context_enabled: false,
            context_consent_destination: String::new(),
            text_size: 18.0,
            transcriber_executable: String::new(),
            transcriber_model: String::new(),
            transcriber_language: "auto".into(),
            exclusions: vec![
                "com.1password.1password".into(),
                "com.apple.keychainaccess".into(),
                "1Password.exe".into(),
            ],
        }
    }
}
impl Settings {
    pub fn credential_account(&self) -> String {
        Self::credential_account_for(&self.mode, &self.local_address, &self.account_id)
    }
    pub fn credential_account_for(mode: &str, local_address: &str, account_id: &str) -> String {
        if mode == "Online" {
            // Never fall back to the old shared `cloudflare` vault item: it has
            // no verified account binding and must not be reused or deleted.
            format!("cloudflare:{}", account_id.trim().to_ascii_lowercase())
        } else {
            format!("local-server:{}", local_address.trim())
        }
    }
    pub fn destination(&self) -> String {
        format!("{}|{}", self.local_address.trim(), self.same_device)
    }
    pub fn reset_consent_for_recipient_change(&mut self, previous: &Self) {
        if self.destination() != previous.destination() {
            self.local_consent.clear();
            self.context_consent_destination.clear();
        }
        if !self
            .account_id
            .trim()
            .eq_ignore_ascii_case(previous.account_id.trim())
        {
            self.cloud_consent = false;
            self.jev_consent = false;
            self.context_consent_destination.clear();
        }
    }
    pub fn context_destination(&self) -> String {
        if self.mode == "Online" {
            format!("cloudflare:{}|jev:{}", self.account_id, self.jev_enabled)
        } else {
            format!("local:{}", self.destination())
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.invocation_config()
            .validate()
            .map_err(|e| e.to_string())?;
        if !self.background_opacity.is_finite()
            || !(0.0..=1.0).contains(&self.background_opacity)
            || !self.theme_intensity.is_finite()
            || !(0.0..=1.0).contains(&self.theme_intensity)
            || self.shortcut.len() > 200
        {
            return Err(
                "Appearance values must be between zero and one, and shortcuts must be bounded."
                    .into(),
            );
        }
        if !["Local", "Online"].contains(&self.mode.as_str()) {
            return Err("Choose Local or Online AI.".into());
        }
        if !self.text_size.is_finite() || !(14.0..=24.0).contains(&self.text_size) {
            return Err("Reading size must be between 14 and 24 points.".into());
        }
        if self.exclusions.len() > 200 || self.exclusions.iter().any(|s| s.len() > 512) {
            return Err("The application exclusion list is too large.".into());
        }
        if self.local_address.len() > 2048
            || self.account_id.len() > 200
            || self.local_model.len() > 200
            || self.local_vision_model.len() > 200
            || self.cloud_model.len() > 200
            || self.transcriber_executable.len() > 4096
            || self.transcriber_model.len() > 4096
            || self.transcriber_language.len() > 50
        {
            return Err("A connection setting is too long.".into());
        }
        Ok(())
    }
    pub fn configure_transcriber(&self) -> Result<(), String> {
        pickle_platform::configure_local_transcriber(self.transcriber_config()?)
            .map_err(|e| e.to_string())
    }
    pub fn invocation_config(&self) -> pickle_platform::InvocationConfig {
        pickle_platform::InvocationConfig {
            shortcut: self.shortcut.clone(),
            modifier_gesture: self.modifier_gesture,
            automatic_menu: self.automatic_menu,
        }
    }
    pub fn transcriber_config(
        &self,
    ) -> Result<Option<pickle_platform::LocalTranscriberConfig>, String> {
        let config = if self.transcriber_executable.is_empty() && self.transcriber_model.is_empty()
        {
            None
        } else {
            Some(pickle_platform::LocalTranscriberConfig {
                executable: self.transcriber_executable.clone().into(),
                model: self.transcriber_model.clone().into(),
                language: self.transcriber_language.clone(),
            })
        };
        Ok(config)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EntryReference {
    pub url: String,
    pub title: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub id: String,
    pub recorded_at: String,
    pub app_name: String,
    pub app_id: String,
    pub action: String,
    pub question: Option<String>,
    pub passage: String,
    pub answer: String,
    pub quality: Option<String>,
    #[serde(default)]
    pub chart: Option<String>,
    #[serde(default)]
    pub reference: Option<EntryReference>,
}
impl Entry {
    fn duplicate(&self, other: &Self) -> bool {
        self.action == other.action
            && self.question == other.question
            && self.passage == other.passage
            && self.answer == other.answer
    }
    fn validate(&self) -> Result<(), String> {
        if self.passage.len() > 12_000
            || self.answer.len() > 12_000
            || self.question.as_ref().is_some_and(|q| q.len() > 2_000)
            || self.app_name.len() > 1000
            || self.app_id.len() > 1000
            || self.id.len() > 200
            || self.action.len() > 100
        {
            return Err("A saved answer exceeds the supported limits.".into());
        }
        Ok(())
    }
    pub(crate) fn search_fields(&self) -> [&str; 6] {
        [
            self.passage.as_str(),
            self.answer.as_str(),
            self.app_name.as_str(),
            self.question.as_deref().unwrap_or_default(),
            self.reference
                .as_ref()
                .map(|reference| reference.title.as_str())
                .unwrap_or_default(),
            self.reference
                .as_ref()
                .map(|reference| reference.url.as_str())
                .unwrap_or_default(),
        ]
    }
    #[cfg(test)]
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim();
        if query.is_empty() {
            return true;
        }
        let cancel = tokio_util::sync::CancellationToken::new();
        let locale = pickle_platform::library_search_locale().expect("Native fixture locale");
        let index =
            pickle_platform::LibrarySearchIndex::new(&[self.search_fields().to_vec()], &cancel)
                .expect("Native fixture index");
        !index
            .search(query, &locale, &cancel)
            .expect("Native fixture search")
            .is_empty()
    }
}

#[derive(Serialize, Deserialize)]
struct Archive<T> {
    version: u32,
    data: T,
}

pub struct Repository {
    directory: PathBuf,
}
impl Repository {
    pub fn default_directory() -> PathBuf {
        directories::ProjectDirs::from("com", "Pickle", "PickleRune")
            .expect("The OS must provide a user application-data directory")
            .data_local_dir()
            .to_path_buf()
    }
    pub fn new(directory: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&directory)
            .map_err(|_| "Could not create Pickle's local data directory.")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .map_err(|_| "Could not protect Pickle's data directory.")?;
        }
        Ok(Self { directory })
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn settings(&self) -> Result<Settings, String> {
        let settings: Settings = self.read("settings.json")?.unwrap_or_default();
        settings.validate()?;
        Ok(settings)
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        settings.validate()?;
        let _lock = self.lock()?;
        // Never overwrite a malformed or newer archive, even with valid in-memory defaults.
        let _: Option<Settings> = self.read("settings.json")?;
        self.write("settings.json", settings)
    }
    pub fn history(&self) -> Result<Vec<Entry>, String> {
        self.entries("history.json", 1000)
    }
    pub fn bookmarks(&self) -> Result<Vec<Entry>, String> {
        self.entries("bookmarks.json", 200)
    }
    fn entries(&self, name: &str, capacity: usize) -> Result<Vec<Entry>, String> {
        let entries: Vec<Entry> = self.read(name)?.unwrap_or_default();
        if entries.len() > capacity {
            return Err(
                "The saved-answer file exceeds its supported capacity. It has not been changed."
                    .into(),
            );
        }
        for entry in &entries {
            entry.validate()?;
        }
        Ok(entries)
    }
    pub fn append(&self, bookmark: bool, entry: Entry) -> Result<(), String> {
        entry.validate()?;
        let _lock = self.lock()?;
        let (name, capacity) = if bookmark {
            ("bookmarks.json", 200)
        } else {
            ("history.json", 1000)
        };
        let mut entries = self.entries(name, capacity)?;
        if entries.iter().any(|e| e.duplicate(&entry)) {
            return Ok(());
        }
        if entries.len() >= capacity {
            return Err(format!(
                "Your saved answers are full ({capacity}). Remove an answer to make room."
            ));
        }
        entries.insert(0, entry);
        self.write(name, &entries)
    }
    pub fn remove(&self, bookmark: bool, id: &str) -> Result<(), String> {
        let _lock = self.lock()?;
        let (name, capacity) = if bookmark {
            ("bookmarks.json", 200)
        } else {
            ("history.json", 1000)
        };
        let mut entries = self.entries(name, capacity)?;
        entries.retain(|entry| entry.id != id);
        self.write(name, &entries)
    }
    #[cfg(test)]
    pub fn remove_all(&self, bookmark: bool) -> Result<(), String> {
        self.remove_all_inner(bookmark, None)
    }
    pub fn remove_all_if_matches(&self, bookmark: bool, expected: &[Entry]) -> Result<(), String> {
        self.remove_all_inner(bookmark, Some(expected))
    }
    fn remove_all_inner(&self, bookmark: bool, expected: Option<&[Entry]>) -> Result<(), String> {
        let _lock = self.lock()?;
        let (name, capacity) = if bookmark {
            ("bookmarks.json", 200)
        } else {
            ("history.json", 1000)
        };
        // Clearing is an ordinary archive mutation, never a recovery/reset path.
        // Validate its current version, capacity and every entry under the lock.
        let entries = self.entries(name, capacity)?;
        if expected.is_some_and(|expected| entries != expected) {
            return Err(
                "These saved answers changed. Review the refreshed list before deleting all."
                    .into(),
            );
        }
        self.write(name, &Vec::<Entry>::new())
    }
    fn lock(&self) -> Result<File, String> {
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(self.directory.join("store.lock"))
            .map_err(|_| "Could not lock Pickle's local storage.")?;
        file.lock_exclusive()
            .map_err(|_| "Pickle's local storage is busy.")?;
        Ok(file)
    }
    fn read<T: serde::de::DeserializeOwned>(&self, name: &str) -> Result<Option<T>, String> {
        let path = self.directory.join(name);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(format!(
                    "{name} could not be opened. Existing data has not been changed."
                ));
            }
        };
        if file
            .metadata()
            .map_err(|_| "Could not inspect local storage.")?
            .len()
            > MAX_ARCHIVE_BYTES as u64
        {
            return Err(format!(
                "{name} is too large. Existing data has not been changed."
            ));
        }
        let archive: Archive<T> = serde_json::from_reader(file).map_err(|_| format!("{name} is damaged. Existing data has not been changed. Move the file aside before trying again."))?;
        if archive.version != 1 {
            return Err(format!(
                "{name} was created by another version. Existing data has not been changed."
            ));
        }
        Ok(Some(archive.data))
    }
    fn write<T: Serialize + ?Sized>(&self, name: &str, data: &T) -> Result<(), String> {
        #[derive(Serialize)]
        struct Borrowed<'a, T: ?Sized> {
            version: u32,
            data: &'a T,
        }
        let mut encoded = ArchiveEncoder::new(MAX_ARCHIVE_BYTES);
        serde_json::to_writer_pretty(&mut encoded, &Borrowed { version: 1, data }).map_err(|_| {
            if encoded.exceeded {
                "This local file would exceed the 32 MB limit. Existing data has not been changed. Remove some saved answers before saving more."
            } else {
                "Could not encode local data."
            }
        })?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)
            .map_err(|_| "Could not stage local data.")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| "Could not protect local data.")?;
        }
        temporary
            .write_all(&encoded.bytes)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|_| "Could not save local data. Existing data has not been changed.")?;
        temporary
            .persist(self.directory.join(name))
            .map_err(|_| "Could not replace local data. Existing data has not been changed.")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(i: usize) -> Entry {
        Entry {
            id: i.to_string(),
            recorded_at: "2026-11-22T00:00:00Z".into(),
            app_name: "Fixture".into(),
            app_id: "fixture".into(),
            action: "Simplify".into(),
            question: None,
            passage: format!("Passage {i}"),
            answer: format!("Answer {i}"),
            quality: None,
            chart: None,
            reference: None,
        }
    }
    #[test]
    fn settings_and_history_roundtrip_without_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        assert_eq!(repo.settings().unwrap(), Settings::default());
        repo.save_settings(&Settings::default()).unwrap();
        repo.append(false, entry(1)).unwrap();
        repo.append(false, entry(1)).unwrap();
        assert_eq!(repo.history().unwrap().len(), 1);
        assert!(repo.history().unwrap()[0].matches("answer 1"));
        repo.remove(false, "1").unwrap();
        assert!(repo.history().unwrap().is_empty());
    }
    #[test]
    fn saved_search_reopens_both_stores_and_matches_full_questions_and_pointers() {
        let directory = tempfile::tempdir().unwrap();
        let reference = pickle_core::WebReference {
            url: format!(
                "https://example.com/video?{}pointer-end",
                "source=owned&".repeat(250)
            ),
            title: format!("{}TITLE-END-Éléphant", "t".repeat(850)),
            kind: "video captions".into(),
            text: "REFERENCE_BODY_MUST_NOT_BE_STORED_OR_SEARCHED".into(),
            timestamp: Some(86.25),
        };
        reference.validate().unwrap();
        for bookmark in [false, true] {
            let mut with_pointer = entry(1);
            with_pointer.passage = format!("{}PASSAGE-END-東京", "p".repeat(10_000));
            with_pointer.answer = format!("{}ANSWER-END-Résumé", "a".repeat(10_000));
            with_pointer.app_name = "Éditeur Application".into();
            with_pointer.question = Some(format!("{}QUESTION-END-Café", "q".repeat(1800)));
            with_pointer.reference = Some(EntryReference {
                url: reference.url.clone(),
                title: reference.title.clone(),
                kind: reference.kind.clone(),
            });
            let without_pointer = entry(2);
            let repository = Repository::new(directory.path().into()).unwrap();
            repository.append(bookmark, with_pointer.clone()).unwrap();
            repository
                .append(bookmark, without_pointer.clone())
                .unwrap();
            drop(repository);

            let path = directory.path().join(if bookmark {
                "bookmarks.json"
            } else {
                "history.json"
            });
            let before_search = fs::read(&path).unwrap();
            let encoded = String::from_utf8(before_search.clone()).unwrap();
            assert!(!encoded.contains(&reference.text));
            assert!(!encoded.contains("\"timestamp\""));
            let repository = Repository::new(directory.path().into()).unwrap();
            let reopened = if bookmark {
                repository.bookmarks().unwrap()
            } else {
                repository.history().unwrap()
            };
            let saved = reopened.iter().find(|entry| entry.id == "1").unwrap();
            assert_eq!(saved, &with_pointer);
            for query in [
                "passage-end-東京",
                "answer-end-RÉSUMÉ",
                "  ÉDITEUR APPLICATION\n",
                "\tquestion-end-CAFÉ \u{2003}",
                "title-end-ÉLÉPHANT",
                "  EXAMPLE.COM/VIDEO? ",
                "POINTER-END",
            ] {
                assert!(
                    saved.matches(query),
                    "Search missed full saved field: {query}"
                );
            }
            for query in ["", " \t\n\u{2003}"] {
                assert!(reopened.iter().all(|entry| entry.matches(query)));
            }
            assert!(reopened.iter().all(|entry| !entry.matches(&reference.text)));
            assert!(!saved.matches("video captions"));
            let absent = reopened.iter().find(|entry| entry.id == "2").unwrap();
            assert_eq!(absent, &without_pointer);
            assert!(absent.question.is_none() && absent.reference.is_none());
            assert!(!absent.matches("QUESTION-END"));
            assert!(!absent.matches("POINTER-END"));
            assert_eq!(fs::read(&path).unwrap(), before_search);
        }
    }
    #[test]
    fn corruption_and_unknown_versions_block_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        fs::write(dir.path().join("history.json"), b"damaged").unwrap();
        assert!(repo.append(false, entry(1)).is_err());
        assert!(repo.remove(false, "1").is_err());
        assert_eq!(
            fs::read(dir.path().join("history.json")).unwrap(),
            b"damaged"
        );
        fs::write(
            dir.path().join("settings.json"),
            br#"{"version":2,"data":{}}"#,
        )
        .unwrap();
        assert!(repo.save_settings(&Settings::default()).is_err());
    }
    #[test]
    fn remove_all_reopens_empty_and_preserves_the_other_store_and_settings() {
        for bookmark in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let repo = Repository::new(directory.path().into()).unwrap();
            repo.save_settings(&Settings {
                history_enabled: true,
                ..Settings::default()
            })
            .unwrap();
            repo.append(bookmark, entry(1)).unwrap();
            repo.append(bookmark, entry(2)).unwrap();
            repo.append(!bookmark, entry(3)).unwrap();
            drop(repo);
            let target = directory.path().join(if bookmark {
                "bookmarks.json"
            } else {
                "history.json"
            });
            let other = directory.path().join(if bookmark {
                "history.json"
            } else {
                "bookmarks.json"
            });
            let settings = directory.path().join("settings.json");
            let retained_other = fs::read(&other).unwrap();
            let retained_settings = fs::read(&settings).unwrap();
            let reopened = Repository::new(directory.path().into()).unwrap();
            reopened.remove_all(bookmark).unwrap();
            drop(reopened);
            let reopened = Repository::new(directory.path().into()).unwrap();
            let (cleared, retained) = if bookmark {
                (reopened.bookmarks().unwrap(), reopened.history().unwrap())
            } else {
                (reopened.history().unwrap(), reopened.bookmarks().unwrap())
            };
            assert!(cleared.is_empty());
            assert_eq!(retained, vec![entry(3)]);
            assert_eq!(fs::read(&other).unwrap(), retained_other);
            assert_eq!(fs::read(&settings).unwrap(), retained_settings);
            let empty_archive = fs::read(&target).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&empty_archive).unwrap(),
                serde_json::json!({"version":1,"data":[]})
            );
            reopened.remove_all(bookmark).unwrap();
            assert_eq!(fs::read(&target).unwrap(), empty_archive);
        }
    }
    #[test]
    fn remove_all_never_resets_malformed_newer_invalid_or_overcapacity_archives() {
        for bookmark in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let repo = Repository::new(directory.path().into()).unwrap();
            repo.save_settings(&Settings::default()).unwrap();
            repo.append(!bookmark, entry(7)).unwrap();
            let target = directory.path().join(if bookmark {
                "bookmarks.json"
            } else {
                "history.json"
            });
            let other = directory.path().join(if bookmark {
                "history.json"
            } else {
                "bookmarks.json"
            });
            let retained_other = fs::read(&other).unwrap();
            let settings = directory.path().join("settings.json");
            let retained_settings = fs::read(&settings).unwrap();
            let mut invalid_entry = entry(1);
            invalid_entry.passage = "x".repeat(12_001);
            let capacity = if bookmark { 200 } else { 1000 };
            let protected = [
                b"damaged archive".to_vec(),
                br#"{"version":1,"data":{}}"#.to_vec(),
                serde_json::to_vec(&Archive {
                    version: 2,
                    data: vec![entry(1)],
                })
                .unwrap(),
                serde_json::to_vec(&Archive {
                    version: 1,
                    data: vec![invalid_entry],
                })
                .unwrap(),
                serde_json::to_vec(&Archive {
                    version: 1,
                    data: (0..=capacity).map(entry).collect::<Vec<_>>(),
                })
                .unwrap(),
            ];
            for bytes in protected {
                fs::write(&target, &bytes).unwrap();
                assert!(repo.remove_all(bookmark).is_err());
                assert_eq!(fs::read(&target).unwrap(), bytes);
                assert_eq!(fs::read(&other).unwrap(), retained_other);
                assert_eq!(fs::read(&settings).unwrap(), retained_settings);
                let reopened = Repository::new(directory.path().into()).unwrap();
                assert!(if bookmark {
                    reopened.bookmarks().is_err()
                } else {
                    reopened.history().is_err()
                });
            }
        }
    }
    #[test]
    fn remove_all_confirmation_refuses_new_or_changed_entries_until_reviewed_again() {
        for bookmark in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let repo = Repository::new(directory.path().into()).unwrap();
            repo.append(bookmark, entry(1)).unwrap();
            repo.append(bookmark, entry(2)).unwrap();
            let read = || {
                if bookmark {
                    repo.bookmarks().unwrap()
                } else {
                    repo.history().unwrap()
                }
            };
            let expected = read();
            let target = directory.path().join(if bookmark {
                "bookmarks.json"
            } else {
                "history.json"
            });
            // A second repository records an unseen answer after confirmation
            // opens. The stale snapshot must not delete that new record.
            Repository::new(directory.path().into())
                .unwrap()
                .append(bookmark, entry(3))
                .unwrap();
            let with_new_record = fs::read(&target).unwrap();
            assert!(repo.remove_all_if_matches(bookmark, &expected).is_err());
            assert_eq!(fs::read(&target).unwrap(), with_new_record);
            assert_eq!(read().len(), 3);

            // Equality must cover the whole entry, beyond duplicate detection's
            // passage/question/answer/action fields and the number of records.
            let mut changed = expected.clone();
            changed[0].quality = Some("A revised stored quality label".into());
            fs::write(
                &target,
                serde_json::to_vec(&Archive {
                    version: 1,
                    data: &changed,
                })
                .unwrap(),
            )
            .unwrap();
            let changed_bytes = fs::read(&target).unwrap();
            assert!(repo.remove_all_if_matches(bookmark, &expected).is_err());
            assert_eq!(fs::read(&target).unwrap(), changed_bytes);
            let reviewed_again = read();
            assert_eq!(reviewed_again, changed);
            repo.remove_all_if_matches(bookmark, &reviewed_again)
                .unwrap();
            drop(repo);
            let reopened = Repository::new(directory.path().into()).unwrap();
            assert!(if bookmark {
                reopened.bookmarks().unwrap().is_empty()
            } else {
                reopened.history().unwrap().is_empty()
            });
        }
    }
    #[test]
    fn two_writers_reread_under_an_exclusive_lock() {
        let dir = tempfile::tempdir().unwrap();
        let location = dir.path().to_path_buf();
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let location = location.clone();
                std::thread::spawn(move || {
                    Repository::new(location)
                        .unwrap()
                        .append(false, entry(i))
                        .unwrap()
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(
            Repository::new(location).unwrap().history().unwrap().len(),
            8
        );
    }

    #[test]
    fn encoded_history_limit_refuses_append_without_stranding_a_readable_archive() {
        let large_entry = |i| {
            let mut record = entry(i);
            record.id = format!("budget-{i:04}");
            record.passage = format!("{}{:04}", "\\".repeat(11_996), i);
            record.answer = "\\".repeat(12_000);
            record
        };
        let encoded = |records: &[Entry]| {
            serde_json::to_vec_pretty(&Archive {
                version: 1,
                data: records,
            })
            .unwrap()
        };
        let one = encoded(&[large_entry(0)]).len();
        let per_entry = encoded(&[large_entry(0), large_entry(1)]).len() - one;
        let count = 1 + (MAX_ARCHIVE_BYTES - one) / per_entry;
        assert!(count < 1000);
        let records: Vec<_> = (0..count).map(large_entry).collect();
        let original = encoded(&records);
        assert!(original.len() <= MAX_ARCHIVE_BYTES);
        assert!(original.len() + per_entry > MAX_ARCHIVE_BYTES);
        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::new(directory.path().into()).unwrap();
        let path = directory.path().join("history.json");
        fs::write(&path, &original).unwrap();
        assert_eq!(repo.history().unwrap().len(), count);

        assert!(
            repo.append(false, large_entry(count)).is_err(),
            "An accepted append must never create an archive that the next read rejects."
        );
        assert!(fs::read(&path).unwrap() == original);
        assert_eq!(repo.history().unwrap().len(), count);
        repo.remove_all_if_matches(false, &records).unwrap();
        repo.append(false, large_entry(count)).unwrap();
        assert_eq!(repo.history().unwrap().len(), 1);
    }

    #[test]
    fn archive_encoding_budget_counts_escaped_and_unicode_bytes_at_the_boundary() {
        for (text, exact_bytes) in [("\n\n", 6), ("é", 4)] {
            let mut exact = ArchiveEncoder::new(exact_bytes);
            serde_json::to_writer_pretty(&mut exact, text).unwrap();
            assert_eq!(exact.bytes.len(), exact_bytes);
            assert!(!exact.exceeded);
            assert_eq!(
                serde_json::from_slice::<String>(&exact.bytes).unwrap(),
                text
            );
            let mut too_small = ArchiveEncoder::new(exact_bytes - 1);
            assert!(serde_json::to_writer_pretty(&mut too_small, text).is_err());
            assert!(too_small.exceeded);
            assert!(too_small.bytes.len() < exact_bytes);
        }
    }
    #[test]
    fn local_endpoints_are_scoped_and_history_is_opt_in() {
        let settings = Settings::default();
        assert!(!settings.history_enabled);
        assert!(!settings.cloud_consent);
        assert!(settings.credential_account().starts_with("local-server:"));
        assert_ne!(
            settings.destination(),
            Settings {
                same_device: true,
                ..settings
            }
            .destination()
        );
    }

    #[test]
    fn online_credentials_are_account_scoped_without_a_legacy_global_fallback() {
        let mut settings = Settings {
            mode: "Online".into(),
            account_id: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
            ..Settings::default()
        };
        let first = settings.credential_account();
        assert_eq!(first, "cloudflare:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        settings.account_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        assert_ne!(settings.credential_account(), first);
        assert_ne!(settings.credential_account(), "cloudflare");
    }

    #[test]
    fn consent_is_invalidated_when_its_account_or_local_destination_changes() {
        let previous = Settings {
            mode: "Online".into(),
            account_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            cloud_consent: true,
            jev_consent: true,
            context_consent_destination: "already-granted-context".into(),
            ..Settings::default()
        };
        let mut next = previous.clone();
        next.account_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        next.reset_consent_for_recipient_change(&previous);
        assert!(!next.cloud_consent && !next.jev_consent);
        assert!(next.context_consent_destination.is_empty());
        let mut local = Settings {
            local_consent: "already-granted-local".into(),
            context_consent_destination: "already-granted-context".into(),
            ..Settings::default()
        };
        let old_local = local.clone();
        local.local_address = "https://another-server.example".into();
        local.reset_consent_for_recipient_change(&old_local);
        assert!(local.local_consent.is_empty());
        assert!(local.context_consent_destination.is_empty());
        let mut equivalent = previous.clone();
        equivalent.account_id.make_ascii_uppercase();
        equivalent.reset_consent_for_recipient_change(&previous);
        assert!(equivalent.cloud_consent && equivalent.jev_consent);
    }
}
