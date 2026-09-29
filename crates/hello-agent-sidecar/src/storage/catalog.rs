use super::Storage;
use crate::SidecarError;
use rusqlite::{params, OptionalExtension};

pub const PROJECT_CATALOG_PAGE_SIZE: usize = 50;
const PATH_BYTES_MAX: usize = 128 * 1024;
const PAGE_BYTES_MAX: usize = 512 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct ProjectCatalogEntry {
    pub project_id: String,
    pub canonical_path: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProjectCatalogPage {
    pub entries: Vec<ProjectCatalogEntry>,
    pub next_cursor: Option<String>,
}

impl Storage {
    pub fn project_catalog_path(&self, project_id: &str) -> Result<Option<String>, SidecarError> {
        self.ensure_writes_admitted()?;
        if !valid_id(project_id) {
            return Err(SidecarError::new(
                "projectCatalogInvalid",
                "invalid project ID",
            ));
        }
        let path: Option<Option<String>> = self.connection.query_row(
            "SELECT CASE WHEN length(CAST(canonical_path AS BLOB)) <= ?2 THEN canonical_path END FROM projects WHERE project_id = ?1",
            params![project_id, PATH_BYTES_MAX as i64],
            |row| row.get(0),
        ).optional().map_err(catalog_sql)?;
        match path {
            None => Ok(None),
            Some(Some(path)) if !path.is_empty() && !path.contains('\0') => Ok(Some(path)),
            _ => Err(SidecarError::new(
                "projectCatalogInvalid",
                "invalid catalog path",
            )),
        }
    }

    /// Metadata only: no filesystem access or project/snapshot activation.
    /// Keyset pagination intentionally uses stable IDs, not a guessed recency.
    pub fn project_catalog_page(
        &self,
        after: Option<&str>,
    ) -> Result<ProjectCatalogPage, SidecarError> {
        self.ensure_writes_admitted()?;
        if after.is_some_and(|value| !valid_id(value)) {
            return Err(SidecarError::new(
                "projectCatalogInvalid",
                "invalid catalog cursor",
            ));
        }
        // Do not materialize projection_json: it can contain many workspaces.
        // Guard each SQL text allocation before returning it to Rust.
        let mut statement = self
            .connection
            .prepare(
                "SELECT CASE WHEN length(CAST(project_id AS BLOB)) <= 128 THEN project_id END, \
             CASE WHEN length(CAST(canonical_path AS BLOB)) <= ?2 THEN canonical_path END \
             FROM projects WHERE project_id > ?1 ORDER BY project_id LIMIT ?3",
            )
            .map_err(catalog_sql)?;
        let mut rows = statement
            .query(params![
                after.unwrap_or(""),
                PATH_BYTES_MAX as i64,
                (PROJECT_CATALOG_PAGE_SIZE + 1) as i64
            ])
            .map_err(catalog_sql)?;
        let mut entries: Vec<ProjectCatalogEntry> = Vec::new();
        let mut bytes = 0;
        let mut more = false;
        while let Some(row) = rows.next().map_err(catalog_sql)? {
            let id: Option<String> = row.get(0).map_err(catalog_sql)?;
            let path: Option<String> = row.get(1).map_err(catalog_sql)?;
            let (Some(project_id), Some(canonical_path)) = (id, path) else {
                return Err(SidecarError::new(
                    "projectCatalogInvalid",
                    "catalog field exceeds its bound",
                ));
            };
            if !valid_id(&project_id) || canonical_path.is_empty() || canonical_path.contains('\0')
            {
                return Err(SidecarError::new(
                    "projectCatalogInvalid",
                    "invalid catalog identity",
                ));
            }
            let entry_bytes = project_id.len() + canonical_path.len();
            if entries.len() == PROJECT_CATALOG_PAGE_SIZE || bytes + entry_bytes > PAGE_BYTES_MAX {
                more = true;
                break;
            }
            bytes += entry_bytes;
            entries.push(ProjectCatalogEntry {
                project_id,
                canonical_path,
            });
        }
        let next_cursor = if more {
            entries.last().map(|entry| entry.project_id.clone())
        } else {
            None
        };
        Ok(ProjectCatalogPage {
            entries,
            next_cursor,
        })
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn catalog_sql(error: rusqlite::Error) -> SidecarError {
    SidecarError::new(
        "projectCatalogReadFailed",
        format!("catalog query failed: {error}"),
    )
}
