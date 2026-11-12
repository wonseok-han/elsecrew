use super::*;

#[test]
fn watcher_scan_rejects_a_different_root_between_slices() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    fs::write(first.path().join("file"), b"first").unwrap();
    let held = Dir::open_ambient_dir(first.path(), ambient_authority()).unwrap();
    let other = Dir::open_ambient_dir(second.path(), ambient_authority()).unwrap();
    let scan = WatchScan::new(&held).unwrap();
    scan.validate_held_paths(&held).unwrap();
    assert!(scan.validate_held_paths(&other).is_err());
}

#[test]
fn watcher_scan_descendant_rename_is_blocked_or_revalidated() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("child")).unwrap();
    fs::write(root.path().join("child/old"), b"old").unwrap();
    let held = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
    let mut scan = WatchScan::new(&held).unwrap();
    assert!(!scan.advance(1).unwrap());
    assert_eq!(scan.pending.last().unwrap().2, "child");
    match fs::rename(root.path().join("child"), root.path().join("moved")) {
        Ok(()) => {
            fs::create_dir(root.path().join("child")).unwrap();
            assert!(scan.validate_held_paths(&held).is_err());
        }
        Err(error) => {
            // cap-std may hold a Windows handle that denies deletion/rename.
            assert_eq!(error.raw_os_error(), Some(32));
            assert!(!root.path().join("moved").exists());
            assert_eq!(fs::read(root.path().join("child/old")).unwrap(), b"old");
            scan.validate_held_paths(&held).unwrap();
        }
    }
}

#[test]
fn watcher_scan_rejects_wrong_retained_descendant() {
    let root = tempfile::tempdir().unwrap();
    let unrelated = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("child")).unwrap();
    fs::write(root.path().join("child/old"), b"old").unwrap();
    let held = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
    let mut scan = WatchScan::new(&held).unwrap();
    assert!(!scan.advance(1).unwrap());
    let other = Dir::open_ambient_dir(unrelated.path(), ambient_authority()).unwrap();
    let entries = other.entries().unwrap();
    *scan.pending.last_mut().unwrap() = (other, entries, "child".into());
    assert!(scan.validate_held_paths(&held).is_err());
}

#[test]
fn windows_watch_initial_scan_retains_only_a_bounded_slice() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..300 {
        fs::write(root.path().join(format!("file-{index}")), b"x").unwrap();
    }
    let held = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
    let expected = snapshot_tree(&held).unwrap();
    let (initial, pending) = begin_watch_scan(&held).unwrap();
    assert!(initial.is_empty());
    let mut scan = pending.expect("Windows scan must yield");
    assert!(scan.snapshot.len() <= 128);
    loop {
        scan.validate_held_paths(&held).unwrap();
        if scan.advance(WATCH_SCAN_BUDGET).unwrap() {
            break;
        }
    }
    assert!(scan.snapshot == expected);
    assert_eq!(scan.snapshot.len(), 300);
}
