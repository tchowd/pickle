use crate::controller::AppState;
use pickle_providers::browser::{BrowserKind, install_browser_host};
use std::path::{Path, PathBuf};

pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let outcome = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .spawn();
    #[cfg(target_os = "windows")]
    let outcome = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
    outcome.map(|_| ()).map_err(|_| {
        "Could not open the file location. Use the displayed path in your file manager.".into()
    })
}
fn packaged_paths() -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let executable = std::env::current_exe().map_err(|_| "Could not locate this application.")?;
    let directory = executable
        .parent()
        .ok_or("Could not locate the application directory.")?;
    let host = directory.join(if cfg!(windows) {
        "pickle-browser-host.exe"
    } else {
        "pickle-browser-host"
    });
    let application = executable
        .ancestors()
        .find(|p| p.extension().is_some_and(|ext| ext == "app"))
        .unwrap_or(&executable)
        .to_path_buf();
    let extension =
        if cfg!(target_os = "macos") && application.extension().is_some_and(|ext| ext == "app") {
            application.join("Contents/Resources/browser-extension")
        } else {
            directory.join("browser-extension")
        };
    Ok((host, application, extension))
}
impl AppState {
    pub fn disconnect_browser(&mut self, edge: bool) {
        if !self.native_enabled {
            self.notice =
                Some("Browser integration is available in the running desktop application.".into());
            return;
        }
        let id = self.extension_id.text.trim().to_owned();
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let Some(repo) = services.repository.clone() else {
            return;
        };
        services.runtime.spawn_blocking(move || {
            let result = pickle_providers::browser::uninstall_browser_host(
                if edge {
                    BrowserKind::Edge
                } else {
                    BrowserKind::Chrome
                },
                &id,
                repo.directory(),
            );
            handle.update(move |s, _| {
                s.notice = Some(match result {
                    Ok(true) => {
                        "Pickle's browser host was disconnected. Existing saved answers are kept."
                            .into()
                    }
                    Ok(false) => "No matching browser host is installed.".into(),
                    Err(error) => error.to_string(),
                })
            });
        });
    }
    pub fn reveal_extension(&mut self) {
        if !self.native_enabled {
            self.notice = Some(
                "Opening the extension folder is available in the running desktop application."
                    .into(),
            );
            return;
        }
        let outcome = packaged_paths().and_then(|(_, _, directory)| {
            if !directory.join("manifest.json").is_file() {
                return Err(
                    "Use the packaged Pickle app to find the bundled browser extension.".into(),
                );
            }
            reveal(&directory)
        });
        if let Err(error) = outcome {
            self.notice = Some(error);
        }
    }
    pub fn connect_browser(&mut self, edge: bool) {
        if !self.native_enabled {
            self.notice =
                Some("Browser integration is available in the running desktop application.".into());
            return;
        }
        let id = self.extension_id.text.trim().to_owned();
        if id.len() != 32 || !id.bytes().all(|byte| (b'a'..=b'p').contains(&byte)) {
            self.notice = Some(
                "Paste the 32-character extension ID shown on your browser's extensions page."
                    .into(),
            );
            return;
        }
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let Some(repo) = services.repository.clone() else {
            return;
        };
        services.runtime.spawn_blocking(move || {
            let result = packaged_paths().and_then(|(host, app, _)| install_browser_host(if edge { BrowserKind::Edge } else { BrowserKind::Chrome }, &id, &host, &app, repo.directory()).map(|_| ()).map_err(|e| e.to_string()));
            handle.update(move |s, _| s.notice = Some(result.map(|_| "Browser host connected. Enable browser references and use the extension's Use this page action.".into()).unwrap_or_else(|e| e)));
        });
    }
}
