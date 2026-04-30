//! MCP resource implementations.

use crate::db::DbHandle;
use rmcp::model::{
    AnnotateAble, ListResourceTemplatesResult, ListResourcesResult, RawResource,
    RawResourceTemplate, ReadResourceResult, ResourceContents,
};

/// List static resources.
pub async fn list_resources(_db: &DbHandle) -> ListResourcesResult {
    ListResourcesResult {
        resources: vec![
            RawResource::new("rummage://tags", "All Tags")
                .with_description("All tags with message counts (JSON)")
                .with_mime_type("application/json")
                .no_annotation(),
            RawResource::new("rummage://stats", "Archive Statistics")
                .with_description("Archive-wide statistics (JSON)")
                .with_mime_type("application/json")
                .no_annotation(),
        ],
        next_cursor: None,
        meta: None,
    }
}

/// List resource templates.
pub async fn list_resource_templates() -> ListResourceTemplatesResult {
    ListResourceTemplatesResult {
        resource_templates: vec![RawResourceTemplate::new(
            "rummage://message/{message_id}",
            "Email Message",
        )
        .with_description("Full content of an email message by ID")
        .with_mime_type("text/plain")
        .no_annotation()],
        next_cursor: None,
        meta: None,
    }
}

/// Read a resource by URI.
pub async fn read_resource(
    db: &DbHandle,
    uri: &str,
) -> Result<ReadResourceResult, rmcp::ErrorData> {
    match uri {
        "rummage://tags" => {
            let tags = db
                .tags()
                .await
                .map_err(|e| rmcp::ErrorData::internal_error(format!("DB error: {e}"), None))?;
            let json = serde_json::to_string(&tags).map_err(|e| {
                rmcp::ErrorData::internal_error(format!("Serialize error: {e}"), None)
            })?;
            Ok(ReadResourceResult::new(vec![ResourceContents::text(
                json,
                "rummage://tags",
            )
            .with_mime_type("application/json")]))
        }
        "rummage://stats" => {
            let stats = db
                .stats()
                .await
                .map_err(|e| rmcp::ErrorData::internal_error(format!("DB error: {e}"), None))?;
            let json = serde_json::to_string(&stats).map_err(|e| {
                rmcp::ErrorData::internal_error(format!("Serialize error: {e}"), None)
            })?;
            Ok(ReadResourceResult::new(vec![ResourceContents::text(
                json,
                "rummage://stats",
            )
            .with_mime_type("application/json")]))
        }
        _ if uri.starts_with("rummage://message/") => {
            let msg_id = uri.trim_start_matches("rummage://message/");
            let raw = db
                .raw_message(msg_id.to_string())
                .await
                .map_err(|e| rmcp::ErrorData::internal_error(format!("DB error: {e}"), None))?;
            let text = String::from_utf8_lossy(&raw).to_string();
            Ok(ReadResourceResult::new(vec![ResourceContents::text(
                text, uri,
            )
            .with_mime_type("text/plain")]))
        }
        _ => Err(rmcp::ErrorData::invalid_params(
            format!("Unknown resource URI: {uri}"),
            None,
        )),
    }
}
