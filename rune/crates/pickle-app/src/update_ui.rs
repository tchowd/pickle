use crate::{
    controller::AppState,
    update::{self, Release},
};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

pub enum UpdateState {
    Idle,
    Checking(CancellationToken),
    Available(Release),
    Downloading(CancellationToken),
    Ready(PathBuf),
    Current,
    Failed(String),
}
impl AppState {
    pub fn cancel_update(&mut self) {
        match &self.update_state {
            UpdateState::Checking(cancel) | UpdateState::Downloading(cancel) => cancel.cancel(),
            _ => {}
        }
        self.update_attempt = uuid::Uuid::new_v4();
        self.update_state = UpdateState::Idle;
    }
    pub fn check_updates(&mut self) {
        if self.settings.strict_offline {
            self.update_state = UpdateState::Failed(
                "Offline mode blocks update checks. Turn it off to contact the release service."
                    .into(),
            );
            return;
        }
        let config = match update::Config::from_build_environment() {
            Ok(Some(config)) => config,
            Ok(None) => {
                self.update_state = UpdateState::Failed("This development build has no signed update feed. Use the release owner's published installer.".into());
                return;
            }
            Err(error) => {
                self.update_state = UpdateState::Failed(error);
                return;
            }
        };
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        self.cancel_update();
        let attempt = self.update_attempt;
        let cancel = services.shutdown.child_token();
        self.update_state = UpdateState::Checking(cancel.clone());
        services.runtime.spawn(async move {
            let outcome = update::check_update(&config, env!("CARGO_PKG_VERSION"), cancel).await;
            handle.update(move |s, _| {
                if s.update_attempt != attempt {
                    return;
                }
                s.update_state = match outcome {
                    Ok(Some(release)) => UpdateState::Available(release),
                    Ok(None) => UpdateState::Current,
                    Err(error) => UpdateState::Failed(error),
                };
            });
        });
    }
    pub fn download_update(&mut self) {
        if self.settings.strict_offline {
            self.update_state = UpdateState::Failed("Offline mode blocks update downloads.".into());
            return;
        }
        let UpdateState::Available(release) = &self.update_state else {
            return;
        };
        let release = release.clone();
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let Some(repo) = &services.repository else {
            return;
        };
        let directory = repo.directory().join("Updates");
        self.cancel_update();
        let attempt = self.update_attempt;
        let cancel = services.shutdown.child_token();
        self.update_state = UpdateState::Downloading(cancel.clone());
        services.runtime.spawn(async move {
            let outcome = update::download_update(&release, &directory, cancel).await;
            handle.update(move |s, _| {
                if s.update_attempt != attempt {
                    return;
                }
                s.update_state = match outcome {
                    Ok(path) => UpdateState::Ready(path),
                    Err(error) => UpdateState::Failed(error),
                };
            });
        });
    }
    pub fn reveal_update(&mut self) {
        if let UpdateState::Ready(path) = &self.update_state
            && let Err(error) = crate::browser_setup::reveal(path)
        {
            self.notice = Some(error);
        }
    }
}
