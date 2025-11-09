use std::{env, fmt::Write as _, sync::Arc, time::Duration};

use anyhow::Result;
use edit_prediction::{Direction, EditPrediction, EditPredictionProvider};
use futures::StreamExt;
use gpui::{App, Context as GpuiContext, Entity, EntityId, Task};
use http_client::HttpClient;
use language::{language_settings::language_settings, Anchor, Buffer, ToOffset};
use util::paths::PathStyle;

use crate::{
    stream_chat_completion, ChatMessage, ChatOptions, ChatRequest, KeepAlive, OLLAMA_API_URL,
};

const OLLAMA_MODEL_ENV: &str = "OLLAMA_MODEL";
const OLLAMA_API_URL_ENV: &str = "OLLAMA_API_URL";
const OLLAMA_API_KEY_ENV: &str = "OLLAMA_API_KEY";

const MAX_PREFIX_BYTES: usize = 2_000; // Reduced for more focused context
const MAX_SUFFIX_BYTES: usize = 500; // Reduced suffix context
const MAX_PREDICT_TOKENS: isize = 256;
const DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(75);

struct PromptContext {
    prefix: String,
    suffix: String,
    workspace_summary: String,
}

pub struct OllamaCompletionProvider {
    http_client: Arc<dyn HttpClient>,
    api_url: String,
    api_key: Option<String>,
    model: Option<String>,
    pending_refresh: Option<Task<Result<()>>>,
    buffer_id: Option<EntityId>,
    cursor_position: Option<Anchor>,
    prediction: Option<EditPrediction>,
}

impl OllamaCompletionProvider {
    pub fn new(http_client: Arc<dyn HttpClient>) -> Self {
        let api_url = env::var(OLLAMA_API_URL_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| OLLAMA_API_URL.to_string());
        let api_key = env::var(OLLAMA_API_KEY_ENV)
            .ok()
            .filter(|value| !value.is_empty());
        let model = env::var(OLLAMA_MODEL_ENV)
            .ok()
            .filter(|value| !value.is_empty());

        Self {
            http_client,
            api_url,
            api_key,
            model,
            pending_refresh: None,
            buffer_id: None,
            cursor_position: None,
            prediction: None,
        }
    }

    fn clear_prediction(&mut self, cx: &mut GpuiContext<Self>) {
        if self.prediction.take().is_some() {
            self.buffer_id = None;
            self.cursor_position = None;
            cx.notify();
        }
    }

    fn collect_context(buffer: &Buffer, cursor: Anchor, cx: &App) -> PromptContext {
        let snapshot = buffer.snapshot();
        let text = &snapshot.text;
        let cursor_offset = cursor.to_offset(text);

        let start_offset = cursor_offset.saturating_sub(MAX_PREFIX_BYTES);
        let end_offset = (cursor_offset + MAX_SUFFIX_BYTES).min(text.len());

        let start_anchor = text.anchor_before(start_offset);
        let end_anchor = text.anchor_after(end_offset);

        let prefix: String = text.text_for_range(start_anchor..cursor).collect();
        let suffix: String = text.text_for_range(cursor..end_anchor).collect();

        let language_name = buffer
            .language_at(cursor)
            .map(|language| language.name().to_string())
            .unwrap_or_else(|| "unknown".into());

        let settings = language_settings(
            buffer.language_at(cursor).map(|language| language.name()),
            buffer.file(),
            cx,
        );
        let tab_size: u32 = settings.tab_size.get();
        let insert_spaces = !settings.hard_tabs;

        let file_path = buffer
            .file()
            .map(|file| file.path().display(PathStyle::Posix).into_owned());

        let mut workspace_summary = String::new();
        if let Some(path) = file_path {
            let _ = writeln!(&mut workspace_summary, "File: {}", path);
        } else {
            let _ = writeln!(&mut workspace_summary, "File: <untitled>");
        }
        let _ = writeln!(&mut workspace_summary, "Language: {}", language_name);
        let _ = writeln!(&mut workspace_summary, "Tab size: {}", tab_size);
        let _ = writeln!(
            &mut workspace_summary,
            "Insert spaces: {}",
            if insert_spaces { "true" } else { "false" }
        );

        PromptContext {
            prefix,
            suffix,
            workspace_summary,
        }
    }

