#!/usr/bin/env python3
"""Finalize an existing Developer ID-signed Pickle bundle; never sign or install it.

The CLI always uses Apple's tools. Tests inject a runner into finalize() without
launching applications, accessing credentials, or contacting the notary service.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import uuid


MAX_OUTPUT = 1024 * 1024
MAX_NOTARY_LOG = 128 * 1024
MAX_ARTIFACT = 512 * 1024 * 1024
BUNDLE_NAME = "Pickle Rune.app"
BUNDLE_ID = "com.pickle.reader.rune"
CODE_PATHS = ("Contents/MacOS/PickleRune", "Contents/MacOS/pickle-browser-host")
MACH_MAGICS = {
    bytes.fromhex(value) for value in
    ("feedface", "cefaedfe", "feedfacf", "cffaedfe", "cafebabe", "bebafeca", "cafebabf", "bfbafeca")
}


class ReleaseError(Exception):
    pass


@dataclass(frozen=True)
class Options:
    bundle: Path
    output: Path
    team_id: str
    version: str
    architecture: str
    keychain_profile: str
    wait_seconds: int = 1800


class AppleTools:
    def run(self, arguments: list[str], timeout: int = 120) -> subprocess.CompletedProcess:
        # Avoid unbounded captured output in memory. Never echo tool output:
        # authentication failures may contain account information.
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            try:
                result = subprocess.run(arguments, stdin=subprocess.DEVNULL,
                                        stdout=stdout, stderr=stderr, timeout=timeout, check=False,
                                        env={**os.environ, "LC_ALL": "C"})
            except (OSError, subprocess.TimeoutExpired) as error:
                raise ReleaseError("An Apple release tool was unavailable or timed out.") from error
            captured = []
            for stream in (stdout, stderr):
                stream.seek(0)
                value = stream.read(MAX_OUTPUT + 1)
                if len(value) > MAX_OUTPUT:
                    raise ReleaseError("An Apple release tool exceeded its output budget.")
                captured.append(value)
            return subprocess.CompletedProcess(arguments, result.returncode, *captured)


def checked(runner, arguments: list[str], label: str) -> subprocess.CompletedProcess:
    result = runner.run(arguments)
    if result.returncode:
        raise ReleaseError(f"{label} failed (exit {result.returncode}).")
    return result


def bounded_json(path: Path) -> dict:
    with path.open("rb") as stream:
        raw = stream.read(MAX_OUTPUT + 1)
    if len(raw) > MAX_OUTPUT:
        raise ReleaseError("The notarization response exceeded its size budget.")
    try:
        value = json.loads(raw)
    except (ValueError, UnicodeError) as error:
        raise ReleaseError("The notarization response was not valid JSON.") from error
    if not isinstance(value, dict):
        raise ReleaseError("The notarization response was not an object.")
    return value


def write_json(path: Path, value: dict) -> None:
    encoded = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
    if len(encoded) > MAX_NOTARY_LOG:
        raise ReleaseError("The release metadata exceeded its size budget.")
    path.write_bytes(encoded)


def validate_options(options: Options) -> tuple[Path, Path]:
    if not re.fullmatch(r"[A-Z0-9]{10}", options.team_id):
        raise ReleaseError("Supply the expected ten-character Apple Team ID.")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", options.version):
        raise ReleaseError("Supply the expected numeric major.minor.patch release version.")
    if options.architecture not in ("arm64", "x86_64"):
        raise ReleaseError("Select an explicit arm64 or x86_64 release architecture.")
    if not (1 <= len(options.keychain_profile) <= 128) or any(
            ord(character) < 32 for character in options.keychain_profile):
        raise ReleaseError("Supply an existing notarytool keychain-profile name.")
    if not 60 <= options.wait_seconds <= 3600:
        raise ReleaseError("The notarization wait must be between 60 and 3600 seconds.")
    if options.bundle.is_symlink() or not options.bundle.is_dir() or options.bundle.suffix != ".app":
        raise ReleaseError("The input must be an existing, nonsymlink application bundle.")
    bundle = options.bundle.resolve(strict=True)
    output = options.output.resolve()
    if options.output.exists() or options.output.is_symlink():
        raise ReleaseError("The output directory must not already exist.")
    if output.is_relative_to(bundle):
        raise ReleaseError("The output directory cannot be inside the input bundle.")
    if not output.parent.is_dir():
        raise ReleaseError("The output parent directory must already exist.")
    return bundle, output


def inspect_layout(bundle: Path, options: Options) -> dict:
    # This product currently contains exactly two standalone Mach-O executables
    # and no symlinked frameworks. New native payloads require explicit review.
    discovered = set()
    for parent, directories, files in os.walk(bundle, followlinks=False):
        for name in directories + files:
            path = Path(parent) / name
            mode = path.lstat().st_mode
            if stat.S_ISLNK(mode) or not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
                raise ReleaseError("The bundle contains an unsupported symlink or special file.")
            if stat.S_ISREG(mode):
                with path.open("rb") as stream:
                    if stream.read(4) in MACH_MAGICS:
                        discovered.add(path.relative_to(bundle).as_posix())
    if discovered != set(CODE_PATHS):
        raise ReleaseError("The bundle must contain exactly the reviewed app and browser-host Mach-O code.")
    for relative in CODE_PATHS:
        path = bundle / relative
        if not path.is_file() or not os.access(path, os.X_OK):
            raise ReleaseError("A required packaged executable is missing or not executable.")
    try:
        with (bundle / "Contents/Info.plist").open("rb") as stream:
            info = plistlib.load(stream)
    except (OSError, ValueError, plistlib.InvalidFileException) as error:
        raise ReleaseError("The bundle Info.plist is invalid.") from error
    if not isinstance(info, dict):
        raise ReleaseError("The bundle Info.plist must be a dictionary.")
    if (info.get("CFBundleIdentifier") != BUNDLE_ID
            or info.get("CFBundleExecutable") != "PickleRune"
            or info.get("CFBundleShortVersionString") != options.version):
        raise ReleaseError("The bundle identity, executable, or version differs from the expected release.")
    build = info.get("CFBundleVersion")
    if not isinstance(build, str) or not re.fullmatch(r"[0-9]+(?:\.[0-9]+){0,2}", build):
        raise ReleaseError("The bundle build number is invalid.")
    return {"bundle_id": BUNDLE_ID, "version": options.version, "build": build}


def inspect_signatures(bundle: Path, options: Options, runner) -> list[dict]:
    # Apple Developer ID Application certificate extension, Apple trust anchor,
    # and exact Team ID; never accept a merely matching human-readable CN.
    requirement = ("anchor apple generic and "
                   "certificate 1[field.1.2.840.113635.100.6.2.6] exists and "
                   "certificate leaf[field.1.2.840.113635.100.6.1.13] exists and "
                   f'certificate leaf[subject.OU] = "{options.team_id}"')
    records = []
    for relative in CODE_PATHS:
        executable = bundle / relative
        target = bundle if relative == CODE_PATHS[0] else executable
        code_requirement = requirement
        if relative == CODE_PATHS[0]:
            code_requirement += f' and identifier "{BUNDLE_ID}"'
        checked(runner, ["/usr/bin/codesign", "--verify", "--strict", "--all-architectures",
                         "-R", "=" + code_requirement, str(target)], "Developer ID signature verification")
        description = checked(runner, ["/usr/bin/codesign", "--display", "--verbose=4", str(target)],
                              "Code-signature inspection")
        text = (description.stdout + description.stderr).decode("utf-8", errors="strict")
        flags = re.search(r"\bflags=0x([0-9a-fA-F]+)\b", text)
        if (f"TeamIdentifier={options.team_id}" not in text.splitlines()
                or not any(line.startswith("Authority=Developer ID Application:") for line in text.splitlines())
                or not flags or not int(flags[1], 16) & 0x10000
                or not re.search(r"(?m)^Timestamp=\S.+$", text)):
            raise ReleaseError("Developer ID, expected team, hardened runtime, and secure timestamp are required on both executables.")
        entitlements = checked(runner, ["/usr/bin/codesign", "--display", "--entitlements", "-", "--xml", str(target)],
                               "Entitlement inspection").stdout
        try:
            claims = plistlib.loads(entitlements) if entitlements.strip() else {}
        except (ValueError, plistlib.InvalidFileException) as error:
            raise ReleaseError("The signed entitlements are invalid.") from error
        if not isinstance(claims, dict) or claims.get("com.apple.security.get-task-allow"):
            raise ReleaseError("Release code must not enable debugger attachment.")
        architecture = checked(runner, ["/usr/bin/lipo", "-archs", str(executable)],
                               "Mach-O architecture inspection").stdout.decode().split()
        if architecture != [options.architecture]:
            raise ReleaseError("Both packaged executables must have exactly the requested architecture.")
        records.append({"path": relative, "team_id": options.team_id,
                        "architecture": options.architecture, "hardened_runtime": True,
                        "secure_timestamp": True, "entitlement_keys": sorted(claims)})
    checked(runner, ["/usr/bin/codesign", "--verify", "--deep", "--strict", "--all-architectures", str(bundle)],
            "Nested code integrity verification")
    return records


def sanitize_log(raw: dict, submission_id: str) -> dict:
    try:
        log_id = str(uuid.UUID(raw["jobId"]))
    except (KeyError, ValueError, TypeError, AttributeError) as error:
        raise ReleaseError("The notarization log has no valid submission identifier.") from error
    if log_id != submission_id:
        raise ReleaseError("The notarization log belongs to another submission.")
    def limited(value, length=512):
        return value[:length] if isinstance(value, str) else None
    issues = raw.get("issues") or []
    if not isinstance(issues, list) or not all(isinstance(item, dict) for item in issues):
        raise ReleaseError("The notarization issue list is invalid.")
    return {"submission_id": submission_id, "status": limited(raw.get("status"), 64),
            "issue_count": len(issues), "omitted_issues": max(0, len(issues) - 32),
            "error_count": sum(issue.get("severity") == "error" for issue in issues),
            "warning_count": sum(issue.get("severity") == "warning" for issue in issues),
            "issues": [{key: limited(issue.get(key)) for key in
                        ("severity", "message", "path", "architecture", "docUrl") if key in issue}
                       for issue in issues[:32]]}


def notarize(archive: Path, options: Options, output: Path, work: Path, runner) -> dict:
    result = runner.run(["/usr/bin/xcrun", "notarytool", "submit", str(archive),
                         "--keychain-profile", options.keychain_profile, "--wait", "--timeout",
                         f"{options.wait_seconds}s", "--output-format", "json"],
                        timeout=options.wait_seconds + 120)
    response_file = work / "submission-response.json"
    response_file.write_bytes(result.stdout)
    response = bounded_json(response_file)
    try:
        identifier = str(uuid.UUID(response["id"]))
    except (KeyError, ValueError, TypeError, AttributeError) as error:
        raise ReleaseError("The notarization response has no valid submission identifier.") from error
    status = response.get("status")
    if not isinstance(status, str) or len(status) > 64:
        raise ReleaseError("The notarization response has no bounded status.")
    write_json(output / "notarization-submission.json", {"submission_id": identifier, "status": status})
    raw_log = work / "notary-log.json"
    checked(runner, ["/usr/bin/xcrun", "notarytool", "log", identifier,
                     "--keychain-profile", options.keychain_profile, str(raw_log)], "Notarization log retrieval")
    log = sanitize_log(bounded_json(raw_log), identifier)
    write_json(output / "notarization-log.json", log)
    if result.returncode or status != "Accepted" or log["status"] != "Accepted" or log["error_count"]:
        raise ReleaseError("Apple did not accept this exact notarization submission. No release was published.")
    return {"submission_id": identifier, "status": "Accepted", "issue_count": log["issue_count"],
            "warning_count": log["warning_count"]}


def digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def finalize(options: Options, runner) -> dict:
    source, output = validate_options(options)
    # Reject escaping symlinks before asking ditto to copy any input payload.
    inspect_layout(source, options)
    output.mkdir(mode=0o700)
    with tempfile.TemporaryDirectory(prefix=".release-work-", dir=output) as temporary:
        work = Path(temporary)
        staged = work / BUNDLE_NAME
        checked(runner, ["/usr/bin/ditto", str(source), str(staged)], "Bundle staging")
        metadata = inspect_layout(staged, options)
        inspect_signatures(staged, options, runner)
        submission = work / "notary-submission.zip"
        checked(runner, ["/usr/bin/ditto", "-c", "-k", "--sequesterRsrc", "--keepParent",
                         str(staged), str(submission)], "Notarization archive creation")
        notarization = notarize(submission, options, output, work, runner)
        checked(runner, ["/usr/bin/xcrun", "stapler", "staple", str(staged)], "Ticket stapling")
        checked(runner, ["/usr/bin/xcrun", "stapler", "validate", str(staged)], "Stapled ticket validation")
        name = f"PickleRune-{options.version}-macos-{options.architecture}.zip"
        archive = work / name
        checked(runner, ["/usr/bin/ditto", "-c", "-k", "--sequesterRsrc", "--keepParent",
                         str(staged), str(archive)], "Stapled release archive creation")
        archive_size = archive.stat().st_size
        if not 0 < archive_size <= MAX_ARTIFACT:
            raise ReleaseError("The final ZIP must be nonempty and at most 512 MiB, matching the update download limit.")
        extracted = work / "extracted"
        checked(runner, ["/usr/bin/ditto", "-x", "-k", str(archive), str(extracted)], "Final archive extraction")
        final_bundle = extracted / BUNDLE_NAME
        if not extracted.is_dir() or sorted(path.name for path in extracted.iterdir()) != [BUNDLE_NAME]:
            raise ReleaseError("The final release archive has unexpected top-level contents.")
        inspect_layout(final_bundle, options)
        signatures = inspect_signatures(final_bundle, options, runner)
        checked(runner, ["/usr/bin/xcrun", "stapler", "validate", str(final_bundle)], "Extracted ticket validation")
        checked(runner, ["/usr/bin/syspolicy_check", "distribution", str(final_bundle)], "System distribution policy assessment")
        checked(runner, ["/usr/sbin/spctl", "--assess", "--type", "execute", "--verbose=2", str(final_bundle)],
                "Gatekeeper execution assessment")
        for record in signatures:
            record["sha256"] = digest(final_bundle / record["path"])
        artifact = {"filename": name, "size": archive_size, "sha256": digest(archive)}
        report = {"schema": 1, "test": "macos-release-finalization", "passed": True,
                  **metadata, "team_id": options.team_id, "architecture": options.architecture,
                  "notarization": notarization, "binaries": signatures, "artifact": artifact,
                  "gatekeeper_assessment": True, "stapled_ticket_validated_after_extraction": True,
                  "clean_machine_tested": False,
                  "source_provenance": "Owner-supplied signed bundle; source review and build reproducibility are not attested by this finalizer.",
                  "scope": "Signature, notarization, stapling, archive round trip and system policy; no app launch or native feature acceptance."}
        # Nothing called a release artifact/report/checksum is exposed until all
        # gates passed. The original bundle is never a destination of any tool.
        promoted = []
        try:
            for source_path, destination in ((archive, output / name), (staged, output / BUNDLE_NAME)):
                promoted.append(destination)
                os.replace(source_path, destination)
            checksum = output / "SHA256SUMS"
            promoted.append(checksum)
            checksum.write_text(f"{artifact['sha256']}  {name}\n")
            report_path = output / "macos-release-report.json"
            promoted.append(report_path)
            write_json(report_path, report)
        except BaseException:
            # A disk failure during final promotion must not leave a partial
            # release looking usable. Retain only the bounded notary evidence.
            for path in reversed(promoted):
                if path.is_dir() and not path.is_symlink():
                    shutil.rmtree(path)
                else:
                    path.unlink(missing_ok=True)
            raise
        return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path, help="New, nonexistent output directory")
    parser.add_argument("--team-id", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--architecture", choices=("arm64", "x86_64"), required=True)
    parser.add_argument("--keychain-profile", required=True, help="Existing notarytool Keychain profile; no credentials are stored")
    parser.add_argument("--wait-seconds", type=int, default=1800)
    arguments = parser.parse_args()
    if sys.platform != "darwin":
        print("Mac release finalization requires macOS 14 or later and Apple's tools.", file=sys.stderr)
        return 1
    try:
        finalize(Options(**vars(arguments)), AppleTools())
    except (ReleaseError, OSError, UnicodeError) as error:
        # All ReleaseError messages are ours, without captured command output.
        message = str(error) if isinstance(error, ReleaseError) else "Release filesystem or encoding validation failed."
        print(message, file=sys.stderr)
        return 1
    print("Mac release finalization passed. No app was launched, installed, or published.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
