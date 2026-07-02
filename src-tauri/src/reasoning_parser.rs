//! Reasoning Parser Module
//!
//! Extracts and normalizes reasoning blocks from various model formats
//! (e.g., `<think>`, `<thinking>`) into unified events for the UI.
//!
//! ## Supported Formats
//! - `<think>...</think>` (DeepSeek R1, Qwen QwQ, MiniMax M2.1)
//! - `<thinking>...</thinking>` (Alternative format)
//!
//! ## Interleaved Reasoning
//! Models like MiniMax M2.1 and Kimi K2 Thinking support tool calls from
//! within reasoning blocks. This parser handles interruption and resumption.

/// Supported reasoning tag formats
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReasoningFormat {
    /// `<think>...</think>` - DeepSeek, Qwen, MiniMax
    Think,
    /// `<thinking>...</thinking>` - Alternative format
    Thinking,
    GemmaThought,
}

impl ReasoningFormat {
    pub fn open_tags(&self) -> Vec<&'static str> {
        match self {
            ReasoningFormat::Think => vec!["<think>"],
            ReasoningFormat::Thinking => vec!["<thinking>"],
            ReasoningFormat::GemmaThought => vec![
                "<|channel|>thought\n",
                "<|channel|>thought\r\n",
                "<|channel>thought\n",
                "<|channel>thought\r\n",
            ],
        }
    }

    pub fn close_tags(&self) -> Vec<&'static str> {
        match self {
            ReasoningFormat::Think => vec!["</think>"],
            ReasoningFormat::Thinking => vec!["</thinking>"],
            ReasoningFormat::GemmaThought => {
                vec!["<|/channel|>", "<channel|>", "</channel>"]
            }
        }
    }

    pub fn open_tag(&self) -> &'static str {
        self.open_tags()[0]
    }

    pub fn close_tag(&self) -> &'static str {
        self.close_tags()[0]
    }
}

/// Result of parsing a text chunk
#[derive(Debug, Default)]
pub struct ParseResult {
    /// Content to append to message.content (non-reasoning text)
    pub text: String,
    /// Content to append to message.reasoning
    pub reasoning: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningSegment {
    Text(String),
    Reasoning(String),
}

impl ParseResult {
    pub fn new() -> Self {
        Self::default()
    }

}

/// Parser state for streaming reasoning extraction
///
/// Handles multiple tag formats and partial tags across chunk boundaries.
pub struct ReasoningParser {
    /// Formats to check (in priority order)
    formats: Vec<ReasoningFormat>,
    /// Currently active format (if inside a reasoning block)
    current_format: Option<ReasoningFormat>,
    /// Buffer for potential partial tags at chunk boundaries
    tag_buffer: String,
    /// Whether we're currently inside a reasoning block
    in_reasoning: bool,
    /// Buffer for incomplete reasoning when interrupted by tool calls
    interrupted_reasoning: Option<String>,
}

impl Default for ReasoningParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ReasoningParser {
    /// Create a new reasoning parser with default formats
    pub fn new() -> Self {
        Self {
            formats: vec![
                ReasoningFormat::Think,
                ReasoningFormat::Thinking,
                ReasoningFormat::GemmaThought,
            ],
            current_format: None,
            tag_buffer: String::new(),
            in_reasoning: false,
            interrupted_reasoning: None,
        }
    }

    /// Reset parser state (for new message)
    pub fn reset(&mut self) {
        self.current_format = None;
        self.tag_buffer.clear();
        self.in_reasoning = false;
        self.interrupted_reasoning = None;
    }

    /// Flush any buffered content when the stream ends
    /// Returns any content that was buffered but not yet emitted
    pub fn flush(&mut self) -> Vec<ReasoningSegment> {
        let mut segments = Vec::new();

        // If there's content in the tag buffer, emit it as text (partial tag that never completed)
        if !self.tag_buffer.is_empty() {
            if self.in_reasoning {
                segments.push(ReasoningSegment::Reasoning(self.tag_buffer.clone()));
            } else {
                segments.push(ReasoningSegment::Text(self.tag_buffer.clone()));
            }
            self.tag_buffer.clear();
        }

        segments
    }