    fn build_messages(context: &PromptContext) -> Vec<ChatMessage> {
        // Check if model supports FIM (Fill-in-the-Middle)
        let model = std::env::var(OLLAMA_MODEL_ENV).unwrap_or_default();
        if Self::supports_fim(&model) {
            return Self::build_fim_messages(context, &model);
        }

        // Use improved chat-based completion prompt
        let system = ChatMessage::System {
            content: "You are a code autocompletion engine. Generate ONLY the code to insert at the cursor position. Do not include any explanations, comments about your completion, or markdown formatting. Do not repeat existing code. Focus on completing the current line or block based on context.".into(),
        };

        let mut content = String::new();

        // Add minimal context about the environment
        let _ = writeln!(
            &mut content,
            "Language: {}",
            context
                .workspace_summary
                .lines()
                .find(|l| l.starts_with("Language:"))
                .and_then(|l| l.strip_prefix("Language: "))
                .unwrap_or("unknown")
        );

        // Show limited context before cursor (last 15 lines for better context)
        let prefix_lines: Vec<&str> = context
            .prefix
            .lines()
            .rev()
            .take(15)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if !prefix_lines.is_empty() {
            let _ = writeln!(&mut content, "\nCode context before cursor:");
            for line in &prefix_lines {
                let _ = writeln!(&mut content, "{}", line);
            }
        }

        // Mark cursor position clearly
        let _ = writeln!(&mut content, "█  <-- Complete from here");

        // Show limited context after cursor if present
        let suffix_lines: Vec<&str> = context.suffix.lines().take(3).collect();
        if !suffix_lines.is_empty() && !context.suffix.trim().is_empty() {
            let _ = writeln!(&mut content, "\nCode context after cursor:");
            for line in &suffix_lines {
                let _ = writeln!(&mut content, "{}", line);
            }
        }

        let _ = writeln!(
            &mut content,
            "\nGenerate only the code that should be inserted at the cursor position."
        );

        let user = ChatMessage::User {
            content,
            images: None,
        };

        vec![system, user]
    }

    fn build_fim_messages(context: &PromptContext, model: &str) -> Vec<ChatMessage> {
        // Different models use different FIM formats
        let content = if model.contains("codellama") || model.contains("code-llama") {
            // CodeLlama format
            format!("<PRE> {} <SUF>{} <MID>", context.prefix, context.suffix)
        } else if model.contains("deepseek") {
            // DeepSeek format
            format!(
                "<｜fim▁begin｜>{}<｜fim▁hole｜>{}<｜fim▁end｜>",
                context.prefix, context.suffix
            )
        } else if model.contains("starcoder") {
            // StarCoder format
            format!(
                "<fim_prefix>{}<fim_suffix>{}<fim_middle>",
                context.prefix, context.suffix
            )
        } else {
            // Generic FIM format that some models understand
            format!(
                "<|fim_prefix|>{}<|fim_suffix|>{}<|fim_middle|>",
                context.prefix, context.suffix
            )
        };

        vec![ChatMessage::User {
            content,
            images: None,
        }]
    }

    fn supports_fim(model: &str) -> bool {
        let model_lower = model.to_lowercase();
        model_lower.contains("codellama")
            || model_lower.contains("code-llama")
            || model_lower.contains("deepseek")
            || model_lower.contains("starcoder")
            || model_lower.contains("codegemma")
            || model_lower.contains("granite-code")
    }
}

