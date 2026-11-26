//! Result preview reads against a real chat workspace (plan 2.4 T4·T5·T15,
//! 2.6 "Rust 단위"): the address comes from the chat binding, paths never
//! leave the workspace, hidden and unknown files never open, and every read
//! goes through the directory handle that was verified (PR #6 review B1).

use super::*;
use crate::project::set_preview_test_hook;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use elsecrew_protocol::preview_v1::PREVIEW_ASSET_CHUNK_BYTES_V1;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind { GitWorktree, Snapshot }

struct Opened {
    chat_id: String,
    address: WorkspaceAddress,
    root: PathBuf,
}

fn workspace(fixture: &mut Fixture, name: &str) -> Opened {
    workspace_of(fixture, name, Kind::Snapshot)
}

/// A task workspace of either kind: a real Git worktree (macOS, the
/// application-baseline path) or a standalone snapshot copy.
fn workspace_of(fixture: &mut Fixture, name: &str, kind: Kind) -> Opened {
    fixture.projects.set_git_worktrees_for_test(kind == Kind::GitWorktree);
    let request = format!("req_preview_snapshot_{name}");
    let snapshot = if kind == Kind::GitWorktree {
        let project_id = fixture.project_id.clone();
        let prepared = fixture.chats
            .prepare_chat_workspace_with_baseline(&mut fixture.storage, &mut fixture.projects, &project_id, &request, true)
            .unwrap()
            .result;
        let mut snapshot = prepared["manifest"].clone();
        snapshot["preparationId"] = prepared["preparationId"].clone();
        snapshot
    } else {
        fixture.snapshot(&request)
    };
    let chat = fixture.create_chat(&snapshot, "preview goal", &format!("req_preview_chat_{name}"));
    let address = address_from_chat(&chat).unwrap();
    let root = PathBuf::from(snapshot["worktreePath"].as_str().unwrap());
    assert_eq!(snapshot["workspaceKind"] == "standaloneSnapshot", kind == Kind::Snapshot, "fixture kind {:?}", snapshot.get("workspaceKind"));
    Opened { chat_id: chat["chatId"].as_str().unwrap().to_string(), address, root }
}

fn resolve(fixture: &mut Fixture, opened: &Opened, entry: Option<&str>) -> Value {
    let project_id = fixture.project_id.clone();
    fixture.projects.preview_resolve(&fixture.storage, &project_id, &opened.chat_id, entry).unwrap().result
}

/// Resolve and keep the handle the sidecar recorded.
fn handle(fixture: &mut Fixture, opened: &Opened) -> String {
    resolve(fixture, opened, None)["previewHandle"].as_str().unwrap().to_string()
}

fn read_at(fixture: &mut Fixture, handle: &str, address: &WorkspaceAddress, path: &str, offset: u64) -> Result<Value, crate::project::ProjectFailure> {
    fixture.projects.preview_read_asset(&fixture.storage, handle, address, path, offset).map(|action| action.result)
}

fn read(fixture: &mut Fixture, handle: &str, address: &WorkspaceAddress, path: &str) -> Value {
    read_at(fixture, handle, address, path, 0).unwrap()
}

fn outcome(fixture: &mut Fixture, handle: &str, address: &WorkspaceAddress, path: &str) -> String {
    read(fixture, handle, address, path)["outcome"].as_str().unwrap().to_string()
}

fn bytes_of(value: &Value) -> Vec<u8> {
    BASE64.decode(value["bytesBase64"].as_str().unwrap()).unwrap()
}

