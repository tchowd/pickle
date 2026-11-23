//! Windows-only, owned-TempDir counterpart of the Unix denied-directory test.
//! No paths, identities, ACLs or privileges outside this fixture are modified.
use super::*;
use std::{
    ffi::{OsString, c_void},
    io,
    os::windows::{ffi::OsStringExt, fs::OpenOptionsExt, io::AsRawHandle},
    ptr::{null_mut, read_unaligned},
};

const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
const DIRECTORY: u32 = 0x10;
const REPARSE_POINT: u32 = 0x400;
const DACL_INFORMATION: u32 = 4;
const PROTECTED_DACL: u32 = 0x8000_0000;
const UNPROTECTED_DACL: u32 = 0x2000_0000;
const DACL_CONTROL: u16 = 0x150c; // present/defaulted/auto-inherit/protected
const SE_DACL_PROTECTED: u16 = 0x1000;

#[repr(C)]
struct Acl {
    revision: u8,
    reserved: u8,
    size: u16,
    count: u16,
    reserved2: u16,
}
#[repr(C)]
struct AceHeader {
    kind: u8,
    flags: u8,
    size: u16,
}
#[repr(C)]
struct Trustee {
    multiple: *mut Trustee,
    operation: u32,
    form: u32,
    kind: u32,
    sid: *mut c_void,
}
#[repr(C)]
struct ExplicitAccess {
    permissions: u32,
    mode: u32,
    inheritance: u32,
    trustee: Trustee,
}
#[repr(C)]
#[derive(Default)]
struct FileInformation {
    attributes: u32,
    times: [u32; 6], // three FILETIMEs, each two DWORDs
    volume: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    index_high: u32,
    index_low: u32,
}
const _: () = {
    assert!(size_of::<Acl>() == 8);
    assert!(size_of::<AceHeader>() == 4);
    assert!(size_of::<FileInformation>() == 52);
    assert!(size_of::<Trustee>() == if size_of::<usize>() == 8 { 32 } else { 20 });
    assert!(size_of::<ExplicitAccess>() == if size_of::<usize>() == 8 { 48 } else { 32 });
};

#[link(name = "advapi32")]
unsafe extern "system" {
    fn GetSecurityInfo(
        handle: *mut c_void,
        kind: u32,
        info: u32,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut Acl,
        sacl: *mut *mut Acl,
        descriptor: *mut *mut c_void,
    ) -> u32;
    fn GetSecurityDescriptorControl(
        descriptor: *mut c_void,
        control: *mut u16,
        revision: *mut u32,
    ) -> i32;
    fn GetSecurityDescriptorDacl(
        descriptor: *mut c_void,
        present: *mut i32,
        dacl: *mut *mut Acl,
        defaulted: *mut i32,
    ) -> i32;
    fn GetSecurityDescriptorLength(descriptor: *mut c_void) -> u32;
    fn IsValidSecurityDescriptor(descriptor: *mut c_void) -> i32;
    fn IsValidAcl(acl: *mut Acl) -> i32;
    fn GetAce(acl: *mut Acl, index: u32, ace: *mut *mut c_void) -> i32;
    fn CreateWellKnownSid(kind: u32, domain: *mut c_void, sid: *mut c_void, size: *mut u32) -> i32;
    fn SetEntriesInAclW(
        count: u32,
        entries: *const ExplicitAccess,
        old: *mut Acl,
        new: *mut *mut Acl,
    ) -> u32;
    fn SetSecurityInfo(
        handle: *mut c_void,
        kind: u32,
        info: u32,
        owner: *mut c_void,
        group: *mut c_void,
        dacl: *mut Acl,
        sacl: *mut Acl,
    ) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
    fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInformation) -> i32;
    fn GetFinalPathNameByHandleW(handle: *mut c_void, path: *mut u16, size: u32, flags: u32)
    -> u32;
}

