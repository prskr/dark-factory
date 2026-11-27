//! Question kinds, options and answers with their rules (`specs/agent-requests/spec.md`,
//! "Question kinds", "Question options", "Answer validation"). Pure rules shared by the
//! `ask_question` tool, the web UI and storage.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Longest question text and text answer, in characters.
pub const MAX_TEXT_CHARS: usize = 4000;
/// Longest Other answer, in characters.
pub const MAX_OTHER_CHARS: usize = 1000;
/// Fewest options of a choice question.
pub const MIN_OPTIONS: usize = 2;
/// Most options of a choice question.
pub const MAX_OPTIONS: usize = 10;
/// Longest option id, in characters.
pub const MAX_OPTION_ID_CHARS: usize = 40;
/// Longest option label, in characters.
pub const MAX_LABEL_CHARS: usize = 200;
/// Longest option description, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 300;
/// The `selected` value that stands for the Other answer in a form post.
pub const OTHER_VALUE: &str = "_other";

/// What kind of answer a question asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    SingleChoice,
    MultiChoice,
    Text,
}

impl QuestionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SingleChoice => "single_choice",
            Self::MultiChoice => "multi_choice",
            Self::Text => "text",
        }
    }

    /// The name shown on the question card ("Single choice").
    pub const fn label(self) -> &'static str {
        match self {
            Self::SingleChoice => "Single choice",
            Self::MultiChoice => "Multiple choice",
            Self::Text => "Text",
        }
    }

    pub const fn is_choice(self) -> bool {
        !matches!(self, Self::Text)
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "single_choice" => Some(Self::SingleChoice),
            "multi_choice" => Some(Self::MultiChoice),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

impl fmt::Display for QuestionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One option of a choice question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A validated question, in the form it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionSpec {
    pub text: String,
    pub kind: QuestionKind,
    pub options: Vec<QuestionOption>,
    pub allow_other: bool,
    /// Multiple choice only.
    pub min_selected: Option<u8>,
    /// Multiple choice only.
    pub max_selected: Option<u8>,
}

impl QuestionSpec {
    /// Checks the arguments of a new question against the rules and returns the normalized
    /// question.
    pub fn parse(mut raw: RawQuestion) -> Result<Self, SpecError> {
        let text = raw.text.trim().to_string();
        if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
            return Err(SpecError::new(
                "text",
                format!("the question must have 1 to {MAX_TEXT_CHARS} characters"),
            ));
        }
        let kind = raw.kind.as_deref().ok_or_else(|| {
            SpecError::new(
                "kind",
                "required: `single_choice` or `multi_choice` (both with `options`), or `text`",
            )
        })?;
        let kind = QuestionKind::parse(kind).ok_or_else(|| {
            SpecError::new("kind", "must be `single_choice`, `multi_choice` or `text`")
        })?;
        let options = raw.options.take().unwrap_or_default();
        if !kind.is_choice() {
            return Self::parse_text(text, &options, &raw);
        }
        let options = parse_options(kind, options)?;
        let allow_other = raw.allow_other.unwrap_or(true);
        let (min_selected, max_selected) = if kind == QuestionKind::MultiChoice {
            let reachable = options.len() + usize::from(allow_other);
            parse_limits(raw.min_selected, raw.max_selected, reachable)?
        } else {
            reject_limits(&raw)?;
            (None, None)
        };
        Ok(Self {
            text,
            kind,
            options,
            allow_other,
            min_selected,
            max_selected,
        })
    }

    fn parse_text(
        text: String,
        options: &[RawOption],
        raw: &RawQuestion,
    ) -> Result<Self, SpecError> {
        if !options.is_empty() {
            return Err(SpecError::new(
                "options",
                "a text question has no options; use a choice kind or drop `options`",
            ));
        }
        if raw.allow_other == Some(true) {
            return Err(SpecError::new(
                "allow_other",
                "a text question has no Other answer",
            ));
        }
        reject_limits(raw)?;
        Ok(Self::text_question(text))
    }

    /// A question asking for a free text answer.
    pub fn text_question(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: QuestionKind::Text,
            options: Vec::new(),
            allow_other: false,
            min_selected: None,
            max_selected: None,
        }
    }

    /// Reads a stored payload without applying the rules of [`Self::parse`]: migrated rows may
    /// have one option, duplicate or multi-line labels.
    pub fn read_stored(payload: &Value) -> Self {
        let text = payload
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .and_then(QuestionKind::parse)
            .unwrap_or(QuestionKind::Text);
        let options = payload
            .get("options")
            .and_then(Value::as_array)
            .map_or_default(|items| {
                items
                    .iter()
                    .filter_map(|item| serde_json::from_value(item.clone()).ok())
                    .collect()
            });
        let limit = |name: &str| {
            payload
                .get(name)
                .and_then(Value::as_u64)
                .and_then(|n| u8::try_from(n).ok())
        };
        Self {
            text,
            kind,
            options,
            allow_other: payload
                .get("allow_other")
                .and_then(Value::as_bool)
                .unwrap_or(kind.is_choice()),
            min_selected: limit("min_selected"),
            max_selected: limit("max_selected"),
        }
    }

    /// The stored form of the question: the `payload` of its request.
    pub fn to_payload(&self) -> Value {
        json!({
            "text": self.text,
            "kind": self.kind,
            "options": self.options,
            "allow_other": self.allow_other,
            "min_selected": self.min_selected,
            "max_selected": self.max_selected,
        })
    }

    /// Checks `answer` against the question and returns it normalized: trimmed, chosen options in
    /// the order of the question.
    pub fn check(&self, answer: QuestionAnswer) -> Result<QuestionAnswer, AnswerError> {
        match (self.kind, answer) {
            (QuestionKind::Text, QuestionAnswer::Text(text)) => check_text(&text),
            (
                QuestionKind::SingleChoice | QuestionKind::MultiChoice,
                QuestionAnswer::Choice { selected, other },
            ) => self.check_choice(selected, other),
            (QuestionKind::Text, QuestionAnswer::Choice { .. }) => Err(AnswerError::new(
                AnswerField::Text,
                "This answer does not fit the question.",
            )),
            (_, QuestionAnswer::Text(_)) => Err(AnswerError::new(
                AnswerField::Selected,
                "This answer does not fit the question.",
            )),
        }
    }

    fn check_choice(
        &self,
        selected: Vec<String>,
        other: Option<String>,
    ) -> Result<QuestionAnswer, AnswerError> {
        let mut seen = HashSet::new();
        let known = |id: &String| self.options.iter().any(|o| &o.id == id);
        if selected.iter().any(|id| !known(id) || !seen.insert(id)) {
            return Err(AnswerError::new(
                AnswerField::Selected,
                "Choose from the listed options.",
            ));
        }
        let count = selected.len() + usize::from(other.is_some());
        self.check_count(count)?;
        let other = match other {
            None => None,
            Some(_) if !self.allow_other => {
                return Err(AnswerError::new(
                    AnswerField::Other,
                    "This question has no Other answer.",
                ));
            }
            Some(text) => Some(check_other(&text)?),
        };
        let selected = self
            .options
            .iter()
            .filter(|o| selected.contains(&o.id))
            .map(|o| o.id.clone())
            .collect();
        Ok(QuestionAnswer::Choice { selected, other })
    }

    fn check_count(&self, count: usize) -> Result<(), AnswerError> {
        let fail = |message: String| Err(AnswerError::new(AnswerField::Selected, message));
        if self.kind == QuestionKind::SingleChoice {
            return if count == 1 {
                Ok(())
            } else {
                fail("Choose one option.".into())
            };
        }
        let min = usize::from(self.min_selected.unwrap_or(1));
        let max = self.max_selected.map(usize::from);
        if max == Some(min) && count != min {
            return fail(format!("Choose exactly {}.", options_phrase(min)));
        }
        if count < min {
            return fail(format!("Choose at least {}.", options_phrase(min)));
        }
        if let Some(max) = max.filter(|max| count > *max) {
            return fail(format!("Choose at most {}.", options_phrase(max)));
        }
        Ok(())
    }

    /// The hint above the options of a multiple-choice question; empty for other kinds.
    pub fn limit_hint(&self) -> String {
        if self.kind != QuestionKind::MultiChoice {
            return String::new();
        }
        let min = self.min_selected.unwrap_or(1);
        match (min, self.max_selected) {
            (0, None) => "Choose any".into(),
            (0, Some(max)) => format!("Choose up to {max}"),
            (1, None) => "Choose one or more".into(),
            (min, None) => format!("Choose at least {min}"),
            (1, Some(1)) => "Choose one".into(),
            (min, Some(max)) if min == max => format!("Choose {min}"),
            (min, Some(max)) => format!("Choose {min} to {max}"),
        }
    }

    /// The option with `id`.
    pub fn option(&self, id: &str) -> Option<&QuestionOption> {
        self.options.iter().find(|o| o.id == id)
    }
}

