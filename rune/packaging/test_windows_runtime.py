#!/usr/bin/env python3
"""Portable PE fixtures for the packaged Windows runtime policy."""
import contextlib
import importlib.util
import io
from pathlib import Path
import struct
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("windows_runtime", Path(__file__).with_name("verify-windows-runtime.py"))
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)


def pe_fixture(imports=("KERNEL32.dll",), delayed=(), machine=0x8664):
    image = bytearray(0x800)
    def put(offset, fmt, *values):
        struct.pack_into("<" + fmt, image, offset, *values)
    put(0, "H", 0x5A4D)
    put(0x3C, "I", 0x80)
    put(0x80, "IHH", 0x4550, machine, 1)
    put(0x94, "H", 240)
    put(0x98, "H", 0x20B)
    put(0x98 + 24, "Q", 0x140000000)
    put(0x98 + 60, "I", 0x200)
    put(0x98 + 108, "I", 16)
    put(0x188 + 8, "IIII", 0x600, 0x1000, 0x600, 0x200)
    for index, values, rva, name_rva, stride in (
        (1, imports, 0x1000, 0x1300, 20),
        (13, delayed, 0x1100, 0x1500, 32),
    ):
        if not values:
            continue
        put(0x98 + 112 + index * 8, "II", rva, stride * (len(values) + 1))
        for count, name in enumerate(values):
            offset = rva - 0x1000 + 0x200 + count * stride
            if index == 1:
                put(offset, "IIIII", 0x1600, 0, 0, name_rva, 0x1600)
            else:
                put(offset, "IIIIIIII", 1, name_rva, 0, 0x1600, 0x1600, 0, 0, 0)
            name_offset = name_rva - 0x1000 + 0x200
            encoded = name.encode("ascii") + b"\0"
            image[name_offset:name_offset + len(encoded)] = encoded
            name_rva += len(encoded)
    return image


class RuntimePolicyTests(unittest.TestCase):
    def test_reads_real_import_directories_instead_of_arbitrary_strings(self):
        image = pe_fixture(("KERNEL32.dll", "dwrite.dll", "api-ms-win-core-synch-l1-2-0.dll"))
        image[0x770:0x770 + 17] = b"VCRUNTIME140.dll\0\0"
        report = runtime.inspect_pe(image)
        self.assertEqual(report["dynamic_crt"], [])
        self.assertIn("dwrite.dll", report["imports"])

    def test_rejects_vc_and_ucrt_import_families_including_debug_versions(self):
        for name in ("VCRUNTIME140.dll", "vcruntime140_1.dll", "vcruntime140d.dll",
                     "MSVCP140.dll", "MSVCP140_ATOMIC_WAIT.dll", "MSVCR120.dll",
                     "msvcrt.dll", "CONCRT140.dll", "VCCORLIB140.dll", "VCOMP140.dll",
                     "ucrtbase.dll", "ucrtbased.dll", "api-ms-win-crt-runtime-l1-1-0.dll"):
            with self.subTest(name=name):
                self.assertEqual(runtime.inspect_pe(pe_fixture((name,)))["dynamic_crt"], [name.lower()])

    def test_checks_delay_imports_as_well_as_eager_imports(self):
        report = runtime.inspect_pe(pe_fixture(delayed=("USER32.dll", "VCRUNTIME140.dll")))
        self.assertEqual(report["dynamic_crt"], ["vcruntime140.dll"])
        self.assertEqual(report["delay_imports"], ["user32.dll", "vcruntime140.dll"])

    def test_supports_legacy_delay_import_virtual_addresses(self):
        image = pe_fixture(delayed=("VCRUNTIME140.dll",))
        struct.pack_into("<Q", image, 0x98 + 24, 0x400000)
        struct.pack_into("<II", image, 0x300, 0, 0x401500)
        self.assertEqual(runtime.inspect_pe(image)["dynamic_crt"], ["vcruntime140.dll"])

    def test_absent_import_directories_are_allowed(self):
        self.assertEqual(runtime.inspect_pe(pe_fixture(imports=()))["imports"], [])

    def test_wrong_architecture_and_truncated_images_fail_closed(self):
        with self.assertRaises(runtime.InvalidPE):
            runtime.inspect_pe(pe_fixture(machine=0xAA64))
        self.assertEqual(runtime.inspect_pe(pe_fixture(machine=0xAA64), 0xAA64)["machine"], "0xaa64")
        for length in (0, 64, 0x80, 0x200, 0x600):
            with self.subTest(length=length), self.assertRaises(runtime.InvalidPE):
                runtime.inspect_pe(pe_fixture()[:length])

    def test_invalid_directory_or_name_cannot_bypass_runtime_policy(self):
        variants = []
        malformed = pe_fixture()
        struct.pack_into("<I", malformed, 0x98 + 112 + 8 + 4, 20)  # No null descriptor.
        variants.append(malformed)
        malformed = pe_fixture()
        struct.pack_into("<I", malformed, 0x200 + 12, 0x80000000)  # Unmapped name RVA.
        variants.append(malformed)
        malformed = pe_fixture()
        malformed[0x500:0x800] = b"A" * 0x300  # No terminating name.
        variants.append(malformed)
        malformed = pe_fixture()
        struct.pack_into("<I", malformed, 0x98 + 108, 100)  # Directories beyond header.
        variants.append(malformed)
        for image in variants:
            with self.assertRaises(runtime.InvalidPE):
                runtime.inspect_pe(image)

    def test_requires_both_packaged_executables_and_scans_bundled_dlls(self):
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            (stage / "PickleRune.exe").write_bytes(pe_fixture())
            with self.assertRaises(runtime.InvalidPE):
                runtime.verify_payload(stage)
            (stage / "pickle-browser-host.exe").write_bytes(pe_fixture())
            (stage / "native.dll").write_bytes(pe_fixture(delayed=("MSVCP140.dll",)))
            report = runtime.verify_payload(stage)
            self.assertEqual(len(report), 3)
            self.assertEqual(report["native.dll"]["dynamic_crt"], ["msvcp140.dll"])
            output = stage / "runtime-report.json"
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(runtime.main(["--directory", directory, "--output", str(output)]), 1)
            self.assertTrue(output.is_file())
            (stage / "native.dll").unlink()
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(runtime.main(["--directory", directory]), 0)


if __name__ == "__main__":
    unittest.main()
