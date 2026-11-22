# Windows C/C++ runtime policy

The x64 Windows package builds both Pickle and the native browser host with a static
C/C++ runtime. Users should not need an administrator-installed Visual C++
Redistributable to start these executables. This is a binary dependency policy,
not evidence that all Windows workflows or a clean installation have passed.

Build from `rune` on Windows with the normal MSVC Rust toolchain and Windows SDK:

```powershell
./scripts/build-windows.ps1 -Configuration release -Installer
```

The script explicitly selects `x86_64-pc-windows-msvc` and requests
`-C target-feature=+crt-static`. Cargo separates target dependencies from host build
scripts and procedural macros even when the target is the host architecture. This
avoids applying an unsupported static-runtime mode to host procedural-macro DLLs.
Native code using `cc` normally follows the resulting `CARGO_CFG_TARGET_FEATURE`
setting and selects `/MT` instead of `/MD`; a dependency can override its own native
build, so compiler flags alone are insufficient proof.

Artifacts come from
`target/windows-package/x86_64-pc-windows-msvc/debug` or `release`. The explicit output
root prevents an inherited `CARGO_TARGET_DIR` from making staging accidentally copy
an older executable from another location. Normal developer Cargo outputs remain
separate. Neither the Rust toolchain nor SDK is installed by this script.

## Existing build flags

The script preserves active `CARGO_ENCODED_RUSTFLAGS`, appending the static-runtime
arguments with Cargo's unit separator. Otherwise it appends to an existing
`RUSTFLAGS` string. Both environment values are restored in a `finally` block,
including when compilation fails. The encoded variable keeps Cargo's usual higher
precedence. The script does not evaluate flag contents as PowerShell code.

When neither environment variable is supplied, the script adds a Cargo
`build.rustflags` array override, preserving existing array entries through Cargo's
configuration merge. Configuration-only string flags cannot merge with that array;
Cargo rejects the build rather than dropping them. Target-specific config flags can
take precedence over `build.rustflags`; the mandatory binary check rejects any
resulting dynamic runtime. For either custom configuration, put all intended flags
in `RUSTFLAGS` (or the encoded variable for arguments containing spaces) before
packaging. The script appends the static-runtime choice and retains the other flags.

For example:

```powershell
$env:RUSTFLAGS = '--cfg=my_release_feature -C debuginfo=1'
./scripts/build-windows.ps1 -Configuration release
```

## Mandatory payload verification

Before signing, creating a ZIP, or invoking NSIS, packaging executes portable parser
fixtures and checks the exact staged `PickleRune.exe` and `pickle-browser-host.exe`.
Any bundled DLLs are checked too. The verifier reads PE ordinary imports and delay
imports; searching binary strings would both miss imports and misidentify unrelated
text. Malformed, truncated, or ambiguous import structures and incorrectly targeted
images fail closed. This is not a complete validation of every PE structure.

The policy rejects direct or delayed imports of `VCRUNTIME`, `MSVCP`, `MSVCR`,
`CONCRT`, `VCCORLIB`, `VCOMP`, `ucrtbase`/`ucrtbased`, and `api-ms-win-crt` DLL
families, including debug runtime versions. Windows operating-system DLLs remain
dynamically linked. The report records actual imports without executing the binaries:

```powershell
python packaging/test_windows_runtime.py
python packaging/verify-windows-runtime.py --directory artifacts/windows-STAGE --architecture x64 --output artifacts/windows-runtime-report.json
```

The verifier also supports `--architecture arm64` for inspecting a separate future
ARM64 payload. That does not add ARM64 build or product support. Full MSIX packaging
must inspect its own staged payload so accepting an arbitrary directory cannot bypass
the same runtime gate.

The report establishes only the inspected loader dependencies. It does not discover
arbitrary runtime `LoadLibrary` calls or validate system/GPU drivers, OCR language
packs, optional user-selected transcription engines, or minimum-version compatibility.
Test the final signed package on a clean supported Windows 11 VM without a separately
installed VC Redistributable. Record installation, app launch, browser-host framing,
rendering, and uninstall. Rebuild and redistribute when the statically linked runtime
requires a security update; installing a newer shared runtime does not update code
already linked into Pickle.

## Primary references

- [Rust: static and dynamic C runtimes](https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes)
- [Cargo: rustflags precedence and explicit-target isolation](https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags)
- [cc: MSVC static-runtime selection](https://docs.rs/cc/latest/cc/struct.Build.html#method.static_crt)
- [Microsoft: PE import and delay-import format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)