/// The arguments of `ask_question` as the agent sent them, before [`QuestionSpec::parse`].
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawQuestion {
    pub text: String,
    pub kind: Option<String>,
    pub options: Option<Vec<RawOption>>,
    pub allow_other: Option<bool>,
    pub min_selected: Option<i64>,
    pub max_selected: Option<i64>,
}

/// An option as the agent sent it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

/// A rule of the question the agent broke; the message names the argument.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {message}")]
pub struct SpecError {
    /// The argument at fault (`text`, `kind`, `options`, `min_selected`, ...).
    pub field: &'static str,
    pub message: String,
}

impl SpecError {
    fn new(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

/// The form field an answer error belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerField {
    Selected,
    Other,
    Text,
}

impl AnswerField {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Other => "other",
            Self::Text => "text",
        }
    }
}

/// Why an answer does not fit its question; the message is shown to the human.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct AnswerError {
    pub field: AnswerField,
    pub message: String,
}

impl AnswerError {
    fn new(field: AnswerField, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

/// The answer to a question. Its JSON form is the stored decision of the question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "AnswerRepr", into = "AnswerRepr")]
pub enum QuestionAnswer {
    /// `other` is `Some` exactly when Other was chosen, also with an empty text.
    Choice {
        selected: Vec<String>,
        other: Option<String>,
    },
    Text(String),
}

/// The JSON form of a [`QuestionAnswer`].
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum AnswerRepr {
    Choice {
        selected: Vec<String>,
        #[serde(default)]
        other: Option<String>,
    },
    Text {
        text: String,
    },
}

impl From<AnswerRepr> for QuestionAnswer {
    fn from(repr: AnswerRepr) -> Self {
        match repr {
            AnswerRepr::Choice { selected, other } => Self::Choice { selected, other },
            AnswerRepr::Text { text } => Self::Text(text),
        }
    }
}

impl From<QuestionAnswer> for AnswerRepr {
    fn from(answer: QuestionAnswer) -> Self {
        match answer {
            QuestionAnswer::Choice { selected, other } => Self::Choice { selected, other },
            QuestionAnswer::Text(text) => Self::Text { text },
        }
    }
}

/// "one option" for 1, "N options" otherwise.
fn options_phrase(n: usize) -> String {
    match n {
        1 => "one option".into(),
        n => format!("{n} options"),
    }
}

/// "At most 4,000 characters." (thousands separated by a comma).
fn at_most_characters(limit: usize) -> String {
    let digits = limit.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("At most {grouped} characters.")
}

fn check_text(text: &str) -> Result<QuestionAnswer, AnswerError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AnswerError::new(AnswerField::Text, "Write an answer."));
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(AnswerError::new(
            AnswerField::Text,
            at_most_characters(MAX_TEXT_CHARS),
        ));
    }
    Ok(QuestionAnswer::Text(text.to_string()))
}