#[test]
fn resolve_takes_the_address_from_the_chat_and_finds_the_entry_in_order() {
    let mut fixture = Fixture::new();
    let opened = workspace(&mut fixture, "order");
    let root = opened.root.clone();
    // No page yet: nothing to show.
    let empty = resolve(&mut fixture, &opened, None);
    assert_eq!(empty["entry"], Value::Null);
    assert_eq!(empty["worktreeId"], json!(opened.address.worktree_id));
    assert_eq!(empty["workspaceIdentity"], json!(opened.address.workspace_identity));
    assert_eq!(empty["chatId"], json!(opened.chat_id));
    assert!(empty["previewHandle"].as_str().unwrap().starts_with("ph_"));

    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("dist")).unwrap();
    fs::write(root.join("docs/index.html"), "docs").unwrap();
    assert_eq!(resolve(&mut fixture, &opened, None)["entry"], json!("docs/index.html"));
    fs::write(root.join("dist/index.html"), "dist").unwrap();
    assert_eq!(resolve(&mut fixture, &opened, None)["entry"], json!("dist/index.html"));
    fs::write(root.join("index.html"), "root").unwrap();
    assert_eq!(resolve(&mut fixture, &opened, None)["entry"], json!("index.html"));
    // A named page wins; a non-page, hidden or missing one is refused.
    assert_eq!(resolve(&mut fixture, &opened, Some("docs/index.html"))["entry"], json!("docs/index.html"));
    for (entry, refusal) in [("app.js", "notPage"), (".git/index.html", "hidden"), ("none.html", "notFound"), ("../x.html", "traversal")] {
        let value = resolve(&mut fixture, &opened, Some(entry));
        assert_eq!(value["entry"], Value::Null, "{entry}");
        assert_eq!(value["refusal"], json!(refusal), "{entry}");
    }
    // A root index.html that is a link is not an entry.
    fs::remove_file(root.join("index.html")).unwrap();
    symlink(root.join("dist/index.html"), root.join("index.html")).unwrap();
    assert_eq!(resolve(&mut fixture, &opened, None)["entry"], json!("dist/index.html"));

    // Another project's id, or an unknown chat, never resolves.
    assert!(fixture.projects.preview_resolve(&fixture.storage, "project_other", &opened.chat_id, None).is_err());
    let project_id = fixture.project_id.clone();
    assert!(fixture.projects.preview_resolve(&fixture.storage, &project_id, "chat_missing", None).is_err());
}

#[test]
fn read_asset_serves_allowed_files_and_refuses_everything_else() {
    let mut fixture = Fixture::new();
    let opened = workspace(&mut fixture, "paths");
    let (root, address) = (opened.root.clone(), opened.address.clone());
    fs::create_dir_all(root.join("css")).unwrap();
    fs::create_dir_all(root.join("a/.secret")).unwrap();
    fs::write(root.join("index.html"), "<p>ok</p>").unwrap();
    fs::write(root.join("css/app.css"), "p{}").unwrap();
    fs::write(root.join("logo.png"), [0_u8, 159, 146, 150, 0, 1]).unwrap();
    fs::write(root.join(".env"), "fixture-only").unwrap();
    fs::write(root.join("a/.secret/x.js"), "fixture-only").unwrap();
    fs::write(root.join("key.pem"), "fixture-only").unwrap();
    fs::write(root.join("config.env"), "fixture-only").unwrap();
    let h = handle(&mut fixture, &opened);

    let page = read(&mut fixture, &h, &address, "index.html");
    assert_eq!(page["outcome"], "ok");
    assert_eq!(page["byteLength"], 9);
    assert_eq!(bytes_of(&page), b"<p>ok</p>");
    assert_eq!(page["version"], json!(sha256_file_version_v1(b"<p>ok</p>")));
    assert_eq!(bytes_of(&read(&mut fixture, &h, &address, "logo.png")), [0_u8, 159, 146, 150, 0, 1]);
    assert_eq!(outcome(&mut fixture, &h, &address, "css/app.css"), "ok");

    // T5: hidden files, Git metadata, secret-looking types.
    for path in [".env", ".git/HEAD", ".git/config", "a/.secret/x.js", "key.pem", "config.env"] {
        assert_eq!(outcome(&mut fixture, &h, &address, path), "refused", "{path}");
    }
    // T4: traversal, absolute and encoded escapes never open anything.
    for path in ["../source/src/value.txt", "css/../../x.txt", "/etc/hosts.txt", "%2e%2e/x.txt", "a%2fb.txt", "a\\b.txt", "a\0.txt", "css//app.css"] {
        assert_eq!(outcome(&mut fixture, &h, &address, path), "refused", "{path:?}");
    }
    assert_eq!(outcome(&mut fixture, &h, &address, "missing.html"), "notFound");
    assert_eq!(outcome(&mut fixture, &h, &address, "css"), "refused"); // no extension
    fs::create_dir_all(root.join("folder.html")).unwrap();
    assert_eq!(outcome(&mut fixture, &h, &address, "folder.html"), "notFound");
    // An unknown or forged handle reads nothing.
    assert!(read_at(&mut fixture, "ph_00000000000000000000000000000000", &address, "index.html", 0).is_err());
}