struct LocalMemory(*mut c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: only pointers allocated by GetSecurityInfo/SetEntriesInAcl.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}
fn status(code: u32) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}
fn boolean(ok: i32) -> io::Result<()> {
    if ok != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::other(message)
}
fn trace(stage: &'static str, win32: Option<i32>, scalar: Option<u64>) {
    use std::io::Write as _;
    // Direct stderr writes bypass libtest's per-test print capture, including
    // when a native fast-fail or an unwind prevents the harness printing it.
    // Stages are literals; only numeric status/flags and booleans are emitted.
    let line = format!(
        "{{\"test\":\"windows-owned-storage-denial\",\"stage\":\"{stage}\",\"win32\":{},\"scalar\":{},\"unwinding\":{}}}\n",
        win32.map_or_else(|| "null".into(), |value| value.to_string()),
        scalar.map_or_else(|| "null".into(), |value| value.to_string()),
        std::thread::panicking(),
    );
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(line.as_bytes());
    let _ = stderr.flush();
}
fn open_mutation(directory: &tempfile::TempDir) -> io::Result<File> {
    // Exact MAXIMUM_ALLOWED is intentional: Microsoft documents that a
    // SetSecurityInfo handle opened this way does NOT propagate ACEs to children.
    // https://learn.microsoft.com/windows/win32/api/aclapi/nf-aclapi-setsecurityinfo
    OpenOptions::new()
        .access_mode(MAXIMUM_ALLOWED)
        .share_mode(3) // READ | WRITE, never DELETE: prevent directory substitution
        .custom_flags(0x0200_0000 | 0x0020_0000) // BACKUP_SEMANTICS | OPEN_REPARSE_POINT
        .open(directory.path())
}
fn open_observer(path: &Path) -> io::Result<File> {
    // READ_CONTROL | FILE_READ_ATTRIBUTES requests neither DELETE nor write
    // access. Share DELETE so the one MAXIMUM_ALLOWED mutation handle can open;
    // that mutation handle itself continues to refuse rename/delete sharing.
    OpenOptions::new()
        .access_mode(0x0002_0080)
        .share_mode(7)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)
}
fn identity(file: &File) -> io::Result<(u32, u64, u32)> {
    let mut info = FileInformation::default();
    // SAFETY: valid borrowed File handle and full writable native structure.
    boolean(unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) })?;
    Ok((
        info.volume,
        (u64::from(info.index_high) << 32) | u64::from(info.index_low),
        info.attributes & (DIRECTORY | REPARSE_POINT),
    ))
}
fn final_path(file: &File) -> io::Result<PathBuf> {
    let mut path = vec![0u16; 32_768];
    // SAFETY: retained File handle; output count is in UTF-16 code units.
    let count = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            path.as_mut_ptr(),
            path.len() as u32,
            0,
        )
    };
    if count == 0 {
        return Err(io::Error::last_os_error());
    }
    if count as usize >= path.len() {
        return Err(invalid("Owned directory final path exceeded its bound"));
    }
    Ok(PathBuf::from(OsString::from_wide(&path[..count as usize])))
}

unsafe fn ace_bytes(acl: *mut Acl, index: u32) -> io::Result<Vec<u8>> {
    let mut ace = null_mut();
    // SAFETY: caller retains an aligned valid native ACL for this entire call.
    unsafe {
        boolean(GetAce(acl, index, &mut ace))?;
        if ace.is_null() {
            return Err(invalid("No owned fixture ACE was returned"));
        }
        let length = usize::from(read_unaligned(ace.cast::<AceHeader>()).size);
        let offset = (ace as usize)
            .checked_sub(acl as usize)
            .ok_or_else(|| invalid("ACE is outside ACL"))?;
        if length < size_of::<AceHeader>()
            || offset
                .checked_add(length)
                .is_none_or(|end| end > usize::from(read_unaligned(acl).size))
        {
            return Err(invalid("Owned fixture ACE exceeds ACL"));
        }
        Ok(std::slice::from_raw_parts(ace.cast::<u8>(), length).to_vec())
    }
}

fn only_added_directory_deny(
    original: &DaclSnapshot,
    proposed: *mut Acl,
    sid: &[u8],
) -> io::Result<()> {
    // SAFETY: both valid ACL allocations remain retained by the caller.
    unsafe {
        if IsValidAcl(proposed) == 0 {
            return Err(invalid("Invalid proposed directory ACL"));
        }
        let old = read_unaligned(original.pointer());
        let new = read_unaligned(proposed);
        if new.revision != old.revision || old.count.checked_add(1) != Some(new.count) {
            return Err(invalid("Directory ACL merge changed more than one ACE"));
        }
        let first = ace_bytes(proposed, 0)?;
        if first.len() != 8 + sid.len()
            || first[0] != 1
            || first[1] != 0
            || first[4..8] != 2u32.to_le_bytes()
            || &first[8..] != sid
        {
            return Err(invalid(
                "Directory denial must be only FILE_ADD_FILE without inheritance",
            ));
        }
        for index in 0..u32::from(old.count) {
            if ace_bytes(original.pointer(), index)? != ace_bytes(proposed, index + 1)? {
                return Err(invalid("Directory ACL merge changed an existing ACE"));
            }
        }
    }
    Ok(())
}

