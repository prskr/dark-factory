//! The input and the answered view of a question card (`request-inbox` spec, "Answer input by
//! kind", "Answer errors on the card", "Answered question in history"; layout in
//! `openspec/changes/question-kinds/design-boards.md`).
//!
//! Option labels and descriptions are plain text (they sit inside form controls); the question
//! text and a text answer render as Markdown. Everything here may come from an agent.

use df_core::question::{MAX_OTHER_CHARS, OTHER_VALUE};
use df_core::{
    AnswerError, AnswerField, QuestionAnswer, QuestionKind, QuestionOption, QuestionSpec,
};
use df_db::requests::RequestRow;
use maud::{Markup, html};
use sha2::{Digest, Sha256};

use crate::components::forms::{FormErrors, INPUT_CLASS, MarkdownField, Preview, markdown_field};
use crate::components::icons::{Icon, icon};
use crate::components::{ButtonKind, action_class, short_time};
use crate::markdown;

const ROW_CLASS: &str = "flex cursor-pointer items-start gap-2.5 py-1.5";
const CONTROL_CLASS: &str = "mt-px size-5 shrink-0";

/// What a board member submitted for a question, to show it again beside an error.
#[derive(Debug, Default, Clone)]
pub(crate) struct Submitted {
    selected: Vec<String>,
    other_chosen: bool,
    other: String,
    text: String,
}

impl Submitted {
    /// Reads the repeated `selected` values and the `other` and `text` fields of the answer form.
    pub(crate) fn from_form(pairs: &[(String, String)]) -> Self {
        let mut submitted = Self::default();
        for (key, value) in pairs {
            match key.as_str() {
                "selected" if value == OTHER_VALUE => submitted.other_chosen = true,
                "selected" => submitted.selected.push(value.clone()),
                "other" => submitted.other.clone_from(value),
                "text" => submitted.text.clone_from(value),
                _ => {}
            }
        }
        submitted
    }

    /// The answer in the shape of the question's kind. An Other text counts only when Other was
    /// chosen, so text left in the always-visible input never rejects a valid choice.
    pub(crate) fn answer(&self, kind: QuestionKind) -> QuestionAnswer {
        match kind {
            QuestionKind::Text => QuestionAnswer::Text(self.text.clone()),
            QuestionKind::SingleChoice | QuestionKind::MultiChoice => QuestionAnswer::Choice {
                selected: self.selected.clone(),
                other: self.other_chosen.then(|| self.other.clone()),
            },
        }
    }
}

/// An answer the server rejected, with what was submitted.
#[derive(Debug, Clone)]
pub(crate) struct Rejection {
    pub submitted: Submitted,
    pub error: AnswerError,
}

/// The hash of the stored question, which does not change while it is pending: the form of an
/// unchanged question moves into a card that is sent again (`requestCards.ts`).
pub(crate) fn question_hash(row: &RequestRow) -> String {
    let digest = Sha256::digest(row.payload.to_string().as_bytes());
    hex::encode(digest.iter().take(8).copied().collect::<Vec<u8>>())
}

/// The answer form of a pending question; `rejection` is the last answer the server refused.
pub(crate) fn input(row: &RequestRow, id: &str, rejection: Option<&Rejection>) -> Markup {
    let question = row.question();
    let error_of = |field: AnswerField| {
        rejection
            .filter(|r| r.error.field == field)
            .map(|r| r.error.message.as_str())
    };
    let empty = Submitted::default();
    let submitted = rejection.map_or(&empty, |r| &r.submitted);
    html! {
        form hx-post={"/requests/" (row.id) "/answer"} hx-target={"#" (id)} hx-swap="outerHTML"
            data-question={(row.id) ":" (question_hash(row))} class="space-y-3" {
            @if question.kind == QuestionKind::Text {
                (text_input(id, submitted, error_of(AnswerField::Text)))
            } @else {
                (choice_input(id, &question, submitted, ChoiceErrors {
                    selected: error_of(AnswerField::Selected),
                    other: error_of(AnswerField::Other),
                }))
            }
            div class="lg:flex lg:justify-end" {
                button type="submit"
                    class={(action_class(ButtonKind::Primary)) " w-full lg:w-auto"} { "Answer" }
            }
        }
    }
}

fn text_input(id: &str, submitted: &Submitted, error: Option<&str>) -> Markup {
    let field_id = format!("{id}-text");
    let mut errors = FormErrors::default();
    if let Some(error) = error {
        errors.add(field_id.clone(), error);
    }
    markdown_field(
        &MarkdownField {
            label: "Your answer",
            name: &field_id,
            submit_name: Some("text"),
            value: &submitted.text,
            hint: None,
            rows: 4,
            preview: Preview::PlainTabs,
            label_hidden: false,
        },
        &errors,
    )
}

/// The messages of a refused choice answer: for the options and for the Other answer.
#[derive(Debug, Default, Clone, Copy)]
struct ChoiceErrors<'a> {
    selected: Option<&'a str>,
    other: Option<&'a str>,
}