    /// Check if currently inside a reasoning block
    pub fn is_in_reasoning(&self) -> bool {
        self.in_reasoning
    }

    /// Call when a tool call interrupts the stream
    ///
    /// Returns any accumulated reasoning that should be emitted before the tool call
    pub fn interrupt_for_tool(&mut self) -> Option<String> {
        self.interrupted_reasoning.take()
    }

    /// Call when resuming after tool completion
    pub fn resume_after_tool(&mut self) {
        // State is preserved; just continue parsing
        // The in_reasoning flag remains set if we were mid-block
    }

    /// Process a text chunk, extracting reasoning and text content
    ///
    /// Returns separated text and reasoning content
    pub fn process(&mut self, chunk: &str) -> ParseResult {
        let segments = self.process_segments(chunk);
        let mut result = ParseResult::new();
        for segment in segments {
            match segment {
                ReasoningSegment::Text(text) => result.text.push_str(&text),
                ReasoningSegment::Reasoning(reasoning) => result.reasoning.push_str(&reasoning),
            }
        }
        result
    }

    /// Process a text chunk, returning ordered segments of text/reasoning
    pub fn process_segments(&mut self, chunk: &str) -> Vec<ReasoningSegment> {
        let mut segments = Vec::new();
        let mut remaining = chunk;

        // If we have buffered content from a previous chunk, prepend it
        if !self.tag_buffer.is_empty() {
            let combined = format!("{}{}", self.tag_buffer, chunk);
            self.tag_buffer.clear();
            return self.process_segments(&combined);
        }

        loop {
            if remaining.is_empty() {
                break;
            }

            if !self.in_reasoning {
                if let Some((format, idx, open_tag)) = self.find_opening_tag(remaining) {
                    let before = &remaining[..idx];
                    if !before.is_empty() {
                        segments.push(ReasoningSegment::Text(before.to_string()));
                    }

                    self.in_reasoning = true;
                    self.current_format = Some(format);

                    remaining = &remaining[idx + open_tag.len()..];
                } else if let Some(partial_idx) = self.find_partial_opening(remaining) {
                    let before = &remaining[..partial_idx];
                    if !before.is_empty() {
                        segments.push(ReasoningSegment::Text(before.to_string()));
                    }
                    self.tag_buffer = remaining[partial_idx..].to_string();
                    break;
                } else {
                    segments.push(ReasoningSegment::Text(remaining.to_string()));
                    break;
                }
            } else {
                let format = self.current_format.expect("in_reasoning but no format");

                if let Some((idx, close_tag)) = self.find_closing_tag(remaining, format) {
                    let reasoning_content = &remaining[..idx];
                    if !reasoning_content.is_empty() {
                        segments.push(ReasoningSegment::Reasoning(reasoning_content.to_string()));
                    }

                    self.in_reasoning = false;
                    self.current_format = None;

                    remaining = &remaining[idx + close_tag.len()..];
                } else if let Some(partial_idx) = self.find_partial_closing(remaining, format) {
                    let reasoning_before = &remaining[..partial_idx];
                    if !reasoning_before.is_empty() {
                        segments.push(ReasoningSegment::Reasoning(reasoning_before.to_string()));
                    }
                    self.tag_buffer = remaining[partial_idx..].to_string();
                    break;
                } else {
                    segments.push(ReasoningSegment::Reasoning(remaining.to_string()));
                    break;
                }
            }
        }

        for segment in &segments {
            if let ReasoningSegment::Reasoning(reasoning) = segment {
                let existing = self.interrupted_reasoning.get_or_insert_with(String::new);
                existing.push_str(reasoning);
            }
        }

        segments
    }