fn check_other(text: &str) -> Result<String, AnswerError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AnswerError::new(
            AnswerField::Other,
            "Write your Other answer.",
        ));
    }
    if text.chars().count() > MAX_OTHER_CHARS {
        return Err(AnswerError::new(
            AnswerField::Other,
            at_most_characters(MAX_OTHER_CHARS),
        ));
    }
    Ok(text.to_string())
}

fn parse_options(
    kind: QuestionKind,
    raw: Vec<RawOption>,
) -> Result<Vec<QuestionOption>, SpecError> {
    if !(MIN_OPTIONS..=MAX_OPTIONS).contains(&raw.len()) {
        return Err(SpecError::new(
            "options",
            format!(
                "a {kind} question needs {MIN_OPTIONS} to {MAX_OPTIONS} options, got {}",
                raw.len()
            ),
        ));
    }
    let mut ids = HashSet::new();
    let mut labels = HashSet::new();
    let mut options = Vec::with_capacity(raw.len());
    for option in raw {
        let option = parse_option(option)?;
        if !ids.insert(option.id.clone()) {
            return Err(SpecError::new(
                "options",
                format!("option id `{}` is used twice", option.id),
            ));
        }
        if !labels.insert(option.label.to_lowercase()) {
            return Err(SpecError::new(
                "options",
                format!("option label `{}` is used twice", option.label),
            ));
        }
        options.push(option);
    }
    Ok(options)
}