impl EditPredictionProvider for OllamaCompletionProvider {
    fn name() -> &'static str {
        "ollama"
    }

    fn display_name() -> &'static str {
        "Ollama"
    }

    fn show_completions_in_menu() -> bool {
        true
    }

    fn show_tab_accept_marker() -> bool {
        true
    }

    fn supports_jump_to_edit() -> bool {
        false
    }

    fn is_enabled(&self, _buffer: &Entity<Buffer>, _cursor_position: Anchor, _cx: &App) -> bool {
        self.model.is_some()
    }

    fn is_refreshing(&self) -> bool {
        self.pending_refresh.is_some()
    }

    fn refresh(
        &mut self,
        buffer: Entity<Buffer>,
        cursor_position: Anchor,
        debounce: bool,
        cx: &mut GpuiContext<Self>,
    ) {
        let Some(model) = self.model.clone() else {
            self.clear_prediction(cx);
            return;
        };

        let cursor_anchor = cursor_position;

        let context = {
            let buffer_ref = buffer.read(cx);
            Self::collect_context(&buffer_ref, cursor_anchor, cx)
        };
        let prefix_for_post = context.prefix.clone();
        let suffix_for_post = context.suffix.clone();
        let messages = Self::build_messages(&context);

        self.prediction = None;
        self.buffer_id = None;
        self.cursor_position = None;

        let http_client = Arc::clone(&self.http_client);
        let api_url = self.api_url.clone();
        let api_key = self.api_key.clone();
        let buffer_id = buffer.entity_id();

        self.pending_refresh = Some(cx.spawn(async move |this, cx| {
            if debounce {
                cx.background_executor().timer(DEBOUNCE_TIMEOUT).await;
            }

            let request = ChatRequest {
                model,
                messages,
                stream: true,
                keep_alive: KeepAlive::default(),
                options: Some(ChatOptions {
                    num_predict: Some(MAX_PREDICT_TOKENS),
                    ..Default::default()
                }),
                tools: Vec::new(),
                think: None,
            };

            let mut stream =
                stream_chat_completion(http_client.as_ref(), &api_url, api_key.as_deref(), request)
                    .await?;

            let mut completion = String::new();
            while let Some(delta) = stream.next().await {
                let delta = delta?;
                if let ChatMessage::Assistant { content, .. } = delta.message {
                    completion.push_str(&content);
                }
                if delta.done {
                    break;
                }
            }

            // Clean up the completion
            completion = completion.trim_matches('\u{feff}').to_string();

            // For FIM models, the response should be clean completion text
            // For chat models, we may need to do light cleanup
            let model = std::env::var(OLLAMA_MODEL_ENV).unwrap_or_default();
            if !OllamaCompletionProvider::supports_fim(&model) {
                // Remove any markdown code block markers if present
                completion = strip_markdown_code_blocks(&completion);
                // Only do minimal trimming for chat-based completions
                // since we're now asking for completion only, not full rewrite
                trim_redundant_prefix(&mut completion, &prefix_for_post);
                trim_redundant_suffix(&mut completion, &suffix_for_post);
            }

            let _ = this.update(cx, |this, cx| -> anyhow::Result<()> {
                this.pending_refresh = None;

                if completion.trim().is_empty() {
                    this.prediction = None;
                    this.buffer_id = None;
                    this.cursor_position = None;
                    cx.notify();
                    return Ok(());
                }

                this.prediction = Some(EditPrediction::Local {
                    id: None,
                    edits: vec![(cursor_anchor..cursor_anchor, completion.clone())],
                    edit_preview: None,
                });
                this.buffer_id = Some(buffer_id);
                this.cursor_position = Some(cursor_anchor);
                cx.notify();
                Ok(())
            })?;

            Ok(())
        }));
    }

    fn cycle(
        &mut self,
        _buffer: Entity<Buffer>,
        _cursor_position: Anchor,
        _direction: Direction,
        _cx: &mut GpuiContext<Self>,
    ) {
    }

    fn accept(&mut self, cx: &mut GpuiContext<Self>) {
        self.clear_prediction(cx);
    }

    fn discard(&mut self, cx: &mut GpuiContext<Self>) {
        self.clear_prediction(cx);
    }

    fn suggest(
        &mut self,
        buffer: &Entity<Buffer>,
        cursor_position: Anchor,
        _cx: &mut GpuiContext<Self>,
    ) -> Option<EditPrediction> {
        if self.buffer_id == Some(buffer.entity_id())
            && self.cursor_position == Some(cursor_position)
        {
            self.prediction.clone()
        } else {
            None
        }
    }
}

fn strip_markdown_code_blocks(text: &str) -> String {
    let text = text.trim();

    // Check if wrapped in markdown code block
    if text.starts_with("```") {
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() > 2 && lines.last().map_or(false, |l| l.trim() == "```") {
            // Extract content between code blocks
            return lines[1..lines.len() - 1].join("\n");
        }
    }

    // Check for inline code markers
    if text.starts_with('`') && text.ends_with('`') && text.len() > 2 {
        return text[1..text.len() - 1].to_string();
    }

    text.to_string()
}

fn trim_redundant_prefix(completion: &mut String, prefix: &str) {
    let max = prefix.len().min(completion.len()).min(100); // Check at most 100 chars
    for count in (1..=max).rev() {
        let start_in_prefix = prefix.len() - count;
        if !completion.is_char_boundary(count) || !prefix.is_char_boundary(start_in_prefix) {
            continue;
        }

        if prefix[start_in_prefix..] == completion[..count] {
            completion.drain(..count);
            break;
        }
    }
}

fn trim_redundant_suffix(completion: &mut String, suffix: &str) {
    let max = suffix.len().min(completion.len()).min(80);
    for count in (1..=max).rev() {
        let end_in_completion = completion.len() - count;
        if !completion.is_char_boundary(end_in_completion) || !suffix.is_char_boundary(count) {
            continue;
        }

        if completion[end_in_completion..] == suffix[..count] {
            completion.truncate(end_in_completion);
            break;
        }
    }
}
