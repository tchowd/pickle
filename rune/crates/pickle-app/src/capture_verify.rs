//! Explicit, isolated fixed-window diagnostic. Dispatch before argument
//! normalization, helpers, repositories, instance leases, services or Rune.
use pickle_platform::PlatformError;
use serde::Deserialize;
use serde_json::Value;

const VERIFY: &str = "--verify-windows-capture";
const FIXTURE: &str = "--windows-capture-owned-fixture";
const USAGE: &str = "The owned-window capture diagnostic requires exactly one exclusive option.";
const FAILED: &str = "Windows owned-window capture verification failed.";

struct Response {
    code: i32,
    stdout: Option<String>,
    stderr: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureReport {
    schema: u8,
    test: String,
    passed: bool,
    package_identity: bool,
    production_window_capture: bool,
    expected_word_matched: bool,
    jpeg_bytes: u64,
    width: u32,
    height: u32,
    dark_pixels: u64,
    painted_before_capture: bool,
    owned_window_closed: bool,
    fixture_thread_joined: bool,
    fixture_process_stopped: bool,
    native_capture: NativeCapture,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeCapture {
    stage: String,
    hresult: i32,
    supported: Option<bool>,
    frame_arrived: bool,
    width: u32,
    height: u32,
    driver_type: i32,
}
fn valid_report(value: &Value) -> bool {
    let Ok(report) = serde_json::from_value::<CaptureReport>(value.clone()) else {
        return false;
    };
    report.schema == 1
        && report.test == "windows-owned-window-capture"
        && report.passed
        && report.package_identity
        && report.production_window_capture
        && report.expected_word_matched
        && report.painted_before_capture
        && report.owned_window_closed
        && report.fixture_thread_joined
        && report.fixture_process_stopped
        && (1..=8 * 1024 * 1024).contains(&report.jpeg_bytes)
        && report.width == 768
        && report.height == 180
        && (1..=u64::from(report.width) * u64::from(report.height)).contains(&report.dark_pixels)
        && report.native_capture.stage == "complete"
        && report.native_capture.hresult == 0
        && report.native_capture.supported == Some(true)
        && report.native_capture.frame_arrived
        && report.native_capture.width == report.width
        && report.native_capture.height == report.height
        && report.native_capture.driver_type == 1
}

pub fn handle(arguments: &[String]) -> Option<i32> {
    let response = dispatch(
        arguments,
        pickle_platform::verify_window_capture_runtime,
        pickle_platform::run_window_capture_fixture,
    )?;
    if let Some(output) = response.stdout {
        println!("{output}");
    }
    if let Some(error) = response.stderr {
        eprintln!("{error}");
    }
    Some(response.code)
}

fn dispatch(
    arguments: &[String],
    verify: impl FnOnce() -> Result<Value, PlatformError>,
    fixture: impl FnOnce() -> Result<(), PlatformError>,
) -> Option<Response> {
    let relevant = arguments
        .iter()
        .any(|argument| argument.starts_with(VERIFY) || argument.starts_with(FIXTURE));
    if !relevant {
        return None;
    }
    let failure = |code, error: &str| Response {
        code,
        stdout: None,
        stderr: Some(error.into()),
    };
    if arguments.len() != 1 || !matches!(arguments[0].as_str(), VERIFY | FIXTURE) {
        return Some(failure(2, USAGE));
    }
    let result = if arguments[0] == FIXTURE {
        // The platform owns its small ready/close protocol on stdout. It must
        // never be replaced by the public successful capture-report envelope.
        fixture().map(|()| None)
    } else {
        verify().and_then(|report| {
            let output =
                serde_json::to_string(&report).map_err(|_| PlatformError("report".into()))?;
            if output.len() > 16_384 || !valid_report(&report) {
                return Err(PlatformError("report".into()));
            }
            Ok(Some(output))
        })
    };
    Some(match result {
        Ok(stdout) => Response {
            code: 0,
            stdout,
            stderr: None,
        },
        Err(error) => {
            // Only the native module's fixed stage categories may reach the
            // terminal. Never echo arbitrary OS, path, source or input errors.
            let category = match error.0.as_str() {
                "identity" | "fixture-start" | "paint" | "target" | "capture" | "image"
                | "word" | "cleanup" | "report" => error.0.as_str(),
                _ => "unavailable",
            };
            failure(1, &format!("{FAILED} ({category})"))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn args(arguments: &[&str]) -> Vec<String> {
        arguments.iter().map(|value| (*value).into()).collect()
    }
    fn report_fixture() -> Value {
        serde_json::json!({"schema":1,"test":"windows-owned-window-capture","passed":true,"package_identity":true,"production_window_capture":true,"expected_word_matched":true,"jpeg_bytes":4321,"width":768,"height":180,"dark_pixels":100,"painted_before_capture":true,"owned_window_closed":true,"fixture_thread_joined":true,"fixture_process_stopped":true,"native_capture":{"stage":"complete","hresult":0,"supported":true,"frame_arrived":true,"width":768,"height":180,"driver_type":1}})
    }

    #[test]
    fn mixed_or_valued_flags_never_call_either_native_entrypoint() {
        for arguments in [
            vec![VERIFY, "--version"],
            vec!["--version", VERIFY],
            vec![VERIFY, "--verify-windows-ocr"],
            vec![VERIFY, FIXTURE],
            vec![FIXTURE, "--background"],
            vec![FIXTURE, FIXTURE],
            vec![VERIFY, "--scene=sample"],
            vec![VERIFY, "--from-version=0.0.0"],
            vec!["--verify-windows-capture=1"],
            vec!["--windows-capture-owned-fixture="],
            vec!["--verify-windows-capture-extra"],
            vec!["--windows-capture-owned-fixture/123"],
            vec![VERIFY, "12345"],
            vec![FIXTURE, "https://example.invalid"],
        ] {
            let calls = Cell::new(0);
            let result = dispatch(
                &args(&arguments),
                || {
                    calls.set(calls.get() + 1);
                    unreachable!()
                },
                || {
                    calls.set(calls.get() + 1);
                    unreachable!()
                },
            )
            .unwrap();
            assert_eq!(calls.get(), 0, "{arguments:?}");
            assert_eq!(result.code, 2);
            assert!(result.stdout.is_none());
        }
    }

    #[test]
    fn successful_public_report_is_native_result_and_helper_has_no_public_report() {
        let report = report_fixture();
        let result = dispatch(&args(&[VERIFY]), || Ok(report.clone()), || unreachable!()).unwrap();
        assert_eq!(result.code, 0);
        assert_eq!(
            serde_json::from_str::<Value>(&result.stdout.unwrap()).unwrap(),
            report
        );
        let result = dispatch(&args(&[FIXTURE]), || unreachable!(), || Ok(())).unwrap();
        assert_eq!(result.code, 0);
        assert!(result.stdout.is_none());
    }

    #[test]
    fn failed_or_invalid_reports_cannot_emit_success_or_arbitrary_errors() {
        for report in [
            serde_json::json!({"schema":1,"test":"windows-owned-window-capture","passed":false}),
            serde_json::json!({"schema":1,"test":"other","passed":true}),
            serde_json::json!({"schema":1,"test":"windows-owned-window-capture","passed":true,"extra":"x".repeat(16384)}),
        ] {
            let result = dispatch(&args(&[VERIFY]), || Ok(report), || unreachable!()).unwrap();
            assert_eq!(result.code, 1);
            assert!(result.stdout.is_none());
        }
        let result = dispatch(
            &args(&[VERIFY]),
            || Err(PlatformError("private path or arbitrary source".into())),
            || unreachable!(),
        )
        .unwrap();
        assert_eq!(result.code, 1);
        assert!(result.stdout.is_none());
        assert_eq!(
            result.stderr.as_deref(),
            Some("Windows owned-window capture verification failed. (unavailable)")
        );
        let result = dispatch(
            &args(&[FIXTURE]),
            || unreachable!(),
            || Err(PlatformError("identity".into())),
        )
        .unwrap();
        assert_eq!(result.code, 1);
        assert!(result.stdout.is_none());
        assert!(result.stderr.unwrap().ends_with("(identity)"));
    }

    #[test]
    fn unrelated_commands_have_no_diagnostic_effects() {
        for arguments in [
            vec![],
            vec!["--version"],
            vec!["--capabilities"],
            vec!["--scene", "sample"],
        ] {
            assert!(dispatch(&args(&arguments), || unreachable!(), || unreachable!()).is_none());
        }
    }

    #[test]
    fn report_requires_actual_frame_hardware_word_and_cleanup_metadata() {
        for (key, value) in [
            ("fixture_process_stopped", serde_json::json!(false)),
            ("production_window_capture", serde_json::json!(1)),
            ("width", serde_json::json!("768")),
            ("dark_pixels", serde_json::json!(0)),
            ("source", serde_json::json!("unexpected source field")),
        ] {
            let mut report = report_fixture();
            report[key] = value;
            assert!(!valid_report(&report), "{key}");
        }
        for (key, value) in [
            ("frame_arrived", serde_json::json!(false)),
            ("supported", Value::Null),
            ("hresult", serde_json::json!(-1)),
            ("driver_type", serde_json::json!(5)),
            ("path", serde_json::json!("unrecognized field")),
        ] {
            let mut report = report_fixture();
            report["native_capture"][key] = value;
            assert!(!valid_report(&report), "{key}");
        }
    }
}
