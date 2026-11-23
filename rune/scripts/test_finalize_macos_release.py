#!/usr/bin/env python3
"""Portable fixtures: no Apple tools, credentials, uploads, trust changes or app launches."""
import dataclasses
import importlib.util
import json
from pathlib import Path
import plistlib
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import zipfile

SPEC = importlib.util.spec_from_file_location("macos_release", Path(__file__).with_name("finalize-macos-release.py"))
release = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = release
SPEC.loader.exec_module(release)
SUBMISSION_ID = "12345678-1234-1234-1234-123456789abc"


class FixtureTools:
    def __init__(self):
        self.calls = []
        self.failure = None
        self.status = "Accepted"
        self.log_status = "Accepted"
        self.job_id = SUBMISSION_ID
        self.architecture = "arm64"
        self.descriptions = {}
        self.claims = {}
        self.strip_ticket = False
        self.issues = []

    def run(self, arguments, timeout=120):
        self.calls.append(arguments)
        tool = Path(arguments[0]).name
        action = tool
        if tool == "xcrun":
            action = "-".join(arguments[1:3])
        elif tool == "ditto":
            action = "zip" if "-c" in arguments else "extract" if "-x" in arguments else "copy"
        if self.failure == action:
            return subprocess.CompletedProcess(arguments, 1, b"fixture-private-error-output", b"")
        stdout, stderr = b"", b""
        if action == "copy":
            shutil.copytree(arguments[-2], arguments[-1])
        elif action == "zip":
            source = Path(arguments[-2])
            with zipfile.ZipFile(arguments[-1], "w") as archive:
                for path in source.rglob("*"):
                    if path.is_file():
                        archive.write(path, path.relative_to(source.parent))
        elif action == "extract":
            with zipfile.ZipFile(arguments[-2]) as archive:
                archive.extractall(arguments[-1])
            bundle = Path(arguments[-1]) / release.BUNDLE_NAME
            for relative in release.CODE_PATHS:
                (bundle / relative).chmod(0o755)
            if self.strip_ticket:
                (bundle / "fixture-ticket").unlink()
        elif action == "codesign" and "--display" in arguments:
            item = "host" if arguments[-1].endswith("pickle-browser-host") else "app"
            if "--entitlements" in arguments:
                stdout = plistlib.dumps(self.claims.get(item, {}))
            else:
                stderr = self.descriptions.get(item, (
                    "Identifier=com.pickle.reader.rune\n"
                    "CodeDirectory v=20500 size=123 flags=0x10000(runtime) hashes=1+2\n"
                    "Authority=Developer ID Application: Fixed Owner (ABCDEFGHIJ)\n"
                    "TeamIdentifier=ABCDEFGHIJ\nTimestamp=Nov 22, 2026 at 12:00:00\n"
                )).encode()
        elif action == "lipo":
            stdout = self.architecture.encode()
        elif action == "notarytool-submit":
            stdout = json.dumps({"id": SUBMISSION_ID, "status": self.status,
                                 "privateUnknownField": "not-retained-fixture-secret"}).encode()
        elif action == "notarytool-log":
            Path(arguments[-1]).write_text(json.dumps({
                "jobId": self.job_id, "status": self.log_status, "issues": self.issues,
                "privateUnknownField": "not-retained-fixture-secret"}))
        elif action == "stapler-staple":
            (Path(arguments[-1]) / "fixture-ticket").write_text("controlled ticket")
        elif action == "stapler-validate":
            if not (Path(arguments[-1]) / "fixture-ticket").is_file():
                return subprocess.CompletedProcess(arguments, 1, b"", b"")
        elif action not in ("codesign", "syspolicy_check", "spctl"):
            raise AssertionError(f"Unexpected fixture command: {arguments}")
        return subprocess.CompletedProcess(arguments, 0, stdout, stderr)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        bundle = self.root / "original.app"
        (bundle / "Contents/MacOS").mkdir(parents=True)
        for relative in release.CODE_PATHS:
            path = bundle / relative
            path.write_bytes(bytes.fromhex("cffaedfe") + b"controlled fixture code")
            path.chmod(0o755)
        (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps({
            "CFBundleIdentifier": release.BUNDLE_ID, "CFBundleExecutable": "PickleRune",
            "CFBundleShortVersionString": "1.2.3", "CFBundleVersion": "123"}))
        self.options = release.Options(bundle, self.root / "output", "ABCDEFGHIJ", "1.2.3",
                                       "arm64", "fixture-profile", 60)
        self.tools = FixtureTools()
        self.original = self.snapshot()

    def snapshot(self):
        return {path.relative_to(self.options.bundle).as_posix():
                (path.read_bytes(), stat.S_IMODE(path.stat().st_mode))
                for path in self.options.bundle.rglob("*") if path.is_file()}

    def assert_clean_failure(self):
        self.assertEqual(self.snapshot(), self.original)
        if self.options.output.exists():
            self.assertFalse(list(self.options.output.glob(".release-work-*")))
            self.assertFalse(list(self.options.output.glob("*.zip")))
            self.assertFalse(list(self.options.output.glob("*.app")))
            self.assertFalse((self.options.output / "SHA256SUMS").exists())
            self.assertFalse((self.options.output / "macos-release-report.json").exists())

    def test_success_orders_all_gates_before_publishing_and_preserves_input(self):
        report = release.finalize(self.options, self.tools)
        self.assertEqual(self.snapshot(), self.original)
        self.assertTrue(report["passed"])
        self.assertEqual(report["notarization"]["status"], "Accepted")
        self.assertIn("no app launch", report["scope"])
        self.assertFalse(list(self.options.output.glob(".release-work-*")))
        archive = self.options.output / report["artifact"]["filename"]
        self.assertEqual(release.digest(archive), report["artifact"]["sha256"])
        self.assertIn(report["artifact"]["sha256"], (self.options.output / "SHA256SUMS").read_text())
        with zipfile.ZipFile(archive) as zipped:
            self.assertIn(release.BUNDLE_NAME + "/fixture-ticket", zipped.namelist())
        calls = self.tools.calls
        submit = next(i for i, cmd in enumerate(calls) if cmd[1:3] == ["notarytool", "submit"])
        staple = next(i for i, cmd in enumerate(calls) if cmd[1:3] == ["stapler", "staple"])
        extraction = next(i for i, cmd in enumerate(calls) if "-x" in cmd)
        policy = next(i for i, cmd in enumerate(calls) if cmd[0].endswith("syspolicy_check"))
        self.assertLess(submit, staple)
        self.assertLess(staple, extraction)
        self.assertLess(extraction, policy)
        self.assertTrue(any("--verify" in cmd and "--deep" in cmd for cmd in calls[:submit]))
        self.assertEqual(sum(cmd[0].endswith("codesign") and "-R" in cmd for cmd in calls), 4)
        for command in calls:
            self.assertNotIn("--sign", command)
            self.assertNotIn("--password", command)
            self.assertNotIn("--apple-id", command)
            if "-R" in command:
                requirement = command[command.index("-R") + 1]
                self.assertTrue(requirement.startswith("=anchor apple generic"))
                self.assertIn("1.2.840.113635.100.6.2.6", requirement)
                self.assertIn("1.2.840.113635.100.6.1.13", requirement)
                self.assertIn('subject.OU] = "ABCDEFGHIJ"', requirement)

    def test_ad_hoc_wrong_team_missing_runtime_and_missing_timestamp_fail_before_upload(self):
        normal = "Authority=Developer ID Application: Owner\nTeamIdentifier=ABCDEFGHIJ\nflags=0x10000(runtime)\nTimestamp=Nov 22, 2026\n"
        variants = [normal.replace("Developer ID Application:", "Apple Development:"),
                    normal.replace("ABCDEFGHIJ", "KLMNOPQRST"),
                    normal.replace("flags=0x10000(runtime)", "flags=0x0(none)"),
                    normal.replace("Timestamp=Nov 22, 2026", "Signed Time=Nov 22, 2026")]
        for index, value in enumerate(variants):
            with self.subTest(index=index):
                options = dataclasses.replace(self.options, output=self.root / f"reject-{index}")
                tools = FixtureTools()
                tools.descriptions["host"] = value
                with self.assertRaises(release.ReleaseError):
                    release.finalize(options, tools)
                self.assertFalse(any(cmd[1:3] == ["notarytool", "submit"] for cmd in tools.calls))
                self.assertFalse(list(options.output.iterdir()))
        self.assertEqual(self.snapshot(), self.original)

    def test_signature_architecture_and_debug_entitlement_rejections(self):
        for failure in ("codesign", "architecture", "entitlement"):
            with self.subTest(failure=failure):
                options = dataclasses.replace(self.options, output=self.root / failure)
                tools = FixtureTools()
                if failure == "architecture":
                    tools.architecture = "x86_64"
                elif failure == "entitlement":
                    tools.claims["app"] = {"com.apple.security.get-task-allow": True}
                else:
                    tools.failure = "codesign"
                with self.assertRaises(release.ReleaseError):
                    release.finalize(options, tools)
                self.assertFalse(list(options.output.iterdir()))
        self.assertEqual(self.snapshot(), self.original)

    def test_every_notary_and_archive_policy_failure_cleans_temporary_payload(self):
        for index, failure in enumerate(("notarytool-submit", "notarytool-log", "stapler-staple",
                                          "stapler-validate", "extract", "syspolicy_check", "spctl")):
            with self.subTest(failure=failure):
                self.options = dataclasses.replace(self.options, output=self.root / f"failure-{index}")
                self.tools = FixtureTools()
                self.tools.failure = failure
                with self.assertRaises(release.ReleaseError):
                    release.finalize(self.options, self.tools)
                self.assert_clean_failure()

    def test_exact_acceptance_and_matching_log_are_required(self):
        for index, (status, log_status, job_id) in enumerate((
            ("Invalid", "Invalid", SUBMISSION_ID),
            ("accepted", "Accepted", SUBMISSION_ID),
            ("Accepted", "Invalid", SUBMISSION_ID),
            ("Accepted", "Accepted", "another-job"),
        )):
            with self.subTest(index=index):
                self.options = dataclasses.replace(self.options, output=self.root / f"status-{index}")
                self.tools = FixtureTools()
                self.tools.status, self.tools.log_status, self.tools.job_id = status, log_status, job_id
                with self.assertRaises(release.ReleaseError):
                    release.finalize(self.options, self.tools)
                self.assert_clean_failure()
                self.assertTrue((self.options.output / "notarization-submission.json").exists())
                self.assertFalse(any(cmd[1:3] == ["stapler", "staple"] for cmd in self.tools.calls))

    def test_final_archive_must_retain_the_stapled_ticket(self):
        self.tools.strip_ticket = True
        with self.assertRaisesRegex(release.ReleaseError, "Extracted ticket"):
            release.finalize(self.options, self.tools)
        self.assert_clean_failure()

    def test_accepted_notarization_cannot_publish_zip_over_update_size_limit(self):
        with mock.patch.object(release, "MAX_ARTIFACT", 1):
            with self.assertRaisesRegex(release.ReleaseError, "512 MiB"):
                release.finalize(self.options, self.tools)
        self.assert_clean_failure()
        self.assertTrue(any(cmd[1:3] == ["stapler", "staple"] for cmd in self.tools.calls))
        self.assertFalse(any("-x" in cmd for cmd in self.tools.calls))
        submission = json.loads((self.options.output / "notarization-submission.json").read_text())
        self.assertEqual(submission["status"], "Accepted")

    def test_final_report_write_failure_removes_all_promoted_artifacts(self):
        original_write = release.write_json
        def fail_report(path, value):
            if path.name == "macos-release-report.json":
                path.write_bytes(b"partial report")
                raise OSError("fixed disk failure")
            return original_write(path, value)
        with mock.patch.object(release, "write_json", side_effect=fail_report):
            with self.assertRaises(OSError):
                release.finalize(self.options, self.tools)
        self.assert_clean_failure()
        self.assertTrue((self.options.output / "notarization-log.json").exists())

    def test_notary_uuid_case_is_normalized_without_accepting_another_job(self):
        self.tools.job_id = SUBMISSION_ID.upper()
        report = release.finalize(self.options, self.tools)
        self.assertEqual(report["notarization"]["submission_id"], SUBMISSION_ID)
        self.assertFalse(report["clean_machine_tested"])
        self.assertIn("not attested", report["source_provenance"])

    def test_an_error_in_an_accepted_log_still_prevents_publication(self):
        self.tools.issues = [{"severity": "error", "message": "Fixed contradictory log."}]
        with self.assertRaises(release.ReleaseError):
            release.finalize(self.options, self.tools)
        self.assert_clean_failure()
        log = json.loads((self.options.output / "notarization-log.json").read_text())
        self.assertEqual(log["error_count"], 1)

    def test_retained_log_is_bounded_and_excludes_unknown_fields(self):
        self.tools.status = self.tools.log_status = "Invalid"
        self.tools.issues = [{"severity": "error", "message": "x" * 2000,
                              "path": "y" * 2000, "privateField": "not-retained-fixture-secret"}] * 100
        with self.assertRaises(release.ReleaseError):
            release.finalize(self.options, self.tools)
        self.assert_clean_failure()
        path = self.options.output / "notarization-log.json"
        self.assertLessEqual(path.stat().st_size, release.MAX_NOTARY_LOG)
        retained = json.loads(path.read_text())
        self.assertEqual(retained["issue_count"], 100)
        self.assertEqual(retained["omitted_issues"], 68)
        self.assertNotIn("not-retained-fixture-secret", path.read_text())
        self.assertNotIn("not-retained-fixture-secret", (self.options.output / "notarization-submission.json").read_text())

    def test_existing_or_input_nested_output_is_refused_without_commands(self):
        self.options.output.mkdir()
        sentinel = self.options.output / "retained"
        sentinel.write_bytes(b"retained")
        with self.assertRaises(release.ReleaseError):
            release.finalize(self.options, self.tools)
        self.assertEqual(sentinel.read_bytes(), b"retained")
        nested = dataclasses.replace(self.options, output=self.options.bundle / "output")
        with self.assertRaises(release.ReleaseError):
            release.finalize(nested, self.tools)
        self.assertFalse(self.tools.calls)
        self.assertEqual(self.snapshot(), self.original)

    def test_metadata_extra_code_and_missing_payload_fail_before_any_tool(self):
        for change in ("version", "missing", "extra"):
            with self.subTest(change=change):
                bundle = self.root / f"bad-{change}.app"
                shutil.copytree(self.options.bundle, bundle)
                if change == "version":
                    info = bundle / "Contents/Info.plist"
                    data = plistlib.loads(info.read_bytes())
                    data["CFBundleShortVersionString"] = "9.9.9"
                    info.write_bytes(plistlib.dumps(data))
                elif change == "missing":
                    (bundle / release.CODE_PATHS[1]).unlink()
                else:
                    (bundle / "unexpected-code").write_bytes(bytes.fromhex("cffaedfe"))
                with self.assertRaises(release.ReleaseError):
                    release.finalize(dataclasses.replace(self.options, bundle=bundle), self.tools)
                self.assertFalse(self.tools.calls)

    @unittest.skipIf(sys.platform == "win32", "Windows fixture users may not have symlink privileges")
    def test_escaping_payload_symlink_is_refused_before_copy(self):
        (self.options.bundle / "external").symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(release.ReleaseError):
            release.finalize(self.options, self.tools)
        self.assertFalse(self.tools.calls)


if __name__ == "__main__":
    unittest.main()
