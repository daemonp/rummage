//! Thread detail page — standalone reader view for a single thread.

use dioxus::prelude::*;

use crate::api::thread::ThreadDetail;
use crate::ui::search::ReaderPane;

/// Standalone thread page (used when navigating directly to /thread/:id).
#[component]
pub fn ThreadPage(detail: ThreadDetail) -> Element {
    rsx! {
        section { class: "i-reader scroll", style: "flex: 1; min-height: 0;",
            ReaderPane { detail }
        }
    }
}