#[test]
fn read_asset_never_follows_links_out_of_the_workspace() {
    let mut fixture = Fixture::new();
    let opened = workspace(&mut fixture, "links");
    let (root, address) = (opened.root.clone(), opened.address.clone());
    let outside = fixture.source.parent().unwrap().join("outside");
    fs::create_dir_all(outside.join("aws")).unwrap();
    fs::write(outside.join("aws/credentials.txt"), "outside").unwrap();
    fs::write(outside.join("id.txt"), "outside").unwrap();
    fs::create_dir_all(root.join("site")).unwrap();
    fs::write(root.join("site/real.txt"), "inside").unwrap();
    // A file link, a folder link, a chain, a link to a file inside, a link
    // in the middle of the path.
    symlink(outside.join("id.txt"), root.join("file-link.txt")).unwrap();
    symlink(outside.join("aws"), root.join("aws")).unwrap();
    symlink(root.join("file-link.txt"), root.join("chain.txt")).unwrap();
    symlink(root.join("site/real.txt"), root.join("inside-link.txt")).unwrap();
    symlink(root.join("site"), root.join("site-link")).unwrap();
    let h = handle(&mut fixture, &opened);
    for path in ["file-link.txt", "aws/credentials.txt", "chain.txt", "inside-link.txt", "site-link/real.txt"] {
        assert_eq!(outcome(&mut fixture, &h, &address, path), "notFound", "{path}");
    }
    assert_eq!(outcome(&mut fixture, &h, &address, "site/real.txt"), "ok");
    // A hard link is a file of the workspace (the plan's accepted residue:
    // shown on screen only, never sent anywhere).
    fs::hard_link(outside.join("id.txt"), root.join("hard.txt")).unwrap();
    assert_eq!(outcome(&mut fixture, &h, &address, "hard.txt"), "ok");

    // Swapping a sub-folder for a link after a successful read: the next read
    // walks the handles again and refuses it.
    fs::rename(root.join("site"), root.join("site-moved")).unwrap();
    symlink(&outside, root.join("site")).unwrap();
    fs::write(outside.join("real.txt"), "outside").unwrap();
    assert_eq!(outcome(&mut fixture, &h, &address, "site/real.txt"), "notFound");
}

/// Replace the workspace folder with a real directory at the same path that
/// carries a copy of its Git link or snapshot marker (so the binding check
/// alone would pass) and an attacker's page.
fn swap_root(root: &Path) {
    let moved = root.with_extension("moved");
    fs::rename(root, &moved).unwrap();
    fs::create_dir(root).unwrap();
    let copied = std::process::Command::new("/bin/cp").arg("-R").arg(moved.join(".git")).arg(root.join(".git")).status().unwrap();
    assert!(copied.success());
    fs::write(root.join("page.html"), "swapped").unwrap();
    fs::write(root.join("swapped.html"), "swapped").unwrap();
}

/// B1: a folder swapped in between the verification and the reads of one
/// request is never read; a folder swapped in after resolve is refused.
#[test]
fn reads_go_through_the_verified_handle_even_when_the_folder_is_swapped_mid_request() {
    for kind in [Kind::GitWorktree, Kind::Snapshot] {
        let mut fixture = Fixture::new();
        let opened = workspace_of(&mut fixture, &format!("swap_read_{kind:?}"), kind);
        let (root, address) = (opened.root.clone(), opened.address.clone());
        fs::write(root.join("page.html"), "original").unwrap();
        let h = handle(&mut fixture, &opened);
        // Between this request's verification and its file open.
        let swapped_root = root.clone();
        set_preview_test_hook("read:verified", move || swap_root(&swapped_root));
        let value = read(&mut fixture, &h, &address, "page.html");
        assert_eq!(bytes_of(&value), b"original", "{kind:?}: read the verified folder, not the swapped one");
        assert!(root.join("swapped.html").exists(), "{kind:?}: the hook ran");
        // The next request finds a different folder at the path: refused.
        let error = read_at(&mut fixture, &h, &address, "page.html", 0).unwrap_err();
        // The swapped folder carries the copied Git link or marker, so the
        // binding check alone passes: the recorded identity is what refuses it.
        assert!(error.detail.contains("changed since the preview opened"), "{kind:?}: {}", error.detail);
    }
}

