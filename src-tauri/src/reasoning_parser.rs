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
}

impl ReasoningFormat {
    /// Returns the opening tag for this format
    pub fn open_tag(&self) -> &'static str {
        match self {
            ReasoningFormat::Think => "<think>",
            ReasoningFormat::Thinking => "<thinking>",
        }
    }

    /// Returns the closing tag for this format
    pub fn close_tag(&self) -> &'static str {
        match self {
            ReasoningFormat::Think => "</think>",
            ReasoningFormat::Thinking => "</thinking>",
        }
    }

    /// Returns the length of the opening tag
    pub fn open_len(&self) -> usize {
        self.open_tag().len()
    }

    /// Returns the length of the closing tag
    pub fn close_len(&self) -> usize {
        self.close_tag().len()
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

impl ParseResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        self.reasoning = reasoning.into();
        self
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
            formats: vec![ReasoningFormat::Think, ReasoningFormat::Thinking],
            current_format: None,
            tag_buffer: String::new(),
            in_reasoning: false,
            interrupted_reasoning: None,
        }
    }

    /// Create a parser with specific formats
    pub fn with_formats(formats: Vec<ReasoningFormat>) -> Self {
        Self {
            formats,
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
        let mut result = ParseResult::new();
        let mut remaining = chunk;

        // If we have buffered content from a previous chunk, prepend it
        if !self.tag_buffer.is_empty() {
            let combined = format!("{}{}", self.tag_buffer, chunk);
            self.tag_buffer.clear();
            return self.process(&combined);
        }

        loop {
            if remaining.is_empty() {
                break;
            }

            if !self.in_reasoning {
                // Look for opening tags
                if let Some((format, idx)) = self.find_opening_tag(remaining) {
                    // Emit text before the tag
                    let before = &remaining[..idx];
                    result.text.push_str(before);

                    // Enter reasoning mode
                    self.in_reasoning = true;
                    self.current_format = Some(format);

                    // Skip past the opening tag
                    remaining = &remaining[idx + format.open_len()..];
                } else {
                    // Check for potential partial tag at end
                    if let Some(partial_idx) = self.find_partial_opening(remaining) {
                        // Emit text before the potential partial tag
                        result.text.push_str(&remaining[..partial_idx]);
                        // Buffer the rest for next chunk
                        self.tag_buffer = remaining[partial_idx..].to_string();
                        break;
                    } else {
                        // No tags found, emit all as text
                        result.text.push_str(remaining);
                        break;
                    }
                }
            } else {
                // Inside reasoning block - look for closing tag
                let format = self.current_format.expect("in_reasoning but no format");

                if let Some(idx) = remaining.find(format.close_tag()) {
                    // Found closing tag
                    let reasoning_content = &remaining[..idx];
                    result.reasoning.push_str(reasoning_content);

                    // Exit reasoning mode
                    self.in_reasoning = false;
                    self.current_format = None;

                    // Skip past the closing tag
                    remaining = &remaining[idx + format.close_len()..];
                } else {
                    // Check for partial closing tag at end
                    if let Some(partial_idx) = self.find_partial_closing(remaining, format) {
                        // Emit reasoning before the potential partial tag
                        result.reasoning.push_str(&remaining[..partial_idx]);
                        // Buffer the rest for next chunk
                        self.tag_buffer = remaining[partial_idx..].to_string();
                        break;
                    } else {
                        // No closing tag found, all is reasoning
                        result.reasoning.push_str(remaining);
                        break;
                    }
                }
            }
        }

        // Store reasoning for potential tool interruption
        if !result.reasoning.is_empty() {
            let existing = self.interrupted_reasoning.get_or_insert_with(String::new);
            existing.push_str(&result.reasoning);
        }

        result
    }

    /// Find the first opening tag in the text
    fn find_opening_tag(&self, text: &str) -> Option<(ReasoningFormat, usize)> {
        let mut best: Option<(ReasoningFormat, usize)> = None;

        for format in &self.formats {
            if let Some(idx) = text.find(format.open_tag()) {
                match best {
                    None => best = Some((*format, idx)),
                    Some((_, best_idx)) if idx < best_idx => best = Some((*format, idx)),
                    _ => {}
                }
            }
        }

        best
    }

    /// Check if the end of text contains a partial opening tag
    fn find_partial_opening(&self, text: &str) -> Option<usize> {
        // Check last N characters for partial matches
        for format in &self.formats {
            let tag = format.open_tag();
            for i in 1..tag.len() {
                let suffix = &tag[..i];
                if text.ends_with(suffix) {
                    return Some(text.len() - i);
                }
            }
        }
        None
    }

    /// Check if the end of text contains a partial closing tag
    fn find_partial_closing(&self, text: &str, format: ReasoningFormat) -> Option<usize> {
        let tag = format.close_tag();
        for i in 1..tag.len() {
            let suffix = &tag[..i];
            if text.ends_with(suffix) {
                return Some(text.len() - i);
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
