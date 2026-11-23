//! Explicit read-only discovery against the saved Local endpoint. No inference,
//! model downloads, draft credentials, or interactive vault access.
use crate::{controller::AppState, storage::Settings};
use pickle_providers::{LocalAIConfiguration, OllamaProvider};
use rune::{Color, El, ScrollSpec, col, row, text, tok::TextStyle};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const MAX_MODELS: usize = 50;
const MAX_MODEL_ENTRIES: usize = 512;

#[derive(Clone, PartialEq, Eq)]
struct ConnectionKey {
    address: String,
    model: String,
    same_device: bool,
    strict_offline: bool,
    account: String,
}
impl ConnectionKey {
    fn from_settings(settings: &Settings) -> Self {
        let local = Settings {
            mode: "Local".into(),
            ..settings.clone()
        };
        Self {
            address: settings.local_address.trim().into(),
            model: settings.local_model.trim().into(),
            same_device: settings.same_device,
            strict_offline: settings.strict_offline,
            account: local.credential_account(),
        }
    }
    fn configuration(&self, token: String) -> LocalAIConfiguration {
        LocalAIConfiguration {
            address: self.address.clone(),
            model: self.model.clone(),
            token,
            same_device: self.same_device,
        }
    }
}

struct ModelList {
    names: Vec<String>,
    omitted: bool,
}
enum Status {
    Idle,
    Checking {
        attempt: uuid::Uuid,
        key: ConnectionKey,
        cancel: CancellationToken,
    },
    Ready {
        key: ConnectionKey,
        models: ModelList,
    },
    Failed(String),
}
pub struct LocalConnectionState {
    saved: ConnectionKey,
    status: Status,
}
impl LocalConnectionState {
    pub fn new(settings: &Settings) -> Self {
        Self {
            saved: ConnectionKey::from_settings(settings),
            status: Status::Idle,
        }
    }
    pub fn record_saved(&mut self, settings: &Settings) {
        let key = ConnectionKey::from_settings(settings);
        if key != self.saved {
            self.reset();
            self.saved = key;
        }
    }
    fn reset(&mut self) {
        if let Status::Checking { cancel, .. } = &self.status {
            cancel.cancel();
        }
        self.status = Status::Idle;
    }
    fn busy(&self) -> bool {
        matches!(self.status, Status::Checking { .. })
    }
}

