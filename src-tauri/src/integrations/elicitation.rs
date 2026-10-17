//! MCP elicitation: a server asks the user for input while one of its tools
//! runs. Requests are projected into a bounded form Blade renders itself, shown
//! only in the conversation whose approved call is in flight, and answered
//! only by the user. Content is validated against the projected form before
//! it reaches the server.
use super::RuntimeError;
use rmcp::model::{
    ElicitRequestParams, ElicitResult, ElicitationAction, ElicitationSchema, EnumSchema,
    MultiSelectEnumSchema, PrimitiveSchemaDefinition, SingleSelectEnumSchema, StringFormat,
};
use serde::Serialize;
use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use uuid::Uuid;

pub const MAX_FIELDS: usize = 32;
const MAX_OPTIONS: usize = 256;
const MAX_TEXT_BYTES: u32 = 8 * 1024;
const MAX_MESSAGE_CHARS: usize = 4000;
const MAX_LABEL_CHARS: usize = 256;
const MAX_DESCRIPTION_CHARS: usize = 1024;
const MAX_URL_BYTES: usize = 4096;
/// How long a request waits for the user before it is cancelled.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    Text {
        min_length: Option<u32>,
        max_length: u32,
        /// `email`, `uri`, `date` or `date-time`.
        format: Option<&'static str>,
        default: Option<String>,
    },
    Number {
        integer: bool,
        minimum: Option<f64>,
        maximum: Option<f64>,
        default: Option<f64>,
    },
    Boolean {
        default: Option<bool>,
    },
    Choice {
        options: Vec<Choice>,
        multiple: bool,
        min_items: Option<u64>,
        max_items: Option<u64>,
        default: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Field {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub required: bool,
    #[serde(flatten)]
    pub kind: FieldKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Request {
    Form {
        fields: Vec<Field>,
    },
    /// Blade shows the full URL and opens it only when the user consents.
    Url {
        url: String,
        host: String,
    },
}

/// What the conversation shows. Server-supplied text is displayed as text.
#[derive(Debug, Clone, Serialize)]
pub struct ElicitationView {
    pub request_id: Uuid,
    pub server_name: String,
    pub tool_name: String,
    pub message: String,
    #[serde(flatten)]
    pub request: Request,
}

fn clip(value: &str, chars: usize) -> String {
    value.chars().take(chars).collect()
}
fn label(value: Option<&str>) -> Option<String> {
    value.map(|value| clip(value, MAX_LABEL_CHARS))
}
fn description(value: Option<&str>) -> Option<String> {
    value.map(|value| clip(value, MAX_DESCRIPTION_CHARS))
}

fn choices(values: impl IntoIterator<Item = (String, String)>) -> Result<Vec<Choice>, ()> {
    let mut seen = HashSet::new();
    let options: Vec<_> = values
        .into_iter()
        .map(|(value, label)| Choice {
            label: clip(&label, MAX_LABEL_CHARS),
            value,
        })
        .collect();
    if options.is_empty()
        || options.len() > MAX_OPTIONS
        || options.iter().any(|option| {
            option.value.len() > MAX_TEXT_BYTES as usize || !seen.insert(option.value.clone())
        })
    {
        return Err(());
    }
    Ok(options)
}

/// One property, or Err for anything Blade cannot render faithfully.
fn field(name: &str, schema: &PrimitiveSchemaDefinition, required: bool) -> Result<Field, ()> {
    let titled = |title: Option<&str>, about: Option<&str>, kind| Field {
        name: name.to_owned(),
        title: label(title),
        description: description(about),
        required,
        kind,
    };
    let single = |options, default: Option<&String>| FieldKind::Choice {
        options,
        multiple: false,
        min_items: None,
        max_items: None,
        default: default.cloned().into_iter().collect(),
    };
    Ok(match schema {
        PrimitiveSchemaDefinition::String(text) => {
            let max_length = text
                .max_length
                .unwrap_or(MAX_TEXT_BYTES)
                .min(MAX_TEXT_BYTES);
            if text.min_length.is_some_and(|min| min > max_length) {
                return Err(());
            }
            titled(
                text.title.as_deref(),
                text.description.as_deref(),
                FieldKind::Text {
                    min_length: text.min_length,
                    max_length,
                    format: match text.format {
                        Some(StringFormat::Email) => Some("email"),
                        Some(StringFormat::Uri) => Some("uri"),
                        Some(StringFormat::Date) => Some("date"),
                        Some(StringFormat::DateTime) => Some("date-time"),
                        // A newer format: accept plain text rather than guess.
                        _ => None,
                    },
                    default: text
                        .default
                        .as_deref()
                        .map(|value| clip(value, MAX_TEXT_BYTES as usize)),
                },
            )
        }
        PrimitiveSchemaDefinition::Number(number) => titled(
            number.title.as_deref(),
            number.description.as_deref(),
            FieldKind::Number {
                integer: false,
                minimum: number.minimum,
                maximum: number.maximum,
                default: number.default,
            },
        ),
        PrimitiveSchemaDefinition::Integer(number) => titled(
            number.title.as_deref(),
            number.description.as_deref(),
            FieldKind::Number {
                integer: true,
                minimum: number.minimum.map(|value| value as f64),
                maximum: number.maximum.map(|value| value as f64),
                default: number.default.map(|value| value as f64),
            },
        ),
        PrimitiveSchemaDefinition::Boolean(flag) => titled(
            flag.title.as_deref(),
            flag.description.as_deref(),
            FieldKind::Boolean {
                default: flag.default,
            },
        ),
        PrimitiveSchemaDefinition::Enum(EnumSchema::Single(SingleSelectEnumSchema::Untitled(
            list,
        ))) => titled(
            list.title.as_deref(),
            list.description.as_deref(),
            single(
                choices(
                    list.enum_
                        .iter()
                        .map(|value| (value.clone(), value.clone())),
                )?,
                list.default.as_ref(),
            ),
        ),
        PrimitiveSchemaDefinition::Enum(EnumSchema::Single(SingleSelectEnumSchema::Titled(
            list,
        ))) => titled(
            list.title.as_deref(),
            list.description.as_deref(),
            single(
                choices(
                    list.one_of
                        .iter()
                        .map(|item| (item.const_.clone(), item.title.clone())),
                )?,
                list.default.as_ref(),
            ),
        ),
        PrimitiveSchemaDefinition::Enum(EnumSchema::Legacy(list)) => {
            let names = list
                .enum_names
                .as_ref()
                .filter(|names| names.len() == list.enum_.len());
            let options = list.enum_.iter().enumerate().map(|(index, value)| {
                (
                    value.clone(),
                    names.map_or_else(|| value.clone(), |names| names[index].clone()),
                )
            });
            titled(
                list.title.as_deref(),
                list.description.as_deref(),
                single(choices(options)?, list.default.as_ref()),
            )
        }
        PrimitiveSchemaDefinition::Enum(EnumSchema::Multi(list)) => {
            let (title, about, options, min_items, max_items, default) = match list {
                MultiSelectEnumSchema::Untitled(list) => (
                    list.title.as_deref(),
                    list.description.as_deref(),
                    choices(
                        list.items
                            .enum_
                            .iter()
                            .map(|value| (value.clone(), value.clone())),
                    )?,
                    list.min_items,
                    list.max_items,
                    list.default.clone(),
                ),
                MultiSelectEnumSchema::Titled(list) => (
                    list.title.as_deref(),
                    list.description.as_deref(),
                    choices(
                        list.items
                            .any_of
                            .iter()
                            .map(|item| (item.const_.clone(), item.title.clone())),
                    )?,
                    list.min_items,
                    list.max_items,
                    list.default.clone(),
                ),
                _ => return Err(()),
            };
            if min_items.zip(max_items).is_some_and(|(min, max)| min > max) {
                return Err(());
            }
            titled(
                title,
                about,
                FieldKind::Choice {
                    options,
                    multiple: true,
                    min_items,
                    max_items,
                    default: default.unwrap_or_default(),
                },
            )
        }
        #[allow(unreachable_patterns)]
        _ => return Err(()),
    })
}

fn form(schema: &ElicitationSchema) -> Result<Vec<Field>, ()> {
    let order: Vec<String> = schema
        .property_order
        .clone()
        .filter(|order| order.len() == schema.properties.len())
        .unwrap_or_else(|| schema.properties.keys().cloned().collect());
    let required: HashSet<&str> = schema
        .required
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();
    if order.len() > MAX_FIELDS
        || required
            .iter()
            .any(|name| !schema.properties.contains_key(*name))
    {
        return Err(());
    }
    order
        .iter()
        .map(|name| {
            let property = schema.properties.get(name).ok_or(())?;
            if name.is_empty() || name.len() > MAX_LABEL_CHARS {
                return Err(());
            }
            field(name, property, required.contains(name.as_str()))
        })
        .collect()
}

/// HTTPS, or plain HTTP to this machine.
fn openable(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let protected =
        parsed.scheme() == "https" || (parsed.scheme() == "http" && super::http::loopback(&parsed));
    (url.len() <= MAX_URL_BYTES
        && protected
        && parsed.username().is_empty()
        && parsed.password().is_none())
    .then(|| parsed.host_str().map(str::to_owned))
    .flatten()
}

/// The request as Blade will show it, or None when it cannot be shown
/// faithfully (the server is then told the user declined).
pub fn project(params: &ElicitRequestParams) -> Option<(String, Request)> {
    match params {
        ElicitRequestParams::FormElicitationParams {
            message,
            requested_schema,
            ..
        } => Some((
            clip(message, MAX_MESSAGE_CHARS),
            Request::Form {
                fields: form(requested_schema).ok()?,
            },
        )),
        ElicitRequestParams::UrlElicitationParams { message, url, .. } => Some((
            clip(message, MAX_MESSAGE_CHARS),
            Request::Url {
                host: openable(url)?,
                url: url.clone(),
            },
        )),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

fn valid_format(format: &str, value: &str) -> bool {
    match format {
        "email" => {
            let mut parts = value.split('@');
            matches!((parts.next(), parts.next(), parts.next()), (Some(user), Some(domain), None)
                if !user.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.'))
                && !value.chars().any(char::is_whitespace)
        }
        "uri" => reqwest::Url::parse(value).is_ok(),
        "date" => chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok(),
        "date-time" => chrono::DateTime::parse_from_rfc3339(value).is_ok(),
        _ => true,
    }
}

/// The accepted content, keeping only the form's fields, or `invalid_arguments`.
pub fn validate(fields: &[Field], content: &Value) -> Result<Value, RuntimeError> {
    let invalid = || RuntimeError::InvalidArguments;
    let content = content.as_object().ok_or_else(invalid)?;
    if content
        .keys()
        .any(|key| !fields.iter().any(|field| &field.name == key))
    {
        return Err(invalid());
    }
    let mut accepted = Map::new();
    for field in fields {
        let Some(value) = content.get(&field.name).filter(|value| !value.is_null()) else {
            if field.required {
                return Err(invalid());
            }
            continue;
        };
        let ok = match &field.kind {
            FieldKind::Text {
                min_length,
                max_length,
                format,
                ..
            } => value.as_str().is_some_and(|text| {
                let length = text.chars().count() as u32;
                length <= *max_length
                    && text.len() <= MAX_TEXT_BYTES as usize
                    && min_length.is_none_or(|min| length >= min)
                    && (text.is_empty() && !field.required
                        || format.is_none_or(|format| valid_format(format, text)))
            }),
            FieldKind::Number {
                integer,
                minimum,
                maximum,
                ..
            } => value.as_f64().is_some_and(|number| {
                number.is_finite()
                    && (!integer || value.is_i64() || value.is_u64())
                    && minimum.is_none_or(|min| number >= min)
                    && maximum.is_none_or(|max| number <= max)
            }),
            FieldKind::Boolean { .. } => value.is_boolean(),
            FieldKind::Choice {
                options,
                multiple: false,
                ..
            } => value
                .as_str()
                .is_some_and(|choice| options.iter().any(|option| option.value == choice)),
            FieldKind::Choice {
                options,
                multiple: true,
                min_items,
                max_items,
                ..
            } => value.as_array().is_some_and(|items| {
                let mut seen = HashSet::new();
                items.iter().all(|item| {
                    item.as_str().is_some_and(|choice| {
                        seen.insert(choice) && options.iter().any(|option| option.value == choice)
                    })
                }) && min_items.is_none_or(|min| items.len() as u64 >= min)
                    && max_items.is_none_or(|max| items.len() as u64 <= max)
            }),
        };
        if !ok {
            return Err(invalid());
        }
        accepted.insert(field.name.clone(), value.clone());
    }
    Ok(Value::Object(accepted))
}

/// A request waiting for the user, with the channel its answer goes back on.
pub struct Prompt {
    pub message: String,
    pub request: Request,
    pub respond: oneshot::Sender<ElicitResult>,
}
pub type Sink = mpsc::Sender<Prompt>;

fn answer(action: ElicitationAction) -> ElicitResult {
    ElicitResult::new(action)
}

/// Per-session routing from the server's requests to the call in flight.
#[derive(Default)]
pub struct Broker {
    sink: Mutex<Option<(Uuid, Sink)>>,
    waiting: AtomicUsize,
    answered: Mutex<Option<Instant>>,
}

/// Detaches the call's sink when the call ends, however it ends.
pub struct Attached {
    broker: Arc<Broker>,
    id: Uuid,
}
impl Drop for Attached {
    fn drop(&mut self) {
        if let Ok(mut sink) = self.broker.sink.lock() {
            if sink.as_ref().is_some_and(|(id, _)| *id == self.id) {
                *sink = None;
            }
        }
    }
}

struct Waiting<'a>(&'a Broker);
impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.waiting.fetch_sub(1, Ordering::AcqRel);
        if let Ok(mut answered) = self.0.answered.lock() {
            *answered = Some(Instant::now());
        }
    }
}

impl Broker {
    pub fn attach(self: &Arc<Self>, sink: Sink) -> Attached {
        let id = Uuid::new_v4();
        if let Ok(mut current) = self.sink.lock() {
            *current = Some((id, sink));
        }
        Attached {
            broker: self.clone(),
            id,
        }
    }

    /// The user is looking at a request, so the call is not stalled.
    pub fn waiting(&self) -> bool {
        self.waiting.load(Ordering::Acquire) > 0
    }

    pub fn answered_at(&self) -> Option<Instant> {
        self.answered.lock().ok().and_then(|answered| *answered)
    }

    /// Ask the user. Without a call in flight in a conversation that can show
    /// the request, or for a request Blade cannot render, the answer is
    /// `decline`; an unanswered or abandoned request is `cancel`.
    pub async fn ask(&self, params: &ElicitRequestParams) -> ElicitResult {
        let Some((message, request)) = project(params) else {
            return answer(ElicitationAction::Decline);
        };
        let Some(sink) = self
            .sink
            .lock()
            .ok()
            .and_then(|sink| sink.as_ref().map(|(_, sink)| sink.clone()))
        else {
            return answer(ElicitationAction::Decline);
        };
        let (respond, response) = oneshot::channel();
        if sink
            .try_send(Prompt {
                message,
                request,
                respond,
            })
            .is_err()
        {
            return answer(ElicitationAction::Decline);
        }
        self.waiting.fetch_add(1, Ordering::AcqRel);
        let _waiting = Waiting(self);
        match tokio::time::timeout(ANSWER_TIMEOUT, response).await {
            Ok(Ok(result)) => result,
            _ => answer(ElicitationAction::Cancel),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn params(schema: Value) -> ElicitRequestParams {
        serde_json::from_value(
            json!({"mode":"form","message":"Need input","requestedSchema":schema}),
        )
        .unwrap()
    }
    fn fields(schema: Value) -> Vec<Field> {
        let Some((_, Request::Form { fields })) = project(&params(schema)) else {
            panic!()
        };
        fields
    }

    #[test]
    fn forms_project_every_primitive_kind_in_wire_order() {
        let fields = fields(json!({"type":"object","properties":{
            "name":{"type":"string","title":"Name","maxLength":100000},
            "email":{"type":"string","format":"email"},
            "age":{"type":"integer","minimum":0,"maximum":150},
            "ratio":{"type":"number"},
            "subscribe":{"type":"boolean","default":true},
            "plan":{"type":"string","oneOf":[{"const":"free","title":"Free"},{"const":"pro","title":"Pro"}]},
            "tags":{"type":"array","items":{"type":"string","enum":["a","b"]},"maxItems":2},
            "legacy":{"type":"string","enum":["x","y"],"enumNames":["Ex","Why"]}
        },"required":["name","plan"]}));
        let names: Vec<_> = fields.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "name",
                "email",
                "age",
                "ratio",
                "subscribe",
                "plan",
                "tags",
                "legacy"
            ]
        );
        assert!(fields[0].required && fields[5].required && !fields[1].required);
        assert!(matches!(
            fields[0].kind,
            FieldKind::Text {
                max_length: MAX_TEXT_BYTES,
                ..
            }
        ));
        assert!(matches!(
            fields[1].kind,
            FieldKind::Text {
                format: Some("email"),
                ..
            }
        ));
        assert!(matches!(
            fields[2].kind,
            FieldKind::Number { integer: true, .. }
        ));
        assert!(
            matches!(&fields[5].kind, FieldKind::Choice { options, multiple: false, .. } if options[1].label == "Pro")
        );
        assert!(matches!(
            &fields[6].kind,
            FieldKind::Choice {
                multiple: true,
                max_items: Some(2),
                ..
            }
        ));
        assert!(
            matches!(&fields[7].kind, FieldKind::Choice { options, .. } if options[0].label == "Ex")
        );
    }

    #[test]
    fn content_is_validated_against_the_projected_form() {
        let fields = fields(json!({"type":"object","properties":{
            "name":{"type":"string","minLength":2},
            "email":{"type":"string","format":"email"},
            "age":{"type":"integer","minimum":0},
            "plan":{"type":"string","enum":["free","pro"]},
            "tags":{"type":"array","items":{"type":"string","enum":["a","b"]},"maxItems":1},
            "when":{"type":"string","format":"date"}
        },"required":["name"]}));
        let ok = json!({"name":"Ada","email":"ada@example.com","age":36,"plan":"pro","tags":["a"],"when":"2026-10-17"});
        assert_eq!(validate(&fields, &ok).unwrap(), ok);
        for bad in [
            json!({}),
            json!({"name":"A"}),
            json!({"name":"Ada","email":"not an email"}),
            json!({"name":"Ada","age":3.5}),
            json!({"name":"Ada","age":-1}),
            json!({"name":"Ada","plan":"enterprise"}),
            json!({"name":"Ada","tags":["a","b"]}),
            json!({"name":"Ada","tags":["a","a"]}),
            json!({"name":"Ada","when":"17/10/2026"}),
            json!({"name":"Ada","extra":true}),
            json!("Ada"),
        ] {
            assert_eq!(
                validate(&fields, &bad),
                Err(RuntimeError::InvalidArguments),
                "{bad}"
            );
        }
        // Empty optional fields are omitted rather than sent.
        assert_eq!(
            validate(&fields, &json!({"name":"Ada","age":null})).unwrap(),
            json!({"name":"Ada"})
        );
    }

    #[test]
    fn unrenderable_requests_are_not_shown() {
        let too_many: Map<String, Value> = (0..=MAX_FIELDS)
            .map(|index| (format!("f{index}"), json!({"type":"boolean"})))
            .collect();
        assert!(project(&params(json!({"type":"object","properties":too_many}))).is_none());
        assert!(project(&params(
            json!({"type":"object","properties":{"a":{"type":"string"}},"required":["b"]})
        ))
        .is_none());
        let url = |url: &str| -> ElicitRequestParams {
            serde_json::from_value(
                json!({"mode":"url","message":"Open","url":url,"elicitationId":"e1"}),
            )
            .unwrap()
        };
        assert!(project(&url("http://example.com/consent")).is_none());
        assert!(project(&url("https://user:pass@example.com/")).is_none());
        let Some((_, Request::Url { host, .. })) =
            project(&url("https://auth.example.com/consent?x=1"))
        else {
            panic!()
        };
        assert_eq!(host, "auth.example.com");
    }

    #[tokio::test]
    async fn requests_reach_only_an_attached_call_and_time_out_as_cancel() {
        let broker = Arc::new(Broker::default());
        let request = params(json!({"type":"object","properties":{"a":{"type":"string"}}}));
        assert_eq!(
            broker.ask(&request).await.action,
            ElicitationAction::Decline
        );
        let (sink, mut prompts) = mpsc::channel(1);
        let attached = broker.attach(sink);
        let user = async {
            let prompt = prompts.recv().await.unwrap();
            assert_eq!(prompt.message, "Need input");
            let _ = prompt
                .respond
                .send(ElicitResult::new(ElicitationAction::Accept).with_content(json!({"a":"b"})));
        };
        let (result, ()) = tokio::join!(broker.ask(&request), user);
        assert_eq!(result.content, Some(json!({"a":"b"})));
        assert!(!broker.waiting() && broker.answered_at().is_some());
        // An abandoned prompt is a cancel, not a decline.
        let user = async { drop(prompts.recv().await) };
        let (result, ()) = tokio::join!(broker.ask(&request), user);
        assert_eq!(result.action, ElicitationAction::Cancel);
        drop(attached);
        assert_eq!(
            broker.ask(&request).await.action,
            ElicitationAction::Decline
        );
    }
}
