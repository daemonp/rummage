//! MCP prompt implementations — 4 reusable conversation templates.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{PromptMessage, PromptMessageRole};
use schemars::JsonSchema;
use serde::Deserialize;

use super::RummageMcpHandler;

// ── Prompt argument structs ────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SummarizeThreadArgs {
    /// Thread ID to summarize.
    pub thread_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindEmailsAboutArgs {
    /// Natural language description of what to find.
    pub description: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeCorrespondenceArgs {
    /// Email address to analyze correspondence with.
    pub email: String,
}

// ── Prompt implementation block ────────────────────────────────────

#[rmcp::prompt_router(router = "prompt_router", vis = "pub")]
impl RummageMcpHandler {
    /// System prompt with notmuch query syntax reference and search strategy guidance.
    #[rmcp::prompt]
    pub async fn search_guide(&self) -> Vec<PromptMessage> {
        let text =
            "You are assisting a user in searching an email archive via notmuch query syntax.\n\n\
            Query syntax:\n\
            - from:alice@example.com         — messages from a sender\n\
            - to:bob@example.com             — messages to a recipient\n\
            - subject:\"quarterly report\"     — subject line search\n\
            - tag:inbox                       — messages with a tag\n\
            - has:attachment                  — messages with attachments\n\
            - date:2013-06-01..2013-06-30  — date range (YYYY-MM-DD)\n\
            - \"exact phrase\"                — full-text phrase search\n\
            - from:alice AND tag:important    — boolean AND\n\
            - from:alice OR from:bob         — boolean OR\n\
            - NOT tag:spam                    — negation\n\n\
            Strategy:\n\
            1. Start broad (20 results), scan subjects/previews.\n\
            2. Narrow with from:, to:, subject:, date: ranges.\n\
            3. Use has:attachment to find messages with files.\n\
            4. For document content, use get_attachment_text."
                .to_string();

        vec![PromptMessage::new_text(PromptMessageRole::Assistant, text)]
    }

    /// Summarize an email conversation thread.
    #[rmcp::prompt]
    pub async fn summarize_thread(
        &self,
        Parameters(args): Parameters<SummarizeThreadArgs>,
    ) -> Result<Vec<PromptMessage>, rmcp::ErrorData> {
        let thread = self.db.thread(args.thread_id.clone()).await.map_err(|e| {
            rmcp::ErrorData::internal_error(
                format!("Failed to fetch thread {}: {e}", args.thread_id),
                None,
            )
        })?;

        let mut thread_text = String::new();
        for msg in &thread.messages {
            thread_text.push_str(&format!(
                "From: {}\nDate: {}\nSubject: {}\n\n{}\n\n---\n\n",
                msg.headers.from,
                msg.date_relative,
                msg.subject,
                msg.body_text
                    .as_deref()
                    .map(String::from)
                    .unwrap_or_else(|| crate::mail::body::html_to_text(&msg.content)),
            ));
        }

        let system = "Summarize this email thread. Include:\n\
            - Main topic and key decisions\n\
            - Participants and their positions\n\
            - Action items or next steps\n\
            - Attachments mentioned (note if they contain relevant data)\n\
            Keep the summary concise (3-5 bullet points).";

        Ok(vec![
            PromptMessage::new_text(PromptMessageRole::Assistant, system),
            PromptMessage::new_text(PromptMessageRole::User, thread_text),
        ])
    }

    /// Translate a natural language description into a notmuch search query.
    #[rmcp::prompt]
    pub async fn find_emails_about(
        &self,
        Parameters(args): Parameters<FindEmailsAboutArgs>,
    ) -> Vec<PromptMessage> {
        let text = format!(
            "The user wants to find emails matching this description: '{}'\n\n\
            Translate this into a notmuch search query using these operators:\n\
            - from:, to: for people\n\
            - subject: for topics\n\
            - date:YYYY-MM-DD..YYYY-MM-DD for date ranges\n\
            - tag: for categories\n\
            - AND, OR, NOT for boolean logic\n\
            - Quoted strings for exact phrases\n\n\
            Construct the query, run the search tool, and present the results.",
            args.description
        );

        vec![PromptMessage::new_text(PromptMessageRole::Assistant, text)]
    }

    /// Analyze communication patterns with a specific contact.
    #[rmcp::prompt]
    pub async fn analyze_correspondence(
        &self,
        Parameters(args): Parameters<AnalyzeCorrespondenceArgs>,
    ) -> Vec<PromptMessage> {
        let text = format!(
            "Analyze the correspondence with {} in this archive.\n\n\
            Use these tools in sequence:\n\
            1. search for from:{} — messages they sent\n\
            2. search for to:{} — messages sent to them\n\
            3. list_tags scoped to their messages (if possible)\n\
            4. Summarize: frequency, main topics, key threads, relationship context\n\n\
            Present a structured analysis of the communication pattern.",
            args.email, args.email, args.email
        );

        vec![PromptMessage::new_text(PromptMessageRole::Assistant, text)]
    }
}
