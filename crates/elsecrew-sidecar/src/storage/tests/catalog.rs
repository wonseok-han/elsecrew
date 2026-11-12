use super::*;
use crate::storage::catalog::PROJECT_CATALOG_PAGE_SIZE;

fn add_project(storage: &mut Storage, index: usize, path: &str) {
    let id = format!("project_{index:04}");
    storage
        .append_event(&event(
            &format!("event_{index}"),
            &format!("operation_{index}"),
            "project",
            &id,
            1,
            json!({"project_id":id,"canonical_path":path,"lifecycle":"opened"}),
            "2026-09-29T00:00:00Z",
        ))
        .unwrap();
}

#[test]
fn catalog_pages_are_stable_read_only_and_survive_reopen() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    assert!(storage
        .project_catalog_page(None)
        .unwrap()
        .entries
        .is_empty());
    for index in (0..PROJECT_CATALOG_PAGE_SIZE + 3).rev() {
        add_project(
            &mut storage,
            index,
            &format!("/not-accessed/project-{index}"),
        );
    }
    let changes = storage.connection.total_changes();
    let first = storage.project_catalog_page(None).unwrap();
    assert_eq!(first.entries.len(), PROJECT_CATALOG_PAGE_SIZE);
    assert_eq!(first.entries[0].project_id, "project_0000");
    let second = storage
        .project_catalog_page(first.next_cursor.as_deref())
        .unwrap();
    assert_eq!(second.entries.len(), 3);
    assert!(second.next_cursor.is_none());
    assert_eq!(storage.connection.total_changes(), changes);
    assert!(storage.project_catalog_page(Some("../bad")).is_err());
    drop(storage);
    let reopened = Storage::open(&path).unwrap();
    assert_eq!(reopened.project_catalog_page(None).unwrap(), first);
}

#[test]
fn catalog_byte_budget_continues_without_skipping_a_project() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    for index in 0..6 {
        add_project(
            &mut storage,
            index,
            &format!("/{}-{index}", "x".repeat(120 * 1024)),
        );
    }
    let first = storage.project_catalog_page(None).unwrap();
    assert_eq!(first.entries.len(), 4);
    let second = storage
        .project_catalog_page(first.next_cursor.as_deref())
        .unwrap();
    assert_eq!(second.entries.len(), 2);
    assert_eq!(second.entries[0].project_id, "project_0004");
    assert!(second.next_cursor.is_none());
}

#[test]
fn catalog_rejects_oversized_metadata_instead_of_silent_omission() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    add_project(&mut storage, 0, &format!("/{}", "x".repeat(128 * 1024)));
    assert!(storage.project_catalog_page(None).is_err());
}

#[test]
fn catalog_does_not_expose_rows_before_storage_admission() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    add_project(&mut storage, 0, "/not-accessed/project");
    storage.writes_admitted = false;
    assert!(storage.project_catalog_page(None).is_err());
    storage.writes_admitted = true;
    assert_eq!(storage.project_catalog_page(None).unwrap().entries.len(), 1);
}
