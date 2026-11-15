//! What a tool returns.

use harness_core::*;

/// A tool's output.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolOutput {
    /// Finished.
    Done(ToolDone),
    /// Ask the person; the answer becomes the result.
    Ask(QuestionSpec),
}

/// A finished call: what becomes its `tool.result`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolDone {
    /// How it ended.
    pub outcome: ToolOutcome,
    /// What the model sees.
    pub content: Vec<Block>,
    /// The error, for a failure.
    pub error: Option<ErrorInfo>,
    /// Output cut by the output limit.
    pub truncated: Option<Truncation>,
    /// Artifacts written, recorded before the result.
    pub artifacts: Vec<ArtifactEvent>,
}

impl ToolDone {
    /// A success with text.
    pub fn ok(text: impl Into<String>) -> Self {
        Self::with(ToolOutcome::Ok, vec![Block::text(text)], None)
    }

    /// A success with structured data.
    pub fn json(value: Value) -> Self {
        Self::with(ToolOutcome::Ok, vec![Block::Json { value }], None)
    }

    /// A readable failure.
    pub fn error(code: &str, message: &str) -> Self {
        Self::with(
            ToolOutcome::Error,
            vec![Block::text(message)],
            Some(ErrorInfo::new(code, message)),
        )
    }

    /// Denied, with a reason the model can read.
    pub fn denied(reason: &str) -> Self {
        Self::with(ToolOutcome::Denied, vec![Block::text(reason)], None)
    }

    /// Timed out.
    pub fn timeout(message: &str) -> Self {
        Self::with(
            ToolOutcome::Timeout,
            vec![Block::text(message)],
            Some(ErrorInfo::new("tool.timeout", message)),
        )
    }

    /// Cancelled before it finished.
    pub fn cancelled() -> Self {
        Self::with(
            ToolOutcome::Cancelled,
            vec![Block::text("The call was cancelled before it finished.")],
            None,
        )
    }

    fn with(outcome: ToolOutcome, content: Vec<Block>, error: Option<ErrorInfo>) -> Self {
        Self {
            outcome,
            content,
            error,
            truncated: None,
            artifacts: Vec::new(),
        }
    }
}

/// A question the ask-user tool puts to the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestionSpec {
    /// What is asked.
    pub prompt: String,
    /// Choices.
    pub options: Vec<Choice>,
    /// More than one choice allowed.
    pub multiple: bool,
    /// Free text allowed.
    pub free_text: bool,
}

/// Apply an output limit: text over `max_bytes` keeps its head and tail with
/// a marker between, and the result records what was cut.
pub fn limit_output(done: ToolDone, max_bytes: usize) -> ToolDone {
    let text: String = done
        .content
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.len() <= max_bytes
        || done
            .content
            .iter()
            .any(|b| !matches!(b, Block::Text { .. }))
    {
        return done;
    }
    let mut head = max_bytes * 3 / 4;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail_start = text.len() - (max_bytes - max_bytes * 3 / 4);
    while !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let omitted = tail_start - head;
    let kept = format!(
        "{}\n[… {omitted} bytes omitted …]\n{}",
        &text[..head],
        &text[tail_start..]
    );
    ToolDone {
        content: vec![Block::text(kept)],
        truncated: Some(Truncation {
            original_bytes: text.len() as u64,
            head_bytes: head as u64,
            tail_bytes: (text.len() - tail_start) as u64,
        }),
        ..done
    }
}