    fn find_opening_tag(&self, text: &str) -> Option<(ReasoningFormat, usize, &'static str)> {
        let mut best: Option<(ReasoningFormat, usize, &'static str)> = None;

        for format in &self.formats {
            for tag in format.open_tags() {
                if let Some(idx) = text.find(tag) {
                    let should_replace = best
                        .as_ref()
                        .map(|(_, best_idx, _)| idx < *best_idx)
                        .unwrap_or(true);
                    if should_replace {
                        best = Some((*format, idx, tag));
                    }
                }
            }
        }

        best
    }

    fn find_partial_opening(&self, text: &str) -> Option<usize> {
        for format in &self.formats {
            for tag in format.open_tags() {
                for i in 1..tag.len() {
                    let suffix = &tag[..i];
                    if text.ends_with(suffix) {
                        return Some(text.len() - i);
                    }
                }
            }
        }
        None
    }

    fn find_closing_tag(
        &self,
        text: &str,
        format: ReasoningFormat,
    ) -> Option<(usize, &'static str)> {
        let mut best: Option<(usize, &'static str)> = None;

        for tag in format.close_tags() {
            if let Some(idx) = text.find(tag) {
                let should_replace = best
                    .as_ref()
                    .map(|(best_idx, _)| idx < *best_idx)
                    .unwrap_or(true);
                if should_replace {
                    best = Some((idx, tag));
                }
            }
        }

        best
    }

    fn find_partial_closing(&self, text: &str, format: ReasoningFormat) -> Option<usize> {
        for tag in format.close_tags() {
            for i in 1..tag.len() {
                let suffix = &tag[..i];
                if text.ends_with(suffix) {
                    return Some(text.len() - i);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_think_block() {
        let mut parser = ReasoningParser::new();
        let result = parser.process("<think>This is reasoning</think>And this is text");

        assert_eq!(result.reasoning, "This is reasoning");
        assert_eq!(result.text, "And this is text");
    }

    #[test]
    fn test_text_before_think() {
        let mut parser = ReasoningParser::new();
        let result = parser.process("Hello <think>reasoning</think> world");

        assert_eq!(result.text, "Hello  world");
        assert_eq!(result.reasoning, "reasoning");
    }

    #[test]
    fn test_streaming_chunks() {
        let mut parser = ReasoningParser::new();

        let r1 = parser.process("Hello <thi");
        assert_eq!(r1.text, "Hello ");
        assert_eq!(r1.reasoning, "");

        let r2 = parser.process("nk>This is");
        assert_eq!(r2.text, "");
        assert_eq!(r2.reasoning, "This is");

        let r3 = parser.process(" reasoning</think> done");
        assert_eq!(r3.text, " done");
        assert_eq!(r3.reasoning, " reasoning");
    }

    #[test]
    fn test_thinking_format() {
        let mut parser = ReasoningParser::new();
        let result = parser.process("<thinking>Deep thought</thinking>Answer");

        assert_eq!(result.reasoning, "Deep thought");
        assert_eq!(result.text, "Answer");
    }

    #[test]
    fn test_gemma_thought_channel() {
        let mut parser = ReasoningParser::new();
        let result = parser.process("<|channel|>thought\nDeep thought<|/channel|>Answer");

        assert_eq!(result.reasoning, "Deep thought");
        assert_eq!(result.text, "Answer");
    }

    #[test]
    fn test_gemma_thought_channel_streaming_chunks() {
        let mut parser = ReasoningParser::new();

        let r1 = parser.process("Before <|channel|>thou");
        assert_eq!(r1.text, "Before ");
        assert_eq!(r1.reasoning, "");

        let r2 = parser.process("ght\nThinking");
        assert_eq!(r2.text, "");
        assert_eq!(r2.reasoning, "Thinking");

        let r3 = parser.process(" done<|/channel|>After");
        assert_eq!(r3.reasoning, " done");
        assert_eq!(r3.text, "After");
    }

    #[test]
    fn test_multiple_reasoning_blocks() {
        let mut parser = ReasoningParser::new();
        let result = parser.process("<think>First</think>Text<think>Second</think>More text");

        assert_eq!(result.reasoning, "FirstSecond");
        assert_eq!(result.text, "TextMore text");
    }

    #[test]
    fn test_reset() {
        let mut parser = ReasoningParser::new();
        let _ = parser.process("<think>partial");
        assert!(parser.is_in_reasoning());

        parser.reset();
        assert!(!parser.is_in_reasoning());
    }

    #[test]
    fn test_interrupt_for_tool() {
        let mut parser = ReasoningParser::new();
        let _ = parser.process("<think>Let me search</think>");

        let interrupted = parser.interrupt_for_tool();
        assert_eq!(interrupted, Some("Let me search".to_string()));

        // Second call should return None
        assert_eq!(parser.interrupt_for_tool(), None);
    }
}
