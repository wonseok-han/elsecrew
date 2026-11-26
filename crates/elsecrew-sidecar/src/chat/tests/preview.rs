//! Result preview reads against a real chat workspace (plan 2.4 T4·T5·T15,
//! 2.6 "Rust 단위"): the address comes from the chat binding, paths never
//! leave the workspace, hidden and unknown files never open.

use super::*;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use elsecrew_protocol::preview_v1::PREVIEW_ASSET_CHUNK_BYTES_V1;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

fn workspace(fixture: &mut Fixture, name: &str) -> (Value, WorkspaceAddress, PathBuf) {
    let snapshot = fixture.snapshot(&format!("req_preview_snapshot_{name}"));
    let chat = fixture.create_chat(&snapshot, "preview goal", &format!("req_preview_chat_{name}"));
    let address = address_from_chat(&chat).unwrap();
    let path = PathBuf::from(snapshot["worktreePath"].as_str().unwrap());
    (chat, address, path)
}

fn read(fixture: &Fixture, address: &WorkspaceAddress, path: &str) -> Value {
    fixture.projects.preview_read_asset(&fixture.storage, address, path, 0).unwrap().result
}

fn outcome(fixture: &Fixture, address: &WorkspaceAddress, path: &str) -> String {
    read(fixture, address, path)["outcome"].as_str().unwrap().to_string()
}

#[test]
fn resolve_takes_the_address_from_the_chat_and_finds_the_entry_in_order() {
    let mut fixture = Fixture::new();
    let (chat, address, root) = workspace(&mut fixture, "order");
    let chat_id = chat["chatId"].as_str().unwrap();
    let project_id = fixture.project_id.clone();
    let resolve = |fixture: &Fixture, entry: Option<&str>| {
        fixture.projects.preview_resolve(&fixture.storage, &project_id, chat_id, entry).unwrap().result
    };
    // No page yet: nothing to show.
    let empty = resolve(&fixture, None);
    assert_eq!(empty["entry"], Value::Null);
    assert_eq!(empty["worktreeId"], json!(address.worktree_id));
    assert_eq!(empty["workspaceIdentity"], json!(address.workspace_identity));
    assert_eq!(empty["chatId"], json!(chat_id));

    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("dist")).unwrap();
    fs::write(root.join("docs/index.html"), "docs").unwrap();
    assert_eq!(resolve(&fixture, None)["entry"], json!("docs/index.html"));
    fs::write(root.join("dist/index.html"), "dist").unwrap();
    assert_eq!(resolve(&fixture, None)["entry"], json!("dist/index.html"));
    fs::write(root.join("index.html"), "root").unwrap();
    assert_eq!(resolve(&fixture, None)["entry"], json!("index.html"));
    // A named page wins; a non-page, hidden or missing one is refused.
    assert_eq!(resolve(&fixture, Some("docs/index.html"))["entry"], json!("docs/index.html"));
    for (entry, refusal) in [("app.js", "notPage"), (".git/index.html", "hidden"), ("none.html", "notFound"), ("../x.html", "traversal")] {
        let value = resolve(&fixture, Some(entry));
        assert_eq!(value["entry"], Value::Null, "{entry}");
        assert_eq!(value["refusal"], json!(refusal), "{entry}");
    }
    // A root index.html that is a link is not an entry.
    fs::remove_file(root.join("index.html")).unwrap();
    symlink(root.join("dist/index.html"), root.join("index.html")).unwrap();
    assert_eq!(resolve(&fixture, None)["entry"], json!("dist/index.html"));

    // Another project's id, or an unknown chat, never resolves.
    assert!(fixture.projects.preview_resolve(&fixture.storage, "project_other", chat_id, None).is_err());
    assert!(fixture.projects.preview_resolve(&fixture.storage, &project_id, "chat_missing", None).is_err());
}

#[test]
fn read_asset_serves_allowed_files_and_refuses_everything_else() {
    let mut fixture = Fixture::new();
    let (_chat, address, root) = workspace(&mut fixture, "paths");
    fs::create_dir_all(root.join("css")).unwrap();
    fs::create_dir_all(root.join("a/.secret")).unwrap();
    fs::write(root.join("index.html"), "<p>ok</p>").unwrap();
    fs::write(root.join("css/app.css"), "p{}").unwrap();
    fs::write(root.join("logo.png"), [0_u8, 159, 146, 150, 0, 1]).unwrap();
    fs::write(root.join(".env"), "fixture-only").unwrap();
    fs::write(root.join("a/.secret/x.js"), "fixture-only").unwrap();
    fs::write(root.join("key.pem"), "fixture-only").unwrap();
    fs::write(root.join("config.env"), "fixture-only").unwrap();

    let page = read(&fixture, &address, "index.html");
    assert_eq!(page["outcome"], "ok");
    assert_eq!(page["byteLength"], 9);
    assert_eq!(BASE64.decode(page["bytesBase64"].as_str().unwrap()).unwrap(), b"<p>ok</p>");
    assert_eq!(page["version"], json!(sha256_file_version_v1(b"<p>ok</p>")));
    let image = read(&fixture, &address, "logo.png");
    assert_eq!(BASE64.decode(image["bytesBase64"].as_str().unwrap()).unwrap(), [0_u8, 159, 146, 150, 0, 1]);
    assert_eq!(outcome(&fixture, &address, "css/app.css"), "ok");

    // T5: hidden files, Git metadata, secret-looking types.
    for path in [".env", ".git/HEAD", ".git/config", "a/.secret/x.js", "key.pem", "config.env"] {
        assert_eq!(outcome(&fixture, &address, path), "refused", "{path}");
    }
    // T4: traversal, absolute and encoded escapes never open anything.
    for path in ["../source/src/value.txt", "css/../../x.txt", "/etc/hosts.txt", "%2e%2e/x.txt", "a%2fb.txt", "a\\b.txt", "a\0.txt", "css//app.css"] {
        assert_eq!(outcome(&fixture, &address, path), "refused", "{path:?}");
    }
    assert_eq!(outcome(&fixture, &address, "missing.html"), "notFound");
    assert_eq!(outcome(&fixture, &address, "css"), "refused"); // no extension
    fs::create_dir_all(root.join("folder.html")).unwrap();
    assert_eq!(outcome(&fixture, &address, "folder.html"), "notFound");
}