impl AppState {
    pub(crate) fn cancel_local_connection_test(&mut self) {
        self.local_connection.reset();
    }
    fn connection_to_test(&self) -> Result<ConnectionKey, String> {
        if self.settings_pending {
            return Err("Wait for the connection settings to finish saving.".into());
        }
        if self.credential_busy() {
            return Err(
                "Wait for the saved credential operation to finish before testing the connection."
                    .into(),
            );
        }
        if self.settings.mode != "Local" {
            return Err("Choose Local AI before testing its saved connection.".into());
        }
        let saved = &self.local_connection.saved;
        if self.local_address.text.trim() != saved.address
            || ConnectionKey::from_settings(&self.settings) != *saved
        {
            return Err(
                "Save the server address and connection privacy settings before testing.".into(),
            );
        }
        saved
            .configuration(String::new())
            .validate(saved.strict_offline)
            .map_err(|error| error.to_string())?;
        Ok(saved.clone())
    }
    pub fn test_local_connection(&mut self) {
        if self.local_connection.busy() {
            return;
        }
        let key = match self.connection_to_test() {
            Ok(key) => key,
            Err(error) => {
                self.local_connection.status = Status::Failed(error);
                return;
            }
        };
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            self.local_connection.status = Status::Failed(
                "The connection service is unavailable. Restart Pickle before testing.".into(),
            );
            return;
        };
        self.cancel_local_connection_test();
        let attempt = uuid::Uuid::new_v4();
        let cancel = services.shutdown.child_token();
        self.local_connection.status = Status::Checking {
            attempt,
            key: key.clone(),
            cancel: cancel.clone(),
        };
        let shutdown = services.shutdown.clone();
        services.runtime.spawn(async move {
            let result = tokio::select! {
                _ = cancel.cancelled() => return,
                outcome = tokio::time::timeout(Duration::from_secs(12), discover(&key, cancel.clone())) => {
                    match outcome {
                        Ok(result) => result,
                        Err(_) => { cancel.cancel(); Err("The connection test timed out. Check the saved server and retry.".into()) }
                    }
                }
            };
            if shutdown.is_cancelled() { return; }
            handle.update(move |state, _| state.finish_local_connection(attempt, key, result));
        });
    }
    fn finish_local_connection(
        &mut self,
        attempt: uuid::Uuid,
        key: ConnectionKey,
        outcome: Result<ModelList, String>,
    ) {
        if !matches!(&self.local_connection.status, Status::Checking { attempt: current, key: current_key, .. }
            if *current == attempt && *current_key == key)
        {
            return;
        }
        if self.connection_to_test().as_ref() != Ok(&key) {
            self.cancel_local_connection_test();
            return;
        }
        if self
            .services
            .as_ref()
            .is_some_and(|services| services.shutdown.is_cancelled())
            || (outcome.is_ok()
                && matches!(&self.local_connection.status, Status::Checking { cancel, .. } if cancel.is_cancelled()))
        {
            self.cancel_local_connection_test();
            return;
        }
        self.local_connection.status = match outcome {
            Ok(models) => Status::Ready { key, models },
            Err(error) => Status::Failed(error),
        };
    }
    fn choose_downloaded_model(&mut self, model: &str) {
        let Status::Ready { key, models } = &self.local_connection.status else {
            return;
        };
        if self.connection_to_test().as_ref() != Ok(key)
            || !models.names.iter().any(|name| name == model)
        {
            return;
        }
        // Choosing a discovered model edits only the form. Save settings applies it.
        self.local_model = rune::TextEdit::new(model);
    }
}

async fn discover(key: &ConnectionKey, cancel: CancellationToken) -> Result<ModelList, String> {
    key.configuration(String::new())
        .validate(key.strict_offline)
        .map_err(|error| error.to_string())?;
    if cancel.is_cancelled() {
        return Err("The connection test was cancelled.".into());
    }
    // Noninteractive reads can still be delayed by an OS vault service. Retain
    // the busy guard in that blocking worker so repeated timeouts cannot create
    // an unbounded number of outstanding vault reads.
    static VAULT_BUSY: AtomicBool = AtomicBool::new(false);
    if VAULT_BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(
            "The previous credential check is still stopping. Wait before retrying.".into(),
        );
    }
    let account = key.account.clone();
    let credential = tokio::task::spawn_blocking(move || {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                VAULT_BUSY.store(false, Ordering::Release);
            }
        }
        let _guard = Guard;
        pickle_platform::credential_get(&account, false).map_err(|error| error.to_string())
    });
    let token = tokio::select! {
        _ = cancel.cancelled() => return Err("The connection test was cancelled.".into()),
        result = credential => result.map_err(|_| "The credential service could not finish.".to_owned())??.unwrap_or_default(),
    };
    fetch_models(key, token, cancel).await
}

async fn fetch_models(
    key: &ConnectionKey,
    token: String,
    cancel: CancellationToken,
) -> Result<ModelList, String> {
    let configuration = key.configuration(token);
    configuration
        .validate(key.strict_offline)
        .map_err(|error| error.to_string())?;
    // installed_models performs GET /api/tags with bounded response and timeout;
    // no /api/show, /api/chat, /api/generate, pull, reconnect, or fallback endpoint.
    let names = OllamaProvider::new(configuration)
        .installed_models(cancel)
        .await
        .map_err(|error| error.to_string())?;
    Ok(bounded_models(names))
}

