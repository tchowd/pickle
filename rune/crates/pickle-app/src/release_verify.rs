//! Read-only offline candidate verification. Dispatch before leases, repositories,
//! runtimes, vaults, windows or capture services; never download or execute anything.
use crate::update::{CandidateVerifier, MAX_ENVELOPE, Release};
use serde::Serialize;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

const VERIFY: &str = "--verify-update-manifest";
const FROM: &str = "--from-version";
const USAGE: &str = "Use only --verify-update-manifest FILE --from-version VERSION. No key, channel, target or candidate-version overrides are accepted.";

struct Request {
    manifest: PathBuf,
    from_version: String,
}

/// The successful output is deliberately restricted: never URLs, release notes,
/// local paths, environment values, credentials or selected application content.
#[derive(Debug, Serialize)]
struct Report {
    version: String,
    target: String,
    channel: String,
    sha256: String,
    size: u64,
}
impl From<Release> for Report {
    fn from(release: Release) -> Self {
        Self {
            version: release.version.clone(),
            target: release.artifact().target.clone(),
            channel: release.channel.clone(),
            sha256: release.artifact().sha256.clone(),
            size: release.artifact().size,
        }
    }
}

pub fn handle(arguments: &[String]) -> Option<i32> {
    let request = match parse(arguments) {
        Ok(Some(request)) => request,
        Ok(None) => return None,
        Err(error) => {
            eprintln!("{error}");
            return Some(2);
        }
    };
    match verify_file(&request) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string(&report).expect("fixed candidate report schema")
            );
            Some(0)
        }
        Err(error) => {
            eprintln!("{error}");
            Some(1)
        }
    }
}

fn parse(arguments: &[String]) -> Result<Option<Request>, &'static str> {
    let relevant = arguments.iter().any(|argument| {
        matches!(
            argument
                .split_once('=')
                .map_or(argument.as_str(), |(flag, _)| flag),
            VERIFY | FROM
        )
    });
    if !relevant {
        return Ok(None);
    }
    let mut manifest = None;
    let mut from_version = None;
    let mut index = 0;
    while index < arguments.len() {
        let (flag, value) = match arguments[index].split_once('=') {
            Some(pair) => pair,
            None => {
                let flag = arguments[index].as_str();
                index += 1;
                let value = arguments.get(index).ok_or(USAGE)?.as_str();
                (flag, value)
            }
        };
        if value.is_empty() || value.starts_with("--") {
            return Err(USAGE);
        }
        match flag {
            VERIFY if manifest.is_none() => manifest = Some(PathBuf::from(value)),
            FROM if from_version.is_none() => {
                semver::Version::parse(value)
                    .map_err(|_| "--from-version must be a valid semantic version.")?;
                from_version = Some(value.to_owned());
            }
            _ => return Err(USAGE),
        }
        index += 1;
    }
    Ok(Some(Request {
        manifest: manifest.ok_or(USAGE)?,
        from_version: from_version.ok_or(USAGE)?,
    }))
}

fn verify_file(request: &Request) -> Result<Report, String> {
    // Fail an unconfigured binary before even reading the supplied file. Only
    // this constructor can bind the opaque verifier used by this CLI.
    let verifier = CandidateVerifier::from_build_environment()?;
    let bytes = read_manifest(&request.manifest)?;
    verifier
        .verify(&bytes, &request.from_version)
        .map(Report::from)
}