#[derive(PartialEq, Eq)]
struct DaclSnapshot {
    // u32 backing guarantees ACL alignment; length excludes final word padding.
    words: Vec<u32>,
    length: usize,
    control: u16,
}
impl DaclSnapshot {
    fn read(file: &File) -> io::Result<Self> {
        let mut descriptor = null_mut();
        // SAFETY: valid File handle; descriptor is LocalAlloc-owned on success.
        status(unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                1,
                DACL_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            )
        })?;
        let allocation = LocalMemory(descriptor);
        if descriptor.is_null() || unsafe { IsValidSecurityDescriptor(descriptor) } == 0 {
            return Err(invalid("Invalid owned fixture security descriptor"));
        }
        let (mut control, mut revision, mut present, mut defaulted) = (0, 0, 0, 0);
        let mut acl = null_mut();
        // SAFETY: Windows-validated descriptor remains alive in allocation.
        unsafe {
            boolean(GetSecurityDescriptorControl(
                descriptor,
                &mut control,
                &mut revision,
            ))?;
            boolean(GetSecurityDescriptorDacl(
                descriptor,
                &mut present,
                &mut acl,
                &mut defaulted,
            ))?;
        }
        // Do not substitute an empty ACL for a null/missing/defaulted ACL.
        // Such a fixture is unsupported and fails BEFORE any permission change.
        if present == 0 || acl.is_null() || defaulted != 0 || control & 0x0100 != 0 {
            return Err(invalid(
                "Owned fixture requires a present nondefaulted stable DACL",
            ));
        }
        if unsafe { IsValidAcl(acl) } == 0 {
            return Err(invalid("Invalid owned fixture ACL"));
        }
        // SAFETY: validated OS ACL contains its complete fixed-size header.
        let length = usize::from(unsafe { read_unaligned(acl) }.size);
        let start = descriptor as usize;
        let size = unsafe { GetSecurityDescriptorLength(descriptor) } as usize;
        let offset = (acl as usize)
            .checked_sub(start)
            .ok_or_else(|| invalid("ACL is outside descriptor"))?;
        if length < size_of::<Acl>() || offset.checked_add(length).is_none_or(|end| end > size) {
            return Err(invalid("Owned fixture ACL exceeds descriptor"));
        }
        let mut words = vec![0u32; length.div_ceil(4)];
        // SAFETY: validated/bounded OS source, aligned destination with enough bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(
                acl.cast::<u8>(),
                words.as_mut_ptr().cast::<u8>(),
                length,
            );
        }
        drop(allocation);
        Ok(Self {
            words,
            length,
            control: control & DACL_CONTROL,
        })
    }
    fn pointer(&self) -> *mut Acl {
        self.words.as_ptr().cast_mut().cast()
    }
    fn information(&self) -> u32 {
        DACL_INFORMATION
            | if self.control & SE_DACL_PROTECTED != 0 {
                PROTECTED_DACL
            } else {
                UNPROTECTED_DACL
            }
    }
    fn apply(&self, file: &File, acl: *mut Acl) -> io::Result<()> {
        // SAFETY: the ACL is borrowed for this call. The retained MAXIMUM_ALLOWED
        // handle targets ONLY the owned directory and prevents child propagation.
        let code = unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                1,
                self.information(),
                null_mut(),
                null_mut(),
                acl,
                null_mut(),
            )
        };
        trace(
            "set-security-return",
            Some(code as i32),
            Some(u64::from(self.information())),
        );
        status(code)
    }
}

