#!/usr/bin/env python3
"""Reject dynamic VC/UCRT imports in the exact Windows package payload.

Reads PE import and delay-import directories without executing binaries or relying
on dumpbin being installed. Windows system DLLs remain dynamically linked.
Format reference: https://learn.microsoft.com/en-us/windows/win32/debug/pe-format
"""
import argparse
import json
import mmap
from pathlib import Path
import re
import struct
import sys


class InvalidPE(ValueError):
    pass


def is_dynamic_crt(name):
    return bool(re.fullmatch(
        r"(?:(?:vcruntime|msvcp|msvcr|concrt|vccorlib|vcomp)[a-z0-9_]*|"
        r"ucrtbased?|api-ms-win-crt-[a-z0-9-]+)\.dll", name, re.IGNORECASE))


def inspect_pe(data, expected_machine=0x8664):
    """Return bounded loader imports, rejecting malformed import structures."""
    def read(offset, fmt):
        size = struct.calcsize(fmt)
        if offset < 0 or offset > len(data) - size:
            raise InvalidPE("Truncated PE structure")
        return struct.unpack_from(fmt, data, offset)

    if read(0, "<H")[0] != 0x5A4D:
        raise InvalidPE("Missing DOS executable header")
    pe = read(0x3C, "<I")[0]
    if pe < 0x40 or read(pe, "<I")[0] != 0x4550:
        raise InvalidPE("Missing PE signature")
    machine, sections = read(pe + 4, "<HH")
    if machine != expected_machine:
        raise InvalidPE(f"Expected machine 0x{expected_machine:04x}; found 0x{machine:04x}")
    optional_size = read(pe + 20, "<H")[0]
    optional = pe + 24
    if optional_size < 112 or read(optional, "<H")[0] != 0x20B:
        raise InvalidPE("Expected PE32+ optional header")
    if optional + optional_size > len(data) or not 1 <= sections <= 96:
        raise InvalidPE("Invalid PE header or section count")
    image_base = read(optional + 24, "<Q")[0]
    header_size = read(optional + 60, "<I")[0]
    directory_count = read(optional + 108, "<I")[0]
    if directory_count > (optional_size - 112) // 8:
        raise InvalidPE("Data directories extend beyond optional header")
    section_table = optional + optional_size
    if section_table + sections * 40 > min(header_size, len(data)):
        raise InvalidPE("Section table extends beyond image headers")
    spans = []
    for index in range(sections):
        virtual_size, rva, size, offset = read(section_table + index * 40 + 8, "<IIII")
        if size and (offset < header_size or offset + size > len(data)):
            raise InvalidPE("Section raw data extends beyond image")
        spans.append((rva, max(virtual_size, size), offset, size))

    def locate(rva, size):
        if rva <= 0 or size < 1:
            raise InvalidPE("Invalid import address")
        matches = []
        if rva < header_size and rva + size <= min(header_size, len(data)):
            matches.append((rva, min(header_size, len(data)) - rva))
        for start, virtual_size, offset, raw_size in spans:
            if start <= rva < start + virtual_size:
                delta = rva - start
                if delta + size > raw_size:
                    raise InvalidPE("Import points outside section file data")
                matches.append((offset + delta, raw_size - delta))
        if len(matches) != 1:
            raise InvalidPE("Import address is absent or ambiguous")
        return matches[0]

    def dll_name(rva):
        offset, available = locate(rva, 1)
        end = data.find(b"\0", offset, offset + min(available, 4096))
        if end < 0:
            raise InvalidPE("Unterminated import library name")
        try:
            name = data[offset:end].decode("ascii")
        except UnicodeDecodeError as error:
            raise InvalidPE("Non-ASCII import library name") from error
        if not re.fullmatch(r"[A-Za-z0-9_.+-]+\.dll", name, re.IGNORECASE):
            raise InvalidPE("Invalid import library name")
        return name.lower()

    def imports(index, stride):
        if directory_count <= index:
            return []
        rva, size = read(optional + 112 + index * 8, "<II")
        if rva == 0 and size == 0:
            return []
        if size < stride or size > 16 * 1024 * 1024:
            raise InvalidPE("Invalid import directory size")
        offset, _ = locate(rva, size)
        result = []
        for entry in range(min(size // stride, 4097)):
            fields = read(offset + entry * stride, "<" + "I" * (stride // 4))
            if not any(fields):
                return sorted(set(result))
            if entry == 4096:
                raise InvalidPE("Too many import libraries")
            if index == 1:
                name_rva = fields[3]
            else:
                attributes, name_rva = fields[:2]
                if attributes not in (0, 1):
                    raise InvalidPE("Unsupported delay-import attributes")
                if attributes == 0:
                    # Legacy delay descriptors encode a VA instead of an RVA.
                    if name_rva < image_base:
                        raise InvalidPE("Invalid delay-import virtual address")
                    name_rva -= image_base
            result.append(dll_name(name_rva))
        raise InvalidPE("Import directory has no terminating descriptor")

    ordinary = imports(1, 20)
    delayed = imports(13, 32)
    return {
        "machine": f"0x{machine:04x}",
        "imports": ordinary,
        "delay_imports": delayed,
        "dynamic_crt": sorted({name for name in ordinary + delayed if is_dynamic_crt(name)}),
    }


def verify_payload(directory, expected_machine=0x8664):
    directory = Path(directory)
    required = [directory / name for name in ("PickleRune.exe", "pickle-browser-host.exe")]
    for path in required:
        if not path.is_file():
            raise InvalidPE(f"Required packaged executable is missing: {path.name}")
    # Include app-local DLLs if later dependencies introduce them. An EXE with no
    # direct CRT imports is insufficient if its bundled native DLL requires one.
    binaries = required + sorted(path for path in directory.rglob("*")
                                 if path.is_file() and path.suffix.lower() == ".dll")
    report = {}
    for path in binaries:
        with path.open("rb") as stream:
            if path.stat().st_size == 0:
                raise InvalidPE(f"Empty PE image: {path.name}")
            with mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
                try:
                    result = inspect_pe(data, expected_machine)
                except InvalidPE as error:
                    raise InvalidPE(f"{path.name}: {error}") from error
        report[path.relative_to(directory).as_posix()] = result
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--architecture", choices=("x64", "arm64"), default="x64")
    args = parser.parse_args(argv)
    try:
        report = verify_payload(args.directory, 0x8664 if args.architecture == "x64" else 0xAA64)
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        failures = [f"{path}: {', '.join(result['dynamic_crt'])}"
                    for path, result in report.items() if result["dynamic_crt"]]
        if failures:
            raise InvalidPE("Packaged binaries import a dynamic C/C++ runtime: " + "; ".join(failures))
    except (InvalidPE, OSError) as error:
        print(f"Windows runtime verification failed: {error}", file=sys.stderr)
        return 1
    print(f"PASS: {len(report)} packaged PE images have no direct or delay-loaded VC/UCRT dependencies.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