fn read_manifest(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| "Could not inspect the candidate manifest.".to_string())?;
    if !metadata.is_file() {
        return Err("The candidate manifest must be a regular file.".into());
    }
    if metadata.len() > MAX_ENVELOPE as u64 {
        return Err("The candidate manifest exceeds 65,536 bytes.".into());
    }
    let file =
        File::open(path).map_err(|_| "Could not open the candidate manifest.".to_string())?;
    let opened = file
        .metadata()
        .map_err(|_| "Could not inspect the opened candidate manifest.".to_string())?;
    if !opened.is_file() || opened.len() > MAX_ENVELOPE as u64 {
        return Err(
            "The opened candidate manifest must be a regular file within 65,536 bytes.".into(),
        );
    }
    let mut bytes = Vec::new();
    file.take(MAX_ENVELOPE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the candidate manifest.".to_string())?;
    if bytes.len() > MAX_ENVELOPE {
        return Err("The candidate manifest exceeds 65,536 bytes.".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn only_the_isolated_complete_command_is_admitted() {
        assert!(parse(&args(&["--version"])).unwrap().is_none());
        assert!(
            parse(&args(&[VERIFY, "manifest.json", FROM, "0.0.0"]))
                .unwrap()
                .is_some()
        );
        assert!(
            parse(&args(&[
                "--from-version=0.0.0",
                "--verify-update-manifest=manifest.json"
            ]))
            .unwrap()
            .is_some()
        );
        for values in [
            vec![VERIFY],
            vec![FROM, "0.0.0"],
            vec![VERIFY, "file"],
            vec![VERIFY, "file", FROM, "invalid"],
            vec![VERIFY, "file", FROM, "0.0.0", "--version"],
            vec![VERIFY, "file", FROM, "0.0.0", "--capabilities"],
            vec![VERIFY, "file", FROM, "0.0.0", "--check-running"],
            vec![VERIFY, "file", FROM, "0.0.0", "--public-key", "00"],
            vec![VERIFY, "file", FROM, "0.0.0", "--channel=beta"],
            vec![VERIFY, "file", FROM, "0.0.0", VERIFY, "another"],
            vec![VERIFY, "file", FROM, "0.0.0", FROM, "0.0.1"],
        ] {
            assert!(parse(&args(&values)).is_err(), "{values:?}");
        }
    }

    #[test]
    fn bounded_reads_preserve_the_input_and_never_create_outputs() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("manifest.json");
        for size in [0, MAX_ENVELOPE, MAX_ENVELOPE + 1] {
            let bytes = vec![b' '; size];
            std::fs::write(&input, &bytes).unwrap();
            let result = read_manifest(&input);
            if size <= MAX_ENVELOPE {
                assert_eq!(result.unwrap(), bytes);
            } else {
                assert!(result.is_err());
            }
            assert_eq!(std::fs::read(&input).unwrap(), bytes);
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
        assert!(read_manifest(directory.path()).is_err());
        assert!(read_manifest(&directory.path().join("missing")).is_err());
    }

    #[test]
    fn unconfigured_or_mixed_commands_preserve_inputs_and_create_no_sibling_files() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = directory.path().join("input.json");
        std::fs::write(&manifest, b"untrusted manifest").unwrap();
        let command = vec![
            VERIFY.into(),
            manifest.to_string_lossy().into_owned(),
            FROM.into(),
            "0.0.0".into(),
        ];
        if matches!(crate::update::Config::from_build_environment(), Ok(None)) {
            assert_eq!(handle(&command), Some(1));
            let missing = Request {
                manifest: directory.path().join("missing.json"),
                from_version: "0.0.0".into(),
            };
            assert!(
                verify_file(&missing)
                    .unwrap_err()
                    .contains("no compiled update trust")
            );
        }
        let mut mixed = command;
        mixed.push("--background".into());
        assert_eq!(handle(&mixed), Some(2));
        assert_eq!(std::fs::read(&manifest).unwrap(), b"untrusted manifest");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert!(!directory.path().join("settings.json").exists());
        assert!(!directory.path().join("instance.lock").exists());
    }

    #[test]
    fn success_output_has_only_the_release_metadata_allowlist() {
        let release = Release::test_report_fixture();
        let report = serde_json::to_value(Report::from(release)).unwrap();
        let mut keys: Vec<_> = report
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["channel", "sha256", "size", "target", "version"]);
        let encoded = report.to_string();
        assert!(!encoded.contains("https://") && !encoded.contains("private fixture note"));
    }
}
