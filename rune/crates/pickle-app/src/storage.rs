//! Versioned, user-local preferences and explicit answer stores. Never stores credentials.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

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
        if self.mode == "Online" {
            "cloudflare".into()
        } else {
            format!("local-server:{}", self.local_address.trim())
        }
    }
    pub fn destination(&self) -> String {
        format!("{}|{}", self.local_address.trim(), self.same_device)
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
    pub fn matches(&self, query: &str) -> bool {
        let q = query.to_lowercase();
        [&self.passage, &self.answer, &self.app_name]
            .iter()
            .any(|s| s.to_lowercase().contains(&q))
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
            > 32_000_000
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
        let encoded = serde_json::to_vec_pretty(&Borrowed { version: 1, data })
            .map_err(|_| "Could not encode local data.")?;
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
            .write_all(&encoded)
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
}