fn parse_option(raw: RawOption) -> Result<QuestionOption, SpecError> {
    let id_ok = raw.id.chars().count() <= MAX_OPTION_ID_CHARS
        && raw.id.chars().enumerate().all(|(i, c)| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || (i > 0 && matches!(c, '-' | '_'))
        })
        && !raw.id.is_empty();
    if !id_ok {
        return Err(SpecError::new(
            "options",
            format!(
                "option id `{}` must be lowercase letters, digits, `-` and `_`, start with a letter or digit and have at most {MAX_OPTION_ID_CHARS} characters",
                raw.id
            ),
        ));
    }
    let label = raw.label.trim().to_string();
    if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS || is_multi_line(&label) {
        return Err(SpecError::new(
            "options",
            format!(
                "the label of option `{}` must be one line of 1 to {MAX_LABEL_CHARS} characters",
                raw.id
            ),
        ));
    }
    let description = raw
        .description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    if description
        .as_deref()
        .is_some_and(|d| d.chars().count() > MAX_DESCRIPTION_CHARS || is_multi_line(d))
    {
        return Err(SpecError::new(
            "options",
            format!(
                "the description of option `{}` must be one line of at most {MAX_DESCRIPTION_CHARS} characters",
                raw.id
            ),
        ));
    }
    Ok(QuestionOption {
        id: raw.id,
        label,
        description,
    })
}

fn is_multi_line(text: &str) -> bool {
    text.contains(['\n', '\r'])
}

/// `min_selected` and `max_selected` of a multiple-choice question whose `reachable` answers
/// are its options plus Other when allowed.
fn parse_limits(
    min: Option<i64>,
    max: Option<i64>,
    reachable: usize,
) -> Result<(Option<u8>, Option<u8>), SpecError> {
    let reachable = i64::try_from(reachable).unwrap_or(i64::MAX);
    let min = min.unwrap_or(1);
    if !(0..=reachable).contains(&min) {
        return Err(SpecError::new(
            "min_selected",
            format!("must be 0 to {reachable}, the options plus Other when allowed"),
        ));
    }
    if let Some(max) = max {
        if !(1..=reachable).contains(&max) {
            return Err(SpecError::new(
                "max_selected",
                format!("must be 1 to {reachable}, the options plus Other when allowed"),
            ));
        }
        if min > max {
            return Err(SpecError::new(
                "min_selected",
                "must not be greater than `max_selected`",
            ));
        }
    }
    let narrow = |n: i64| u8::try_from(n).unwrap_or(u8::MAX);
    Ok((Some(narrow(min)), max.map(narrow)))
}

/// A kind without limits ignores `null` limits and rejects real ones.
fn reject_limits(raw: &RawQuestion) -> Result<(), SpecError> {
    for (field, value) in [
        ("min_selected", raw.min_selected),
        ("max_selected", raw.max_selected),
    ] {
        if value.is_some() {
            return Err(SpecError::new(
                field,
                "only a multi_choice question has limits; drop it",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