/// The options as radio buttons or checkboxes, "Other" last.
fn choice_input(
    id: &str,
    question: &QuestionSpec,
    submitted: &Submitted,
    errors: ChoiceErrors<'_>,
) -> Markup {
    // Without an Other row its message (Other chosen on a question without Other) sits below the
    // options like the message of the options.
    let other_error = errors.other.filter(|_| question.allow_other);
    let selected_error = errors
        .selected
        .or(errors.other.filter(|_| !question.allow_other));
    let control = if question.kind == QuestionKind::MultiChoice {
        "checkbox"
    } else {
        "radio"
    };
    // One rhythm for all rows: 56 px when any option has a description, else 44 px (36 from 1024).
    let row_height = if question.options.iter().any(|o| o.description.is_some()) {
        "min-h-14"
    } else {
        "min-h-11 lg:min-h-9"
    };
    let (hint_id, error_id) = (format!("{id}-hint"), format!("{id}-error"));
    let hint = question.limit_hint();
    let describedby = [
        (!hint.is_empty()).then_some(hint_id.as_str()),
        selected_error.map(|_| error_id.as_str()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    html! {
        fieldset aria-labelledby={(id) "-question"} aria-describedby=[(!describedby.is_empty()).then_some(describedby)]
            aria-invalid=[selected_error.map(|_| "true")]
            x-data={"questionOther(" (serde_json::Value::from(OTHER_VALUE)) ")"} "x-on:change"="sync(true)" class="min-w-0 space-y-1" {
            @if !hint.is_empty() { p id=(hint_id) class="text-[13px] text-muted" { (hint) } }
            @for option in &question.options {
                (option_row(control, row_height, option, submitted.selected.contains(&option.id)))
            }
            @if question.allow_other {
                label class={(ROW_CLASS) " " (row_height)} {
                    input type=(control) name="selected" value=(OTHER_VALUE) checked[submitted.other_chosen]
                        class=(CONTROL_CLASS);
                    span class="block text-sm text-ink" { "Other" }
                }
                // Shown while "Other" is checked; without the component it is always shown.
                div ":hidden"="!open" class="pb-1 pl-[30px]" {
                    input type="text" id={(id) "-other"} name="other" maxlength=(MAX_OTHER_CHARS) autocomplete="off"
                        placeholder="Your Other answer" aria-label="Your Other answer" data-other-input
                        value=(submitted.other) aria-invalid=[other_error.map(|_| "true")]
                        aria-describedby=[other_error.map(|_| format!("{id}-other-error"))]
                        class={(INPUT_CLASS) " min-h-10 lg:min-h-10"};
                    @if let Some(error) = other_error {
                        p id={(id) "-other-error"} role="alert" class="pt-1 text-sm text-danger" { (error) }
                    }
                }
            }
            @if let Some(error) = selected_error { (field_error_with_id(&error_id, error)) }
        }
    }
}

fn option_row(control: &str, row_height: &str, option: &QuestionOption, checked: bool) -> Markup {
    html! {
        label class={(ROW_CLASS) " " (row_height)} {
            input type=(control) name="selected" value=(option.id) checked[checked] class=(CONTROL_CLASS);
            span class="min-w-0" {
                span class="block break-words text-sm text-ink" { (option.label) }
                @if let Some(description) = &option.description {
                    span class="block break-words text-[13px] text-muted" { (description) }
                }
            }
        }
    }
}

fn field_error_with_id(id: &str, message: &str) -> Markup {
    html! { p id=(id) role="alert" class="pt-1 text-sm text-danger" { (message) } }
}

/// The answer of an answered question: every option with the chosen ones marked, or the answer
/// rendered as Markdown, then who answered and when.
pub(crate) fn answered(row: &RequestRow) -> Markup {
    let question = row.question();
    let by = match &row.decided_by {
        Some(df_core::Principal::Human { display_name, .. }) => format!(" by {display_name}"),
        _ => String::new(),
    };
    let when = row.decided_at.map_or_default(short_time);
    html! {
        @match row.answer() {
            Some(QuestionAnswer::Text(text)) => {
                div class="md-body md-request min-w-0 break-words rounded-lg bg-panel px-4 py-3 text-sm [&_p]:my-1" {
                    (markdown::render(&text))
                }
            }
            Some(QuestionAnswer::Choice { selected, other }) => {
                @let glyph = if question.kind == QuestionKind::MultiChoice { Icon::SquareCheck } else { Icon::CircleCheck };
                ul class="space-y-2 text-sm" {
                    @for option in &question.options {
                        (answered_option(glyph, &option.label, selected.contains(&option.id)))
                    }
                    @if let Some(other) = other {
                        (answered_option(glyph, &format!("Other: {other}"), true))
                    }
                }
            }
            None => {}
        }
        p class="text-sm text-muted" { "Answered" (by) @if !when.is_empty() { " · " (when) } }
    }
}

fn answered_option(glyph: Icon, label: &str, chosen: bool) -> Markup {
    html! {
        li class={"flex items-start gap-2 " (if chosen { "text-ink" } else { "text-muted" })} {
            @if chosen {
                span class="mt-0.5 shrink-0 text-accent-text [&_svg]:size-4" { (icon(glyph)) }
            } @else {
                span class="mt-0.5 size-4 shrink-0" aria-hidden="true" {}
            }
            span class="min-w-0 break-words" {
                (label)
                @if chosen { span class="sr-only" { "chosen" } }
            }
        }
    }
}