struct RestoreAcl<'a> {
    directory: &'a tempfile::TempDir,
    file: File,
    original: DaclSnapshot,
    identity: (u32, u64, u32),
    armed: bool,
}
impl<'a> RestoreAcl<'a> {
    fn deny_create(
        directory: &'a tempfile::TempDir,
        observer: &File,
        expected_dacl: &DaclSnapshot,
    ) -> io::Result<Self> {
        trace("mutation-open-enter", None, None);
        let file = open_mutation(directory)?;
        trace("mutation-open-complete", None, None);
        let id = identity(&file)?;
        if id.2 & DIRECTORY == 0
            || id.2 & REPARSE_POINT != 0
            || id != identity(observer)?
            || final_path(&file)? != fs::canonicalize(directory.path())?
        {
            return Err(invalid(
                "ACL target is not the exact owned non-reparse directory",
            ));
        }
        trace("precheck-identity-complete", None, None);
        let original = DaclSnapshot::read(&file)?;
        if original != *expected_dacl || DaclSnapshot::read(observer)? != original {
            return Err(invalid(
                "Owned ACL observer and mutation snapshots differ before denial",
            ));
        }
        trace(
            "precheck-snapshot-complete",
            None,
            Some(u64::from(original.control)),
        );
        let mut guard = Self {
            directory,
            file,
            original,
            identity: id,
            armed: false,
        };
        let mut sid = [0u32; 17]; // SECURITY_MAX_SID_SIZE, aligned for SID writes
        let mut size = size_of_val(&sid) as u32;
        // WinWorldSid = Everyone; no account lookup, user SID or privilege change.
        trace("sid-create-enter", None, None);
        boolean(unsafe { CreateWellKnownSid(1, null_mut(), sid.as_mut_ptr().cast(), &mut size) })?;
        trace("sid-create-complete", None, Some(u64::from(size)));
        let entry = ExplicitAccess {
            permissions: 2, // FILE_ADD_FILE; never GENERIC_WRITE or child inheritance
            mode: 3,        // DENY_ACCESS
            inheritance: 0, // NO_INHERITANCE, applies only to this directory
            trustee: Trustee {
                multiple: null_mut(),
                operation: 0,
                form: 0,
                kind: 5,
                sid: sid.as_mut_ptr().cast(),
            },
        };
        let mut denied = null_mut();
        // SAFETY: SID/entry/original aligned ACL outlive the native merge call.
        trace("acl-merge-enter", None, None);
        status(unsafe { SetEntriesInAclW(1, &entry, guard.original.pointer(), &mut denied) })?;
        trace("acl-merge-complete", None, None);
        let allocation = LocalMemory(denied.cast());
        if denied.is_null() {
            return Err(invalid("No owned denial ACL was returned"));
        }
        if size as usize > size_of_val(&sid) {
            return Err(invalid("Invalid well-known SID length"));
        }
        // SAFETY: initialized prefix from CreateWellKnownSid in aligned storage.
        let sid_bytes =
            unsafe { std::slice::from_raw_parts(sid.as_ptr().cast::<u8>(), size as usize) };
        only_added_directory_deny(&guard.original, denied, sid_bytes)?;
        trace("acl-merge-verified", None, None);
        // Arm BEFORE the first mutation, so any failure/unwind attempts restore.
        guard.armed = true;
        trace("deny-apply-enter", None, None);
        guard.original.apply(&guard.file, denied)?;
        trace("deny-apply-complete", None, None);
        drop(allocation);
        guard.assert_identity()?;
        trace("deny-identity-complete", None, None);
        Ok(guard)
    }
    fn assert_identity(&self) -> io::Result<()> {
        let fresh = open_observer(self.directory.path())?;
        if identity(&self.file)? != self.identity
            || identity(&fresh)? != self.identity
            || final_path(&self.file)? != fs::canonicalize(self.directory.path())?
        {
            return Err(invalid("Owned ACL fixture directory identity changed"));
        }
        Ok(())
    }
    fn restore(&mut self) -> io::Result<()> {
        if self.armed {
            // Restore through the pre-denial handle, not a path reopened under
            // a modified ACL. Borrowing TempDir also enforces cleanup order.
            trace("restore-apply-enter", None, None);
            self.original.apply(&self.file, self.original.pointer())?;
            trace("restore-apply-complete", None, None);
            let restored = DaclSnapshot::read(&self.file)?;
            trace(
                "restore-control-pair",
                None,
                Some((u64::from(self.original.control) << 16) | u64::from(restored.control)),
            );
            trace(
                "restore-acl-bytes-equal",
                None,
                Some(u64::from(
                    restored.words == self.original.words
                        && restored.length == self.original.length,
                )),
            );
            if restored != self.original {
                return Err(invalid(
                    "Original owned directory DACL/control was not restored exactly",
                ));
            }
            self.assert_identity()?;
            self.armed = false;
            trace("restore-verified", None, None);
        }
        Ok(())
    }
}
impl Drop for RestoreAcl<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            trace("drop-restore-failed", error.raw_os_error(), None);
            // Preserve the primary denial/store/explicit-restore assertion on
            // unwind. A second panic here would abort libtest and hide both.
            // Normal failed cleanup remains a failing test, never a silent pass.
            if !std::thread::panicking() {
                panic!("Could not restore the owned fixture ACL; see fixed diagnostic stages");
            }
        }
    }
}
fn child_acls(directory: &Path) -> Vec<DaclSnapshot> {
    [
        "history.json",
        "bookmarks.json",
        "settings.json",
        "store.lock",
        "child",
        "child/proof.txt",
    ]
    .iter()
    .map(|name| DaclSnapshot::read(&open_observer(&directory.join(name)).unwrap()).unwrap())
    .collect()
}