fn bounded_models(names: Vec<String>) -> ModelList {
    let mut result = ModelList {
        names: Vec::new(),
        omitted: names.len() > MAX_MODEL_ENTRIES,
    };
    for name in names.into_iter().take(MAX_MODEL_ENTRIES) {
        let valid = !name.is_empty()
            && name.len() <= 200
            && name.as_bytes()[0].is_ascii_alphanumeric()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
            && !name.to_ascii_lowercase().contains("cloud");
        if !valid {
            result.omitted = true;
            continue;
        }
        if result.names.contains(&name) {
            continue;
        }
        if result.names.len() == MAX_MODELS {
            result.omitted = true;
            break;
        }
        result.names.push(name);
    }
    result.names.sort();
    result
}

fn note(value: impl Into<String>) -> El {
    text(value.into())
        .wrap()
        .w_full()
        .shrink()
        .font(TextStyle::new(12.0, 400.0).lh(18.0))
        .color(Color {
            r: 0.35469,
            g: 0.47700,
            b: 0.38006,
            a: 1.0,
        })
}
pub fn controls(state: &AppState) -> El {
    let busy = state.local_connection.busy();
    let available = state.connection_to_test();
    let mut buttons = row().gap(8.0).child(
        crate::views::action(
            state,
            "test-local-connection",
            if busy {
                "Testing connection…"
            } else {
                "Test connection"
            },
            false,
            |state, _| state.test_local_connection(),
        )
        .disabled(busy || available.is_err()),
    );
    if busy {
        buttons = buttons.child(crate::views::action(
            state,
            "cancel-local-connection",
            "Cancel test",
            false,
            |state, _| state.cancel_local_connection_test(),
        ));
    }
    let mut view = col().key("local-connection-tools").w_full().gap(8.0)
        .child(note("Checks the saved server for downloaded models. This sends no passage and does not run or download a model."))
        .child(buttons);
    if let Err(reason) = &available {
        view = view.child(note(reason));
    }
    match &state.local_connection.status {
        Status::Ready { key, models } if available.as_ref() == Ok(key) => {
            view = view.child(note(if models.names.is_empty() {
                "Connected. No supported downloaded model names were returned."
            } else {
                "Connected. Choose a downloaded model, then Save settings to apply it."
            }));
            if !models.names.is_empty() {
                let mut chooser = col()
                    .key("downloaded-model-chooser")
                    .role("radiogroup")
                    .label("Downloaded Local models")
                    .focus_group()
                    .w_full()
                    .gap(6.0)
                    .h((models.names.len() as f32 * 40.0).min(180.0))
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    });
                for (index, name) in models.names.iter().enumerate() {
                    let selected = state.local_model.text.trim() == name;
                    let model = name.clone();
                    chooser = chooser.child(
                        crate::views::button(
                            state,
                            format!("downloaded-model-{index}"),
                            name,
                            selected,
                            move |state, _| state.choose_downloaded_model(&model),
                        )
                        .child(
                            text(name)
                                .font(TextStyle::new(13.0, 600.0))
                                .truncate()
                                .w_full()
                                .shrink()
                                .color(if selected {
                                    Color {
                                        r: 0.00350,
                                        g: 0.00657,
                                        b: 0.00542,
                                        a: 1.0,
                                    }
                                } else {
                                    Color {
                                        r: 0.78741,
                                        g: 0.89001,
                                        b: 0.72935,
                                        a: 1.0,
                                    }
                                }),
                        )
                        .role("radio")
                        .checked(selected)
                        .focus_selected(selected)
                        .w_full(),
                    );
                }
                view = view.child(chooser);
            }
            if models.omitted {
                view = view.child(note("Unsupported or additional model entries are hidden. Showing up to 50 downloaded models."));
            }
        }
        Status::Failed(reason) => view = view.child(note(reason)),
        _ => {}
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::Activity;
    use rune::{Cx, Env, Runtime, TextEdit};
    use std::io::{Read, Write};

    fn checking(state: &mut AppState) -> (uuid::Uuid, ConnectionKey, CancellationToken) {
        let key = state.connection_to_test().unwrap();
        let attempt = uuid::Uuid::new_v4();
        let cancel = CancellationToken::new();
        state.local_connection.status = Status::Checking {
            attempt,
            key: key.clone(),
            cancel: cancel.clone(),
        };
        (attempt, key, cancel)
    }
    fn models() -> ModelList {
        bounded_models(vec!["a-model:1b".into(), "z-model:2b".into()])
    }

    #[test]
    fn unsaved_recipient_and_offline_tunnel_are_rejected_before_any_service() {
        let mut state = AppState {
            local_address: TextEdit::new("https://another.example"),
            ..AppState::default()
        };
        assert!(state.connection_to_test().is_err());
        state.test_local_connection();
        assert!(
            matches!(&state.local_connection.status, Status::Failed(message) if message.contains("Save"))
        );
        let settings = Settings {
            strict_offline: true,
            same_device: false,
            ..Settings::default()
        };
        state.load_settings(settings);
        assert!(
            state
                .connection_to_test()
                .err()
                .unwrap()
                .contains("Offline")
        );
        assert!(state.services.is_none());
        state.test_local_connection();
        assert!(
            matches!(&state.local_connection.status, Status::Failed(message) if message.contains("Offline"))
        );
        state.settings.same_device = true;
        assert!(state.connection_to_test().err().unwrap().contains("Save"));
        state.load_settings(state.settings.clone());
        assert!(state.connection_to_test().is_ok());
    }

    #[test]
    fn recipient_policy_and_attempt_changes_reject_stale_model_lists() {
        for change in 0..5 {
            let mut state = AppState::default();
            let (attempt, key, cancel) = checking(&mut state);
            match change {
                0 => state.local_address = TextEdit::new("https://new.example"),
                1 => state.settings.strict_offline = true,
                2 => state.settings_pending = true,
                3 => state.credential_operation = Some((uuid::Uuid::new_v4(), key.account.clone())),
                _ => state.settings.mode = "Online".into(),
            }
            state.finish_local_connection(attempt, key, Ok(models()));
            assert!(matches!(state.local_connection.status, Status::Idle));
            assert!(cancel.is_cancelled());
        }
        let mut state = AppState::default();
        let (old_attempt, old_key, _) = checking(&mut state);
        state.cancel_local_connection_test();
        let (attempt, key, _) = checking(&mut state);
        state.finish_local_connection(old_attempt, old_key, Ok(models()));
        assert!(state.local_connection.busy());
        state.finish_local_connection(attempt, key, Ok(models()));
        assert!(matches!(
            state.local_connection.status,
            Status::Ready { .. }
        ));
    }

    #[test]
    fn cancellation_and_saved_endpoint_changes_stop_discovery_and_hide_old_choices() {
        let mut state = AppState::default();
        let (attempt, key, cancel) = checking(&mut state);
        state.cancel();
        assert!(cancel.is_cancelled());
        state.finish_local_connection(attempt, key, Ok(models()));
        assert!(matches!(state.local_connection.status, Status::Idle));
        let (_, _, cancel) = checking(&mut state);
        let changed = Settings {
            local_address: "https://saved.example".into(),
            ..state.settings.clone()
        };
        state.local_connection.record_saved(&changed);
        assert!(cancel.is_cancelled());
        assert!(matches!(state.local_connection.status, Status::Idle));
        assert_eq!(
            state.local_connection.saved.account,
            Settings {
                mode: "Local".into(),
                ..changed
            }
            .credential_account()
        );
    }

    #[test]
    fn model_names_are_valid_bounded_deduplicated_and_sorted() {
        let mut names = vec![
            "z:1b".into(),
            "a:1b".into(),
            "z:1b".into(),
            "cloud:remote".into(),
            "bad\nname".into(),
            "a".repeat(201),
        ];
        names.extend((0..80).map(|index| format!("model-{index:02}:1b")));
        let list = bounded_models(names);
        assert_eq!(list.names.len(), MAX_MODELS);
        assert!(list.omitted);
        assert_eq!(list.names.iter().filter(|name| *name == "z:1b").count(), 1);
        assert_eq!(list.names.first().unwrap(), "a:1b");
        assert!(
            list.names
                .iter()
                .all(|name| !name.contains("cloud") && !name.contains('\n'))
        );
    }

    fn view(state: &AppState, _: &mut Cx<AppState>) -> El {
        col().w_full().h_full().padding(24.0).child(controls(state))
    }
    #[test]
    fn downloaded_model_click_edits_only_draft_and_preserves_current_answer() {
        let mut state = AppState::default();
        let (attempt, key, _) = checking(&mut state);
        state.finish_local_connection(attempt, key, Ok(models()));
        let active_model = state.settings.local_model.clone();
        let cancel = CancellationToken::new();
        state.activity = Activity::Running {
            id: 1,
            cancel: cancel.clone(),
            progress: "Writing".into(),
        };
        state.draft = "Existing answer".into();
        let mut runtime = Runtime::new(state, view, (480.0, 600.0), Env::new(true, 1.0));
        runtime.frame(0.0);
        let (x, y) = runtime.find("downloaded-model-0").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(1.0 / 60.0);
        assert_eq!(runtime.state.local_model.text, "a-model:1b");
        assert_eq!(runtime.state.settings.local_model, active_model);
        assert_eq!(runtime.state.draft, "Existing answer");
        assert!(!cancel.is_cancelled());
        runtime.update(|state, _| state.local_address = TextEdit::new("https://new.example"));
        runtime.frame(1.0 / 60.0);
        assert!(runtime.find("downloaded-model-chooser").is_none());
        runtime.state.choose_downloaded_model("z-model:2b");
        assert_eq!(runtime.state.local_model.text, "a-model:1b");
    }

    #[test]
    fn shutdown_and_cancelled_success_never_publish_downloaded_models() {
        let mut state = AppState::default();
        let (attempt, key, cancel) = checking(&mut state);
        cancel.cancel();
        state.finish_local_connection(attempt, key, Ok(models()));
        assert!(matches!(state.local_connection.status, Status::Idle));
        let (attempt, key, _) = checking(&mut state);
        let (services, _runtime_owner) =
            crate::controller::Services::new(None, &state.settings).unwrap();
        let services = std::sync::Arc::new(services);
        services.shutdown.cancel();
        state.services = Some(services);
        state.finish_local_connection(attempt, key, Ok(models()));
        assert!(matches!(state.local_connection.status, Status::Idle));
    }

    #[tokio::test]
    async fn discovery_uses_only_authenticated_get_tags_against_the_exact_saved_endpoint() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(_) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => return,
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                let mut byte = [0];
                if socket.read_exact(&mut byte).is_err() {
                    return;
                }
                request.push(byte[0]);
            }
            let body =
                r#"{"models":[{"name":"fixture:1b"},{"name":"cloud:remote"},{"name":"bad name"}]}"#;
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            sender.send(String::from_utf8(request).unwrap()).unwrap();
        });
        let settings = Settings {
            local_address: address,
            same_device: true,
            strict_offline: true,
            ..Settings::default()
        };
        let key = ConnectionKey::from_settings(&settings);
        assert_eq!(key.account, settings.credential_account());
        let result = fetch_models(&key, "fixture-access-key".into(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.names, ["fixture:1b"]);
        let request = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(request.starts_with("GET /api/tags HTTP/1.1\r\n"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-access-key\r\n")
        );
        assert!(
            !request.contains("/api/chat")
                && !request.contains("/api/pull")
                && !request.contains("Existing answer")
        );
        server.join().unwrap();
    }
}