#[test]
fn read_asset_never_follows_links_out_of_the_workspace() {
    let mut fixture = Fixture::new();
    let (_chat, address, root) = workspace(&mut fixture, "links");
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
    for path in ["file-link.txt", "aws/credentials.txt", "chain.txt", "inside-link.txt", "site-link/real.txt"] {
        assert_eq!(outcome(&fixture, &address, path), "notFound", "{path}");
    }
    assert_eq!(outcome(&fixture, &address, "site/real.txt"), "ok");
    // A hard link is a file of the workspace (the plan's accepted residue:
    // shown on screen only, never sent anywhere).
    fs::hard_link(outside.join("id.txt"), root.join("hard.txt")).unwrap();
    assert_eq!(outcome(&fixture, &address, "hard.txt"), "ok");

    // Swapping a folder for a link after a successful read: the next read
    // walks the handles again and refuses it.
    assert_eq!(outcome(&fixture, &address, "site/real.txt"), "ok");
    fs::rename(root.join("site"), root.join("site-moved")).unwrap();
    symlink(&outside, root.join("site")).unwrap();
    fs::write(outside.join("real.txt"), "outside").unwrap();
    assert_eq!(outcome(&fixture, &address, "site/real.txt"), "notFound");
}

#[test]
fn read_asset_is_bounded_and_chunked() {
    let mut fixture = Fixture::new();
    let (_chat, address, root) = workspace(&mut fixture, "size");
    let chunk = PREVIEW_ASSET_CHUNK_BYTES_V1 as usize;
    let body: Vec<u8> = (0..chunk * 2 + 17).map(|index| (index % 251) as u8).collect();
    fs::write(root.join("big.js"), &body).unwrap();
    let mut joined = Vec::new();
    let mut offset = 0_u64;
    let mut version = None;
    loop {
        let value = fixture.projects.preview_read_asset(&fixture.storage, &address, "big.js", offset).unwrap().result;
        assert_eq!(value["outcome"], "ok");
        assert_eq!(value["offset"], offset);
        assert_eq!(value["byteLength"], body.len());
        version.get_or_insert_with(|| value["version"].clone());
        assert_eq!(&value["version"], version.as_ref().unwrap());
        let bytes = BASE64.decode(value["bytesBase64"].as_str().unwrap()).unwrap();
        assert!(bytes.len() <= chunk);
        offset += bytes.len() as u64;
        joined.extend(bytes);
        if offset == body.len() as u64 { break; }
    }
    assert_eq!(joined, body);
    assert!(fixture.projects.preview_read_asset(&fixture.storage, &address, "big.js", offset).is_err());
    fs::write(root.join("empty.txt"), b"").unwrap();
    let empty = read(&fixture, &address, "empty.txt");
    assert_eq!((empty["outcome"].as_str(), empty["byteLength"].as_u64(), empty["bytesBase64"].as_str()), (Some("ok"), Some(0), Some("")));
    let file = fs::File::create(root.join("huge.mp4")).unwrap();
    file.set_len(5 * 1024 * 1024 + 1).unwrap();
    assert_eq!(outcome(&fixture, &address, "huge.mp4"), "tooLarge");
}

#[test]
fn read_asset_stops_for_a_discarded_or_foreign_workspace() {
    let mut fixture = Fixture::new();
    let (_chat_a, address_a, root_a) = workspace(&mut fixture, "a");
    let (_chat_b, address_b, root_b) = workspace(&mut fixture, "b");
    fs::write(root_a.join("a.txt"), "a").unwrap();
    fs::write(root_b.join("b.txt"), "b").unwrap();
    assert_eq!(outcome(&fixture, &address_a, "a.txt"), "ok");
    // Each workspace reads only its own files.
    assert_eq!(outcome(&fixture, &address_a, "b.txt"), "notFound");
    // T15: chat A with workspace B's identity is refused outright.
    let aliased = WorkspaceAddress { chat_id: address_a.chat_id.clone(), ..address_b.clone() };
    assert!(fixture.projects.preview_read_asset(&fixture.storage, &aliased, "b.txt", 0).is_err());
    fixture.projects.workspace_close(&fixture.storage, &address_a).unwrap();
    assert!(fixture.projects.preview_read_asset(&fixture.storage, &address_a, "a.txt", 0).is_err());
    assert_eq!(outcome(&fixture, &address_b, "b.txt"), "ok");
}
