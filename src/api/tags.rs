use crate::db::DbHandle;
use crate::error::{AppError, Result};
use axum::extract::State;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct TagList {
    pub tags: Vec<TagItem>,
}

#[derive(Serialize)]
pub struct TagItem {
    pub name: String,
    pub count: usize,
}

pub async fn handler(State(db): State<DbHandle>) -> Result<Json<TagList>> {
    let tags = db.tags().await?;
    Ok(Json(TagList {
        tags: tags
            .into_iter()
            .map(|(name, count)| TagItem { name, count })
            .collect(),
    }))
}

/// Synchronous tag listing against an open notmuch `Database`.
///
/// # Errors
/// Returns `AppError::Notmuch` on query failures.
pub fn do_tags(db: &notmuch::Database) -> Result<Vec<(String, usize)>> {
    let all_tags = db.all_tags().map_err(AppError::Notmuch)?;
    let mut tags: Vec<String> = all_tags.collect();
    tags.sort();

    let mut result = Vec::with_capacity(tags.len());
    for tag in tags {
        let query_str = format!("tag:{tag}");
        let query = db.create_query(&query_str).map_err(AppError::Notmuch)?;
        let count = query.count_messages().map_err(AppError::Notmuch)? as usize;
        result.push((tag, count));
    }

    Ok(result)
}
