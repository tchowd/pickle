import importlib.util
import json
import hashlib
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("notices", pathlib.Path(__file__).with_name("generate-notices.py"))
NOTICES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NOTICES)


class NoticesTests(unittest.TestCase):
    def metadata(self, directory):
        packages = []
        for name in ["app", "normal", "build", "dev"]:
            package = directory / name
            package.mkdir()
            (package / "LICENSE").write_text("Fixture license\n", encoding="utf-8")
            packages.append({"id": name, "name": name, "version": "1.0.0", "manifest_path": str(package / "Cargo.toml"), "license": "MIT", "license_file": None, "source": None if name == "app" else "registry+https://example.com/index"})
        return {"packages": packages, "resolve": {"nodes": [{"id": "app", "deps": [{"pkg": "normal", "dep_kinds": [{"kind": None}]}, {"pkg": "build", "dep_kinds": [{"kind": "build"}]}, {"pkg": "dev", "dep_kinds": [{"kind": "dev"}]}]}]}}

    def test_reachable_graph_includes_build_but_not_dev_only(self):
        with tempfile.TemporaryDirectory() as temporary:
            metadata = self.metadata(pathlib.Path(temporary))
            self.assertEqual([p["name"] for p in NOTICES.selected_packages(metadata, ["app"])], ["app", "build", "normal"])

    def test_inventory_is_deterministic_deduplicated_and_has_no_local_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            metadata = self.metadata(pathlib.Path(temporary))
            first, notices = NOTICES.collect(metadata, ["app"], "fixture-target")
            second, again = NOTICES.collect(metadata, ["app"], "fixture-target")
            self.assertEqual(first, second)
            self.assertEqual(notices, again)
            self.assertEqual(len(first["license_texts"]), 1)
            self.assertNotIn(temporary, json.dumps(first))
            self.assertEqual(first["review_gaps"], [])

    def test_missing_license_text_is_an_explicit_review_gap(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            metadata = self.metadata(directory)
            (directory / "normal" / "LICENSE").unlink()
            inventory, _ = NOTICES.collect(metadata, ["app"], "fixture-target")
            self.assertEqual(inventory["review_gaps"], ["normal 1.0.0"])

    def test_unknown_root_packages_fail_instead_of_empty_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            metadata = self.metadata(pathlib.Path(temporary))
            with self.assertRaises(ValueError):
                NOTICES.selected_packages(metadata, ["missing-app"])

    def test_explicit_license_paths_cannot_publish_files_outside_dependency_sources(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            metadata = self.metadata(directory)
            private = directory / "private.txt"
            private.write_text("must never enter a release artifact", encoding="utf-8")
            package = next(p for p in metadata["packages"] if p["name"] == "normal")
            package["license_file"] = str(private)
            (directory / "normal" / "LICENSE").unlink()
            inventory, text = NOTICES.collect(metadata, ["app"], "fixture-target")
            self.assertNotIn("must never enter", text)
            self.assertEqual(inventory["review_gaps"], ["normal 1.0.0"])

    def test_all_reviewed_upstream_notices_match_their_checksums(self):
        overrides = json.loads((NOTICES.PACKAGING / "license-overrides.json").read_text(encoding="utf-8"))
        self.assertEqual(overrides["schema_version"], 1)
        for package in overrides["packages"].values():
            self.assertEqual(len(package["upstream_revision"]), 40)
            self.assertTrue(package["files"])
            for notice in package["files"]:
                resource = NOTICES.PACKAGING / notice["path"]
                resource.resolve().relative_to((NOTICES.PACKAGING / "licenses").resolve())
                self.assertTrue(notice["source"].startswith("https://raw.githubusercontent.com/"))
                self.assertEqual(hashlib.sha256(resource.read_bytes()).hexdigest(), notice["sha256"])

    def test_reviewed_notice_bytes_survive_windows_git_checkout(self):
        resource_path = "rune/packaging/licenses/AccessKit-accesskit-f3026222-LICENSE-APACHE.txt"
        original = (NOTICES.ROOT / resource_path).read_bytes()
        attributes = (NOTICES.ROOT / ".gitattributes").read_bytes()
        # Use independent temporary indexes, not commits or the user's Git config.
        # The control case reproduces the exact conversion that broke Windows CI.
        for preserve in (False, True):
            with self.subTest(preserve=preserve), tempfile.TemporaryDirectory() as temporary:
                repository = pathlib.Path(temporary) / "repository"
                repository.mkdir()
                global_attributes = pathlib.Path(temporary) / "global-attributes"
                global_attributes.write_bytes(b"")
                def git(*arguments):
                    return subprocess.run(["git", "-c", "core.autocrlf=true", "-c", "core.safecrlf=false",
                                           "-c", "core.attributesFile=" + global_attributes.as_posix(), *arguments], cwd=repository,
                                          check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                git("init", "--quiet", "--template=")
                resource = repository / resource_path
                resource.parent.mkdir(parents=True)
                resource.write_bytes(original)
                if preserve:
                    (repository / ".gitattributes").write_bytes(attributes)
                    git("add", "--", ".gitattributes")
                git("add", "--", resource_path)
                checkout = pathlib.Path(temporary) / "checkout"
                checkout.mkdir()
                # A trailing slash is part of checkout-index's --prefix contract.
                git("checkout-index", "--all", "--prefix=" + checkout.as_posix() + "/")
                actual = (checkout / resource_path).read_bytes()
                self.assertEqual(actual, original if preserve else original.replace(b"\n", b"\r\n"))
                if preserve:
                    self.assertEqual(hashlib.sha256(actual).hexdigest(), "62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a")
                else:
                    self.assertEqual(hashlib.sha256(actual).hexdigest(), "20fe7b00e904ed690e3b9fd6073784d3fc428141dbd10b81c01fd143d0797f58")

    def test_reviewed_notice_newline_changes_still_fail_strict_checksum_validation(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            packaging = directory / "packaging"
            (packaging / "licenses").mkdir(parents=True)
            reviewed = b"Reviewed upstream license\nCopyright fixture\n"
            (packaging / "licenses" / "fixture.txt").write_bytes(reviewed.replace(b"\n", b"\r\n"))
            item = {"declared_license": "MIT", "upstream_repository": "https://example.com/fixture", "upstream_revision": "1" * 40,
                    "files": [{"path": "licenses/fixture.txt", "sha256": hashlib.sha256(reviewed).hexdigest()}]}
            (packaging / "license-overrides.json").write_text(json.dumps({"schema_version": 1, "packages": {"fixture@1.0.0": item}}), encoding="utf-8")
            source = directory / "source"
            source.mkdir()
            package = {"name": "fixture", "version": "1.0.0", "repository": item["upstream_repository"],
                       "license": "MIT", "source": "registry+https://example.com/index", "manifest_path": str(source / "Cargo.toml")}
            with mock.patch.object(NOTICES, "PACKAGING", packaging):
                with self.assertRaisesRegex(ValueError, "reviewed upstream notice failed its checksum"):
                    NOTICES.upstream_override(package)

    def test_updated_cargo_vcs_revisions_require_a_new_notice_review(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            (directory / ".cargo_vcs_info.json").write_text(json.dumps({"git": {"sha1": "0" * 40}}), encoding="utf-8")
            package = {"name": "accesskit", "version": "0.25.2", "repository": "https://github.com/AccessKit/accesskit", "license": "MIT OR Apache-2.0", "source": "registry+https://example.com/index", "manifest_path": str(directory / "Cargo.toml")}
            with self.assertRaises(ValueError):
                NOTICES.upstream_override(package)

    def test_old_crates_without_vcs_metadata_still_require_matching_published_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            (directory / "lib.rs").write_text("not the reviewed fxhash source", encoding="utf-8")
            package = {"name": "fxhash", "version": "0.2.1", "repository": "https://github.com/cbreeden/fxhash", "license": "Apache-2.0/MIT", "source": "registry+https://example.com/index", "manifest_path": str(directory / "Cargo.toml")}
            with self.assertRaises(ValueError):
                NOTICES.upstream_override(package)


if __name__ == "__main__":
    unittest.main()
