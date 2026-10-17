//! Bounded projections of MCP prompts. A prompt is user-controlled: the user
//! picks it, fills its arguments and reviews the generated messages before
//! anything enters the composer.
use super::{
    resources::{MAX_IMAGE_BYTES, MAX_MESSAGE_TEXT_BYTES},
    RuntimeError,
};
use rmcp::model::{ContentBlock, GetPromptResult, Prompt, ResourceContents, Role};
use serde::Serialize;
use std::collections::HashMap;

/// Prompt and argument names are identifiers sent back to the server, so an
/// over-long one is skipped rather than clipped.
const MAX_NAME_BYTES: usize = 256;
const MAX_TITLE_CHARS: usize = 256;
const MAX_DESCRIPTION_CHARS: usize = 1024;
const MAX_URI_BYTES: usize = 2048;
pub const MAX_ARGUMENTS: usize = 32;
pub const MAX_ARGUMENT_BYTES: usize = 16 * 1024;
pub const MAX_ARGUMENTS_BYTES: usize = 64 * 1024;
const MAX_MESSAGES: usize = 64;
const MAX_IMAGES: usize = 8;
const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PromptArgumentEntry {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PromptEntry {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub arguments: Vec<PromptArgumentEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PromptListing {
    pub prompts: Vec<PromptEntry>,
    /// Prompts beyond the bounds, or with unusable names, were omitted.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PromptPart {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        /// Base64, exactly as the server sent it.
        data: String,
    },
    /// Embedded text resource; attached as a resource, fenced as external data.
    Resource {
        uri: String,
        mime_type: Option<String>,
        text: String,
    },
    /// A link the server suggests; never fetched on the prompt's behalf.
    Link {
        uri: String,
        name: String,
    },
    /// Content Blade cannot insert; reported, not silently dropped.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PromptMessageView {
    /// `user` or `assistant`.
    pub role: &'static str,
    pub part: PromptPart,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PromptRender {
    pub name: String,
    pub description: Option<String>,
    pub messages: Vec<PromptMessageView>,
    pub truncated: bool,
}

fn clip(value: &str, chars: usize) -> String {
    value.chars().take(chars).collect()
}
fn optional(value: Option<&str>, chars: usize) -> Option<String> {
    value.map(|value| clip(value, chars))
}
fn usable_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_BYTES
}

impl PromptListing {
    pub(super) fn push(&mut self, prompt: Prompt) {
        let arguments = prompt.arguments.unwrap_or_default();
        if self.prompts.len() >= super::resources::MAX_ENTRIES
            || !usable_name(&prompt.name)
            || arguments.len() > MAX_ARGUMENTS
            || arguments
                .iter()
                .any(|argument| !usable_name(&argument.name))
        {
            self.truncated = true;
            return;
        }
        self.prompts.push(PromptEntry {
            title: optional(prompt.title.as_deref(), MAX_TITLE_CHARS),
            description: optional(prompt.description.as_deref(), MAX_DESCRIPTION_CHARS),
            arguments: arguments
                .into_iter()
                .map(|argument| PromptArgumentEntry {
                    title: optional(argument.title.as_deref(), MAX_TITLE_CHARS),
                    description: optional(argument.description.as_deref(), MAX_DESCRIPTION_CHARS),
                    required: argument.required.unwrap_or(false),
                    name: argument.name,
                })
                .collect(),
            name: prompt.name,
        });
    }
}

/// Bounds on what the webview may send as prompt arguments.
pub fn check_arguments(
    name: &str,
    arguments: &HashMap<String, String>,
) -> Result<(), RuntimeError> {
    let total: usize = arguments
        .iter()
        .map(|(key, value)| key.len() + value.len())
        .sum();
    if !usable_name(name)
        || arguments.len() > MAX_ARGUMENTS
        || total > MAX_ARGUMENTS_BYTES
        || arguments
            .iter()
            .any(|(key, value)| !usable_name(key) || value.len() > MAX_ARGUMENT_BYTES)
    {
        return Err(RuntimeError::InvalidArguments);
    }
    Ok(())
}

fn bounded(text: String, remaining: &mut usize, truncated: &mut bool) -> String {
    let mut end = text.len().min(*remaining);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    *truncated |= end < text.len();
    *remaining -= end;
    let mut text = text;
    text.truncate(end);
    text
}

fn image(mime_type: &str, data: String, images: &mut usize) -> PromptPart {
    let mime_type = mime_type.to_ascii_lowercase();
    // Base64 length bounds the decoded size without decoding.
    if IMAGE_TYPES.contains(&mime_type.as_str())
        && data.len() / 4 * 3 <= MAX_IMAGE_BYTES
        && *images < MAX_IMAGES
    {
        *images += 1;
        PromptPart::Image { mime_type, data }
    } else {
        PromptPart::Unsupported
    }
}

impl PromptRender {
    pub(super) fn project(name: String, result: GetPromptResult) -> Result<Self, RuntimeError> {
        let mut truncated = result.messages.len() > MAX_MESSAGES;
        let mut remaining = MAX_MESSAGE_TEXT_BYTES;
        let mut images = 0;
        let mut messages = Vec::new();
        for message in result.messages.into_iter().take(MAX_MESSAGES) {
            let role = match message.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            let part = match message.content {
                ContentBlock::Text(content) => PromptPart::Text {
                    text: bounded(content.text, &mut remaining, &mut truncated),
                },
                ContentBlock::Image(content) => {
                    image(&content.mime_type, content.data, &mut images)
                }
                ContentBlock::Resource(content) => match content.resource {
                    ResourceContents::TextResourceContents {
                        uri,
                        mime_type,
                        text,
                        ..
                    } if !uri.is_empty() && uri.len() <= MAX_URI_BYTES => PromptPart::Resource {
                        uri,
                        mime_type: optional(mime_type.as_deref(), MAX_TITLE_CHARS),
                        text: bounded(text, &mut remaining, &mut truncated),
                    },
                    ResourceContents::BlobResourceContents {
                        mime_type: Some(mime_type),
                        blob,
                        ..
                    } => image(&mime_type, blob, &mut images),
                    _ => PromptPart::Unsupported,
                },
                ContentBlock::ResourceLink(link) if link.uri.len() <= MAX_URI_BYTES => {
                    PromptPart::Link {
                        name: clip(&link.name, MAX_TITLE_CHARS),
                        uri: link.uri,
                    }
                }
                // Audio, and content kinds newer than this SDK knows.
                _ => PromptPart::Unsupported,
            };
            messages.push(PromptMessageView { role, part });
        }
        if messages.is_empty() {
            return Err(RuntimeError::UnsupportedResult);
        }
        Ok(Self {
            name,
            description: optional(result.description.as_deref(), MAX_DESCRIPTION_CHARS),
            messages,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{PromptArgument, PromptMessage};

    #[test]
    fn listings_skip_prompts_with_unusable_names() {
        let mut listing = PromptListing::default();
        listing.push(Prompt::new(
            "x".repeat(MAX_NAME_BYTES + 1),
            None::<String>,
            None,
        ));
        listing.push(Prompt::new(
            "bad",
            None::<String>,
            Some(vec![PromptArgument::new("")]),
        ));
        assert!(listing.truncated && listing.prompts.is_empty());
        listing.push(Prompt::new(
            "review",
            Some("d".repeat(MAX_DESCRIPTION_CHARS + 5)),
            Some(vec![PromptArgument::new("file").with_required(true)]),
        ));
        let entry = &listing.prompts[0];
        assert_eq!(
            entry.description.as_ref().unwrap().len(),
            MAX_DESCRIPTION_CHARS
        );
        assert!(entry.arguments[0].required);
    }

    #[test]
    fn arguments_are_bounded() {
        let ok = HashMap::from([("file".to_owned(), "src/main.rs".to_owned())]);
        assert!(check_arguments("review", &ok).is_ok());
        assert!(check_arguments("", &ok).is_err());
        let long = HashMap::from([("file".to_owned(), "x".repeat(MAX_ARGUMENT_BYTES + 1))]);
        assert!(check_arguments("review", &long).is_err());
        let many = (0..=MAX_ARGUMENTS)
            .map(|index| (format!("a{index}"), String::new()))
            .collect();
        assert!(check_arguments("review", &many).is_err());
    }

    #[test]
    fn renders_keep_roles_and_bound_text() {
        let result = GetPromptResult::new(vec![
            PromptMessage::new_text(Role::User, "é".repeat(MAX_MESSAGE_TEXT_BYTES)),
            PromptMessage::new_text(Role::Assistant, "more"),
            PromptMessage::new(
                Role::User,
                ContentBlock::ResourceLink(rmcp::model::Resource::new("mem://a", "A")),
            ),
            PromptMessage::new(Role::User, ContentBlock::image("AAAA", "image/svg+xml")),
        ]);
        let render = PromptRender::project("review".into(), result).unwrap();
        assert!(render.truncated);
        let PromptPart::Text { text } = &render.messages[0].part else {
            panic!()
        };
        assert_eq!(text.len(), MAX_MESSAGE_TEXT_BYTES);
        assert_eq!(render.messages[1].role, "assistant");
        assert_eq!(
            render.messages[1].part,
            PromptPart::Text {
                text: String::new()
            }
        );
        assert!(matches!(render.messages[2].part, PromptPart::Link { .. }));
        assert_eq!(render.messages[3].part, PromptPart::Unsupported);
        assert!(PromptRender::project("empty".into(), GetPromptResult::new(vec![])).is_err());
    }
}
