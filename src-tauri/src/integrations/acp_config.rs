//! The settings an ACP agent offers for its session (model, effort, mode…).
//! Blade shows them as the agent declares them and only sends back a value
//! the agent listed. When the agent offers configuration options, its legacy
//! session modes are ignored.
use super::{
    acp_session::{clip, MAX_DESCRIPTION, MAX_LABEL},
    RuntimeError,
};
use agent_client_protocol::schema::v1::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
    SessionConfigSelectOptions, SessionModeState,
};
use serde::{Deserialize, Serialize};

const MAX_OPTIONS: usize = 32;
/// Across all groups of one option; model lists can be long.
const MAX_CHOICES: usize = 512;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ChoiceView {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ChoiceGroupView {
    /// `None` for an agent that does not group its choices.
    pub name: Option<String>,
    pub choices: Vec<ChoiceView>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigKindView {
    Select {
        current_value: String,
        groups: Vec<ChoiceGroupView>,
    },
    Boolean {
        current_value: bool,
    },
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ConfigOptionView {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// `mode`, `model`, `model_config`, `thought_level` or the agent's own.
    pub category: Option<String>,
    #[serde(flatten)]
    pub kind: ConfigKindView,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ModesView {
    pub current_mode_id: String,
    pub modes: Vec<ChoiceView>,
}

/// The agent's session settings, in the agent's order.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct SessionConfig {
    pub options: Vec<ConfigOptionView>,
    /// Legacy session modes, kept only when the agent offers no options.
    pub modes: Option<ModesView>,
}

/// A change the user asked for.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigChange {
    Select { config_id: String, value: String },
    Boolean { config_id: String, value: bool },
    Mode { mode_id: String },
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_LABEL
}

fn description(text: Option<&str>) -> Option<String> {
    text.map(|text| clip(text, MAX_DESCRIPTION))
}

fn choice(option: &SessionConfigSelectOption) -> Option<ChoiceView> {
    let value = option.value.to_string();
    valid_id(&value).then(|| ChoiceView {
        value,
        name: clip(&option.name, MAX_LABEL),
        description: description(option.description.as_deref()),
    })
}

fn category(category: &SessionConfigOptionCategory) -> String {
    match category {
        SessionConfigOptionCategory::Mode => "mode".into(),
        SessionConfigOptionCategory::Model => "model".into(),
        SessionConfigOptionCategory::ModelConfig => "model_config".into(),
        SessionConfigOptionCategory::ThoughtLevel => "thought_level".into(),
        SessionConfigOptionCategory::Other(other) => clip(other, MAX_LABEL),
        _ => "other".into(),
    }
}

/// Unique, non-empty values, at most `MAX_CHOICES` in all.
fn choices<'a>(
    options: impl Iterator<Item = &'a SessionConfigSelectOption>,
    seen: &mut Vec<String>,
) -> Vec<ChoiceView> {
    options
        .filter_map(choice)
        .filter(|choice| {
            let fresh = seen.len() < MAX_CHOICES && !seen.contains(&choice.value);
            if fresh {
                seen.push(choice.value.clone());
            }
            fresh
        })
        .collect()
}

fn option_view(option: &SessionConfigOption) -> Option<ConfigOptionView> {
    let id = option.id.to_string();
    if !valid_id(&id) {
        return None;
    }
    let kind = match &option.kind {
        SessionConfigKind::Select(select) => {
            let mut seen = Vec::new();
            let groups: Vec<ChoiceGroupView> = match &select.options {
                SessionConfigSelectOptions::Ungrouped(options) => vec![ChoiceGroupView {
                    name: None,
                    choices: choices(options.iter(), &mut seen),
                }],
                SessionConfigSelectOptions::Grouped(groups) => groups
                    .iter()
                    .map(|group| ChoiceGroupView {
                        name: Some(clip(&group.name, MAX_LABEL)),
                        choices: choices(group.options.iter(), &mut seen),
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let groups: Vec<_> = groups
                .into_iter()
                .filter(|group| !group.choices.is_empty())
                .collect();
            if groups.is_empty() {
                return None;
            }
            ConfigKindView::Select {
                current_value: clip(&select.current_value.to_string(), MAX_LABEL),
                groups,
            }
        }
        SessionConfigKind::Boolean(boolean) => ConfigKindView::Boolean {
            current_value: boolean.current_value,
        },
        _ => return None,
    };
    Some(ConfigOptionView {
        id,
        name: clip(&option.name, MAX_LABEL),
        description: description(option.description.as_deref()),
        category: option.category.as_ref().map(category),
        kind,
    })
}

fn options_view(options: &[SessionConfigOption]) -> Vec<ConfigOptionView> {
    let mut views: Vec<ConfigOptionView> = Vec::new();
    for view in options.iter().filter_map(option_view) {
        if views.len() == MAX_OPTIONS {
            break;
        }
        if !views.iter().any(|known| known.id == view.id) {
            views.push(view);
        }
    }
    views
}

fn modes_view(modes: &SessionModeState) -> Option<ModesView> {
    let mut seen = Vec::new();
    let modes_list: Vec<ChoiceView> = modes
        .available_modes
        .iter()
        .filter_map(|mode| {
            let value = mode.id.to_string();
            (valid_id(&value) && !seen.contains(&value) && seen.len() < MAX_CHOICES).then(|| {
                seen.push(value.clone());
                ChoiceView {
                    value,
                    name: clip(&mode.name, MAX_LABEL),
                    description: description(mode.description.as_deref()),
                }
            })
        })
        .collect();
    (!modes_list.is_empty()).then(|| ModesView {
        current_mode_id: clip(&modes.current_mode_id.to_string(), MAX_LABEL),
        modes: modes_list,
    })
}

impl SessionConfig {
    /// What `session/new` declared.
    pub fn new(options: Option<&[SessionConfigOption]>, modes: Option<&SessionModeState>) -> Self {
        let options = options.map(options_view).unwrap_or_default();
        let modes = if options.is_empty() {
            modes.and_then(modes_view)
        } else {
            None
        };
        Self { options, modes }
    }

    /// Takes the agent's full option state, as sent with
    /// `config_option_update` or returned by `session/set_config_option`.
    pub fn replace_options(&mut self, options: &[SessionConfigOption]) {
        self.options = options_view(options);
        if !self.options.is_empty() {
            self.modes = None;
        }
    }

    /// Records the agent's current legacy mode. Returns whether anything changed.
    pub fn set_mode(&mut self, mode_id: &str) -> bool {
        match &mut self.modes {
            Some(modes) if modes.current_mode_id != mode_id => {
                modes.current_mode_id = clip(mode_id, MAX_LABEL);
                true
            }
            _ => false,
        }
    }

    /// Whether the agent offered `change`, and if so whether it is already set.
    pub fn check(&self, change: &ConfigChange) -> Result<bool, RuntimeError> {
        let option = |id: &str| self.options.iter().find(|option| option.id == id);
        match change {
            ConfigChange::Select { config_id, value } => match option(config_id).map(|o| &o.kind) {
                Some(ConfigKindView::Select {
                    current_value,
                    groups,
                }) if groups
                    .iter()
                    .flat_map(|group| &group.choices)
                    .any(|choice| &choice.value == value) =>
                {
                    Ok(current_value == value)
                }
                _ => Err(RuntimeError::InvalidArguments),
            },
            ConfigChange::Boolean { config_id, value } => {
                match option(config_id).map(|o| &o.kind) {
                    Some(ConfigKindView::Boolean { current_value }) => Ok(current_value == value),
                    _ => Err(RuntimeError::InvalidArguments),
                }
            }
            ConfigChange::Mode { mode_id } => match &self.modes {
                Some(modes) if modes.modes.iter().any(|mode| &mode.value == mode_id) => {
                    Ok(&modes.current_mode_id == mode_id)
                }
                _ => Err(RuntimeError::InvalidArguments),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn options_keep_the_agents_order_and_drop_what_cannot_be_sent_back() {
        let options: Vec<SessionConfigOption> = parse(json!([
            {"id":"model","name":"Model","category":"model","type":"select","currentValue":"a/1",
                "options":[{"group":"a","name":"Provider A","options":[
                    {"value":"a/1","name":"One"},{"value":"a/1","name":"Duplicate"},{"value":"","name":"Empty"}]},
                    {"group":"b","name":"Provider B","options":[{"value":"b/2","name":"Two","description":"Fast"}]}]},
            {"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"high",
                "options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]},
            {"id":"empty","name":"Nothing to pick","type":"select","currentValue":"x","options":[]},
            {"id":"model","name":"Shadowed","type":"boolean","currentValue":true},
            {"id":"fast","name":"Fast mode","category":"_vendor","type":"boolean","currentValue":false}
        ]));
        let modes: SessionModeState = parse(json!({"currentModeId":"build",
            "availableModes":[{"id":"build","name":"Build"}]}));
        let config = SessionConfig::new(Some(&options), Some(&modes));
        let ids: Vec<_> = config
            .options
            .iter()
            .map(|option| option.id.as_str())
            .collect();
        assert_eq!(ids, ["model", "effort", "fast"]);
        assert_eq!(config.modes, None, "config options replace legacy modes");
        assert_eq!(
            serde_json::to_value(&config.options[0]).unwrap(),
            json!({"id":"model","name":"Model","description":null,"category":"model","type":"select",
                "current_value":"a/1","groups":[
                    {"name":"Provider A","choices":[{"value":"a/1","name":"One","description":null}]},
                    {"name":"Provider B","choices":[{"value":"b/2","name":"Two","description":"Fast"}]}]})
        );
        assert_eq!(config.options[2].category.as_deref(), Some("_vendor"));

        let select = |config_id: &str, value: &str| ConfigChange::Select {
            config_id: config_id.into(),
            value: value.into(),
        };
        assert_eq!(config.check(&select("model", "b/2")), Ok(false));
        assert_eq!(config.check(&select("model", "a/1")), Ok(true));
        assert_eq!(
            config.check(&select("model", "c/3")),
            Err(RuntimeError::InvalidArguments)
        );
        assert_eq!(
            config.check(&select("fast", "on")),
            Err(RuntimeError::InvalidArguments)
        );
        let toggle = ConfigChange::Boolean {
            config_id: "fast".into(),
            value: true,
        };
        assert_eq!(config.check(&toggle), Ok(false));
        let mode = ConfigChange::Mode {
            mode_id: "build".into(),
        };
        assert_eq!(config.check(&mode), Err(RuntimeError::InvalidArguments));
    }

    #[test]
    fn legacy_modes_are_used_only_without_options() {
        let modes: SessionModeState = parse(json!({"currentModeId":"ask",
            "availableModes":[{"id":"ask","name":"Ask"},{"id":"code","name":"Code","description":"Edits files"}]}));
        let mut config = SessionConfig::new(None, Some(&modes));
        assert_eq!(config.modes.as_ref().unwrap().modes.len(), 2);
        let change = |mode_id: &str| ConfigChange::Mode {
            mode_id: mode_id.into(),
        };
        assert_eq!(config.check(&change("code")), Ok(false));
        assert_eq!(
            config.check(&change("yolo")),
            Err(RuntimeError::InvalidArguments)
        );
        assert!(config.set_mode("code"));
        assert!(!config.set_mode("code"));
        assert_eq!(config.check(&change("code")), Ok(true));

        let options: Vec<SessionConfigOption> = parse(json!([
            {"id":"mode","name":"Mode","type":"select","currentValue":"ask",
                "options":[{"value":"ask","name":"Ask"}]}]));
        config.replace_options(&options);
        assert_eq!(config.modes, None);
        assert_eq!(
            SessionConfig::new(Some(&[]), Some(&modes))
                .modes
                .unwrap()
                .current_mode_id,
            "ask"
        );
    }
}