#[test]
fn resolve_looks_up_the_entry_through_the_verified_handle() {
    for kind in [Kind::GitWorktree, Kind::Snapshot] {
        let mut fixture = Fixture::new();
        let opened = workspace_of(&mut fixture, &format!("swap_resolve_{kind:?}"), kind);
        let (root, address) = (opened.root.clone(), opened.address.clone());
        fs::write(root.join("index.html"), "original").unwrap();
        let swapped_root = root.clone();
        set_preview_test_hook("resolve:verified", move || swap_root(&swapped_root));
        let resolved = resolve(&mut fixture, &opened, None);
        // The swapped folder has no index.html: the entry came from the verified one.
        assert_eq!(resolved["entry"], json!("index.html"), "{kind:?}");
        let h = resolved["previewHandle"].as_str().unwrap().to_string();
        // Its identity was recorded from the verified handle, so nothing of the
        // swapped folder is ever served.
        for path in ["swapped.html", "index.html"] {
            let error = read_at(&mut fixture, &h, &address, path, 0).unwrap_err();
            assert!(error.detail.contains("changed since the preview opened"), "{kind:?}: {}", error.detail);
        }
    }
}

#[test]
fn read_asset_is_bounded_and_reads_a_file_once() {
    let mut fixture = Fixture::new();
    let opened = workspace(&mut fixture, "size");
    let (root, address) = (opened.root.clone(), opened.address.clone());
    let chunk = PREVIEW_ASSET_CHUNK_BYTES_V1 as usize;
    let body: Vec<u8> = (0..chunk * 2 + 17).map(|index| (index % 251) as u8).collect();
    fs::write(root.join("big.js"), &body).unwrap();
    let h = handle(&mut fixture, &opened);
    let mut joined = Vec::new();
    let mut offset = 0_u64;
    let mut version = None;
    loop {
        let value = read_at(&mut fixture, &h, &address, "big.js", offset).unwrap();
        assert_eq!(value["outcome"], "ok");
        assert_eq!(value["offset"], offset);
        assert_eq!(value["byteLength"], body.len());
        version.get_or_insert_with(|| value["version"].clone());
        assert_eq!(&value["version"], version.as_ref().unwrap());
        let bytes = bytes_of(&value);
        assert!(bytes.len() <= chunk);
        offset += bytes.len() as u64;
        joined.extend(bytes);
        if offset == body.len() as u64 { break; }
        // Changing the file between chunks: the rest still comes from the one
        // bounded read (one consistent version, no second read).
        fs::write(root.join("big.js"), b"changed").unwrap();
    }
    assert_eq!(joined, body);
    // After the last chunk the kept copy is dropped: a new read sees the file now.
    let fresh = read(&mut fixture, &h, &address, "big.js");
    assert_eq!(bytes_of(&fresh), b"changed");
    assert!(read_at(&mut fixture, &h, &address, "big.js", 7).is_err(), "past the end");
    fs::write(root.join("empty.txt"), b"").unwrap();
    let empty = read(&mut fixture, &h, &address, "empty.txt");
    assert_eq!((empty["outcome"].as_str(), empty["byteLength"].as_u64(), empty["bytesBase64"].as_str()), (Some("ok"), Some(0), Some("")));
    // Over 5 MiB: refused before any byte is read (bounded-read unit test in
    // project::preview covers a file that grows while read).
    let file = fs::File::create(root.join("huge.mp4")).unwrap();
    file.set_len(5 * 1024 * 1024 + 1).unwrap();
    assert_eq!(outcome(&mut fixture, &h, &address, "huge.mp4"), "tooLarge");
}

#[test]
fn read_asset_stops_for_a_discarded_or_foreign_workspace() {
    let mut fixture = Fixture::new();
    let a = workspace(&mut fixture, "a");
    let b = workspace(&mut fixture, "b");
    fs::write(a.root.join("a.txt"), "a").unwrap();
    fs::write(b.root.join("b.txt"), "b").unwrap();
    let ha = handle(&mut fixture, &a);
    let hb = handle(&mut fixture, &b);
    assert_eq!(outcome(&mut fixture, &ha, &a.address, "a.txt"), "ok");
    // Each workspace reads only its own files.
    assert_eq!(outcome(&mut fixture, &ha, &a.address, "b.txt"), "notFound");
    // T15: chat A with workspace B's identity, or A's handle for B, is refused outright.
    let aliased = WorkspaceAddress { chat_id: a.address.chat_id.clone(), ..b.address.clone() };
    assert!(read_at(&mut fixture, &hb, &aliased, "b.txt", 0).is_err());
    assert!(read_at(&mut fixture, &ha, &b.address, "b.txt", 0).is_err());
    fixture.projects.workspace_close(&fixture.storage, &a.address).unwrap();
    assert!(read_at(&mut fixture, &ha, &a.address, "a.txt", 0).is_err());
    assert_eq!(outcome(&mut fixture, &hb, &b.address, "b.txt"), "ok");
}
