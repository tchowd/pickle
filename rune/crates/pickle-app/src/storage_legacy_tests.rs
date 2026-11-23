//! Original KnowledgeBase capacity/write-failure and two-copy mutation predicates.
//! Only owned temporary archives are created; production capacities are unchanged.
use super::*;

fn entry(number: usize) -> Entry {
    Entry {
        id: format!("legacy-storage-{number}"),
        recorded_at: "2026-11-23T12:00:00Z".into(),
        app_name: "Fixture".into(),
        app_id: "fixture".into(),
        action: "Simplify".into(),
        question: None,
        passage: "Cells release energy.".into(),
        answer: format!("Answer {number}."),
        quality: None,
        chart: None,
        reference: None,
    }
}
fn read(repo: &Repository, bookmark: bool) -> Vec<Entry> {
    if bookmark {
        repo.bookmarks().unwrap()
    } else {
        repo.history().unwrap()
    }
}
fn names(bookmark: bool) -> (&'static str, &'static str) {
    if bookmark {
        ("bookmarks.json", "history.json")
    } else {
        ("history.json", "bookmarks.json")
    }
}
fn seed_other_store_and_settings(repo: &Repository, bookmark: bool) -> (Vec<u8>, Vec<u8>) {
    repo.append(!bookmark, entry(90_000)).unwrap();
    repo.save_settings(&Settings {
        history_enabled: true,
        ..Settings::default()
    })
    .unwrap();
    other_bytes(repo, bookmark)
}
fn other_bytes(repo: &Repository, bookmark: bool) -> (Vec<u8>, Vec<u8>) {
    (
        fs::read(repo.directory().join(names(bookmark).1)).unwrap(),
        fs::read(repo.directory().join("settings.json")).unwrap(),
    )
}

#[test]
fn ordinary_append_at_actual_capacity_keeps_accepted_order_and_rejects_without_mutation() {
    for (bookmark, capacity) in [(false, 1000), (true, 200)] {
        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::new(directory.path().into()).unwrap();
        let untouched = seed_other_store_and_settings(&repo, bookmark);
        let seed: Vec<_> = (0..capacity - 2).map(entry).collect();
        // One valid seed archive avoids hundreds of fsyncs. The two admitted
        // appends and boundary refusal use the ordinary production mutation API.
        repo.write(names(bookmark).0, &seed).unwrap();
        assert_eq!(read(&repo, bookmark), seed);
        let first = entry(capacity);
        let second = entry(capacity + 1);
        repo.append(bookmark, first.clone()).unwrap();
        repo.append(bookmark, second.clone()).unwrap();
        let mut expected = vec![second, first];
        expected.extend(seed);
        assert_eq!(read(&repo, bookmark), expected);
        let path = directory.path().join(names(bookmark).0);
        let accepted_bytes = fs::read(&path).unwrap();

        let failure = repo.append(bookmark, entry(capacity + 2)).unwrap_err();
        assert!(failure.contains("full"));
        assert!(failure.contains(&capacity.to_string()));
        assert_eq!(fs::read(&path).unwrap(), accepted_bytes);
        assert_eq!(read(&repo, bookmark), expected);
        assert_eq!(other_bytes(&repo, bookmark), untouched);
        drop(repo);
        let reopened = Repository::new(directory.path().into()).unwrap();
        assert_eq!(read(&reopened, bookmark), expected);
        assert_eq!(other_bytes(&reopened, bookmark), untouched);
    }
}

#[test]
fn two_repository_copies_append_then_remove_only_their_own_entry_in_both_stores() {
    for bookmark in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let first = Repository::new(directory.path().into()).unwrap();
        let second = Repository::new(directory.path().into()).unwrap();
        let untouched = seed_other_store_and_settings(&first, bookmark);
        let original = entry(1);
        let mut other = entry(2);
        other.action = "Explain more".into();
        first.append(bookmark, original.clone()).unwrap();
        let first_view = read(&first, bookmark);
        assert_eq!(first_view.as_slice(), std::slice::from_ref(&original));
        second.append(bookmark, other.clone()).unwrap();
        assert_eq!(read(&second, bookmark), [other.clone(), original]);
        // Remove from the first copy's older view after the second copy wrote.
        first.remove(bookmark, &first_view[0].id).unwrap();
        assert_eq!(
            read(&first, bookmark).as_slice(),
            std::slice::from_ref(&other)
        );
        assert_eq!(
            read(&second, bookmark).as_slice(),
            std::slice::from_ref(&other)
        );
        assert_eq!(other_bytes(&first, bookmark), untouched);
        drop((first, second));
        let reopened = Repository::new(directory.path().into()).unwrap();
        assert_eq!(read(&reopened, bookmark), [other]);
        assert_eq!(other_bytes(&reopened, bookmark), untouched);
    }
}

// Windows directory write denial requires its own executed DACL fixture. This
// test asserts actual Unix permission denial; elevated/root bypass is a failure,
// never a skip or evidence that the storage write-failure predicate was covered.
#[cfg(unix)]
#[test]
fn denied_directory_write_preserves_readable_entries_other_store_and_settings() {
    use std::os::unix::fs::PermissionsExt;

    struct RestoreDirectory {
        path: PathBuf,
        permissions: fs::Permissions,
    }
    impl Drop for RestoreDirectory {
        fn drop(&mut self) {
            fs::set_permissions(&self.path, self.permissions.clone())
                .expect("Restore exact permissions on the owned fixture directory");
        }
    }
    for bookmark in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::new(directory.path().into()).unwrap();
        let untouched = seed_other_store_and_settings(&repo, bookmark);
        let original = entry(1);
        let removed = entry(2);
        repo.append(bookmark, original.clone()).unwrap();
        repo.append(bookmark, removed.clone()).unwrap();
        repo.remove(bookmark, &removed.id).unwrap();
        assert_eq!(
            read(&repo, bookmark).as_slice(),
            std::slice::from_ref(&original)
        );
        let path = directory.path().join(names(bookmark).0);
        let original_bytes = fs::read(&path).unwrap();
        let original_permissions = fs::metadata(directory.path()).unwrap().permissions();
        let original_mode = original_permissions.mode();
        let restore = RestoreDirectory {
            path: directory.path().into(),
            permissions: original_permissions,
        };
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!(
            fs::metadata(directory.path()).unwrap().permissions().mode() & 0o7777,
            0o500
        );
        let probe = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.path().join("denied-create-probe"))
            .expect_err("The owned directory must really deny file creation; do not run as root");
        assert_eq!(probe.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            repo.append(bookmark, entry(3)).unwrap_err(),
            "Could not stage local data."
        );
        assert_eq!(fs::read(&path).unwrap(), original_bytes);
        assert_eq!(
            read(&repo, bookmark).as_slice(),
            std::slice::from_ref(&original)
        );
        assert_eq!(other_bytes(&repo, bookmark), untouched);

        // Restore before reopening (Repository::new protects directory mode)
        // and before TempDir cleanup; Drop also restores during test unwinding.
        drop(restore);
        assert_eq!(
            fs::metadata(directory.path()).unwrap().permissions().mode(),
            original_mode
        );
        drop(repo);
        let reopened = Repository::new(directory.path().into()).unwrap();
        assert_eq!(read(&reopened, bookmark), [original]);
        assert_eq!(other_bytes(&reopened, bookmark), untouched);
        assert_eq!(fs::read(path).unwrap(), original_bytes);
    }
}