#[test]
fn windows_denied_directory_create_preserves_archives_and_restores_exact_owned_acl() {
    for bookmark in [false, true] {
        trace("case-start", None, Some(u64::from(bookmark)));
        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::new(directory.path().into()).unwrap();
        let untouched = seed_other_store_and_settings(&repo, bookmark);
        let original = entry(1);
        let removed = entry(2);
        repo.append(bookmark, original.clone()).unwrap();
        repo.append(bookmark, removed.clone()).unwrap();
        repo.remove(bookmark, &removed.id).unwrap();
        let path = directory.path().join(names(bookmark).0);
        let bytes = fs::read(&path).unwrap();
        fs::create_dir(directory.path().join("child")).unwrap();
        fs::write(directory.path().join("child/proof.txt"), b"owned child").unwrap();
        let children_before = child_acls(directory.path());
        let before_handle = open_observer(directory.path()).unwrap();
        let before = DaclSnapshot::read(&before_handle).unwrap();
        let before_identity = identity(&before_handle).unwrap();
        trace("setup-complete", None, None);

        let mut restore = RestoreAcl::deny_create(&directory, &before_handle, &before).unwrap();
        assert!(DaclSnapshot::read(&before_handle).unwrap() != before);
        assert!(
            child_acls(directory.path()) == children_before,
            "Directory denial must not change child ACLs"
        );
        trace("denial-child-acls-verified", None, None);
        trace("probe-enter", None, None);
        let probe = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.path().join("denied-create-probe"))
            .expect_err(
                "FILE_ADD_FILE must really be denied, including on an administrator runner",
            );
        trace("probe-return", probe.raw_os_error(), None);
        assert_eq!(probe.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(probe.raw_os_error(), Some(5));
        trace("store-append-enter", None, None);
        let failure = repo.append(bookmark, entry(3)).unwrap_err();
        trace(
            "store-append-refused",
            None,
            Some(u64::from(failure == "Could not stage local data.")),
        );
        assert_eq!(failure, "Could not stage local data.");
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            read(&repo, bookmark).as_slice(),
            std::slice::from_ref(&original)
        );
        assert_eq!(other_bytes(&repo, bookmark), untouched);
        trace("denial-data-verified", None, None);

        trace("explicit-restore-enter", None, None);
        restore.restore().unwrap();
        drop(restore);
        assert!(DaclSnapshot::read(&before_handle).unwrap() == before);
        assert_eq!(identity(&before_handle).unwrap(), before_identity);
        assert_eq!(
            identity(&open_observer(directory.path()).unwrap()).unwrap(),
            before_identity
        );
        assert!(
            child_acls(directory.path()) == children_before,
            "Restoration must not change child ACLs"
        );
        trace("post-restore-security-verified", None, None);
        // Exact restoration is proven before reopening or TempDir cleanup.
        drop((before_handle, repo));
        let reopened = Repository::new(directory.path().into()).unwrap();
        assert_eq!(read(&reopened, bookmark), [original]);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(other_bytes(&reopened, bookmark), untouched);
        trace("case-complete", None, Some(u64::from(bookmark)));
    }
}
