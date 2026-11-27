use serde_json::{Value, json};

use super::*;

fn raw(value: Value) -> RawQuestion {
    serde_json::from_value(value).expect("raw question")
}

fn spec(value: Value) -> QuestionSpec {
    QuestionSpec::parse(raw(value)).expect("valid question")
}

fn two_options() -> Value {
    json!([{"id": "pg16", "label": "PostgreSQL 16"}, {"id": "pg17", "label": "PostgreSQL 17"}])
}

fn single() -> QuestionSpec {
    spec(json!({"text": "Which?", "kind": "single_choice", "options": two_options()}))
}

fn multi(min: Option<u8>, max: Option<u8>, allow_other: bool) -> QuestionSpec {
    spec(json!({
        "text": "Which?", "kind": "multi_choice", "allow_other": allow_other,
        "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}, {"id": "c", "label": "C"}],
        "min_selected": min, "max_selected": max,
    }))
}

fn text() -> QuestionSpec {
    spec(json!({"text": "Why?", "kind": "text"}))
}

fn parse_error(value: Value) -> SpecError {
    QuestionSpec::parse(raw(value)).expect_err("rejected question")
}

fn choice(selected: &[&str], other: Option<&str>) -> QuestionAnswer {
    QuestionAnswer::Choice {
        selected: selected.iter().map(ToString::to_string).collect(),
        other: other.map(ToString::to_string),
    }
}

fn rejected(spec: &QuestionSpec, answer: QuestionAnswer) -> (AnswerField, String) {
    let error = spec.check(answer).expect_err("rejected answer");
    (error.field, error.message)
}

#[test]
fn each_kind_parses_with_its_defaults() {
    let single = single();
    assert_eq!(single.kind, QuestionKind::SingleChoice);
    assert!(single.allow_other);
    assert_eq!((single.min_selected, single.max_selected), (None, None));

    let multi = multi(None, None, true);
    assert_eq!(multi.kind, QuestionKind::MultiChoice);
    assert_eq!((multi.min_selected, multi.max_selected), (Some(1), None));

    let text = text();
    assert_eq!(text.kind, QuestionKind::Text);
    assert!(text.options.is_empty());
    assert!(!text.allow_other);
}

#[test]
fn text_is_trimmed_and_bounded() {
    let spec = spec(json!({"text": "  Why?  ", "kind": "text"}));
    assert_eq!(spec.text, "Why?");

    for bad in ["   ".to_string(), "x".repeat(4001)] {
        let error = parse_error(json!({"text": bad, "kind": "text"}));
        assert_eq!(error.field, "text");
    }
}

#[test]
fn neutral_arguments_of_another_kind_are_ignored() {
    let spec = spec(json!({
        "text": "Why?", "kind": "text", "options": [], "allow_other": false,
        "min_selected": null, "max_selected": null,
    }));
    assert_eq!(spec.kind, QuestionKind::Text);

    let single = QuestionSpec::parse(raw(json!({
        "text": "Which?", "kind": "single_choice", "options": two_options(),
        "min_selected": null, "max_selected": null,
    })));
    assert!(single.is_ok());
}

#[test]
fn real_arguments_of_another_kind_are_rejected_naming_the_argument() {
    let cases = [
        (
            json!({"text": "Q", "kind": "text", "options": two_options()}),
            "options",
        ),
        (
            json!({"text": "Q", "kind": "text", "min_selected": 1}),
            "min_selected",
        ),
        (
            json!({"text": "Q", "kind": "text", "max_selected": 2}),
            "max_selected",
        ),
        (
            json!({"text": "Q", "kind": "single_choice", "options": two_options(), "min_selected": 2}),
            "min_selected",
        ),
        (
            json!({"text": "Q", "kind": "single_choice", "options": two_options(), "max_selected": 2}),
            "max_selected",
        ),
    ];
    for (args, field) in cases {
        assert_eq!(parse_error(args.clone()).field, field, "{args}");
    }
}

#[test]
fn a_missing_or_unknown_kind_is_rejected_naming_kind_and_options() {
    let missing = parse_error(json!({"text": "Q"}));
    assert_eq!(missing.field, "kind");
    assert!(missing.to_string().contains("options"), "{missing}");

    let unknown = parse_error(json!({"text": "Q", "kind": "number"}));
    assert_eq!(unknown.field, "kind");
}

#[test]
fn the_option_count_is_two_to_ten() {
    let one = json!([{"id": "a", "label": "A"}]);
    assert_eq!(
        parse_error(json!({"text": "Q", "kind": "single_choice", "options": one})).field,
        "options"
    );
    assert_eq!(
        parse_error(json!({"text": "Q", "kind": "multi_choice"})).field,
        "options"
    );
    let eleven: Vec<Value> = (0..11)
        .map(|i| json!({"id": format!("o{i}"), "label": format!("O{i}")}))
        .collect();
    assert_eq!(
        parse_error(json!({"text": "Q", "kind": "single_choice", "options": eleven})).field,
        "options"
    );
    let ten: Vec<Value> = (0..10)
        .map(|i| json!({"id": format!("o{i}"), "label": format!("O{i}")}))
        .collect();
    assert!(
        QuestionSpec::parse(raw(
            json!({"text": "Q", "kind": "single_choice", "options": ten})
        ))
        .is_ok()
    );
}

#[test]
fn option_ids_follow_the_pattern_and_are_unique() {
    for id in ["", "_a", "-a", "A", "a b", "a.b", &"x".repeat(41)] {
        let options = json!([{"id": id, "label": "A"}, {"id": "b", "label": "B"}]);
        let error = parse_error(json!({"text": "Q", "kind": "single_choice", "options": options}));
        assert_eq!(error.field, "options", "id {id:?}");
    }
    let ok = json!([{"id": "0a_b-c", "label": "A"}, {"id": &"x".repeat(40), "label": "B"}]);
    assert!(
        QuestionSpec::parse(raw(
            json!({"text": "Q", "kind": "single_choice", "options": ok})
        ))
        .is_ok()
    );

    let duplicate = json!([{"id": "api", "label": "A"}, {"id": "api", "label": "B"}]);
    let error = parse_error(json!({"text": "Q", "kind": "multi_choice", "options": duplicate}));
    assert_eq!(error.field, "options");
    assert!(error.message.contains("api"), "{error}");
}

#[test]
fn option_labels_are_one_line_bounded_and_unique_ignoring_case() {
    let bad_labels = [
        String::new(),
        "   ".to_string(),
        "two\nlines".to_string(),
        "two\rlines".to_string(),
        "x".repeat(201),
    ];
    for label in bad_labels {
        let options = json!([{"id": "a", "label": label}, {"id": "b", "label": "B"}]);
        let error = parse_error(json!({"text": "Q", "kind": "single_choice", "options": options}));
        assert_eq!(error.field, "options", "label {label:?}");
    }
    let duplicate = json!([{"id": "a", "label": "Yes"}, {"id": "b", "label": " yes "}]);
    let error = parse_error(json!({"text": "Q", "kind": "single_choice", "options": duplicate}));
    assert_eq!(error.field, "options");
}

#[test]
fn option_descriptions_are_one_line_and_bounded() {
    for description in ["two\nlines".to_string(), "x".repeat(301)] {
        let options = json!([{"id": "a", "label": "A", "description": description}, {"id": "b", "label": "B"}]);
        let error = parse_error(json!({"text": "Q", "kind": "single_choice", "options": options}));
        assert_eq!(error.field, "options");
    }
    let options = json!([{"id": "a", "label": "A", "description": " why "}, {"id": "b", "label": "B", "description": null}]);
    let spec = spec(json!({"text": "Q", "kind": "single_choice", "options": options}));
    assert_eq!(spec.options[0].description.as_deref(), Some("why"));
    assert_eq!(spec.options[1].description, None);
}

#[test]
fn limits_are_checked_against_the_reachable_count() {
    let cases = [
        (Some(3), Some(2), true, "min_selected"),
        (None, Some(0), true, "max_selected"),
        (Some(5), None, true, "min_selected"),
        (Some(4), None, false, "min_selected"),
        (None, Some(5), true, "max_selected"),
        (None, Some(4), false, "max_selected"),
    ];
    for (min, max, allow_other, field) in cases {
        let args = json!({
            "text": "Q", "kind": "multi_choice", "allow_other": allow_other,
            "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}, {"id": "c", "label": "C"}],
            "min_selected": min, "max_selected": max,
        });
        assert_eq!(
            parse_error(args).field,
            field,
            "{min:?} {max:?} {allow_other}"
        );
    }
    assert_eq!(multi(Some(0), Some(4), true).max_selected, Some(4));
}

#[test]
fn read_stored_reads_questions_the_rules_would_reject() {
    let payload = json!({
        "text": "Which?", "kind": "single_choice", "allow_other": true,
        "options": [{"id": "a1", "label": "Same\nline"}, {"id": "a2", "label": "same\nline"}],
    });
    let spec = QuestionSpec::read_stored(&payload);
    assert_eq!(spec.options.len(), 2);
    assert_eq!(spec.options[0].label, "Same\nline");
    assert!(spec.allow_other);

    let one = QuestionSpec::read_stored(&json!({
        "text": "T", "kind": "single_choice", "options": [{"id": "a1", "label": "Only"}],
    }));
    assert_eq!(one.options.len(), 1);
}

#[test]
fn a_spec_survives_its_payload() {
    for spec in [single(), multi(Some(1), Some(2), true), text()] {
        assert_eq!(QuestionSpec::read_stored(&spec.to_payload()), spec);
    }
    let payload = single().to_payload();
    assert_eq!(
        payload["options"][0],
        json!({"id": "pg16", "label": "PostgreSQL 16"})
    );
}

#[test]
fn valid_answers_are_accepted_and_normalized() {
    let spec = multi(None, None, true);
    let checked = spec.check(choice(&["c", "a"], Some("  more  "))).unwrap();
    assert_eq!(checked, choice(&["a", "c"], Some("more")));

    assert_eq!(
        single().check(choice(&["pg17"], None)).unwrap(),
        choice(&["pg17"], None)
    );
    assert_eq!(
        single().check(choice(&[], Some("15"))).unwrap(),
        choice(&[], Some("15"))
    );
    assert_eq!(
        text().check(QuestionAnswer::Text("  hi  ".into())).unwrap(),
        QuestionAnswer::Text("hi".into())
    );
}

#[test]
fn choice_answers_are_rejected_with_the_message_of_their_case() {
    let single = single();
    let selected = AnswerField::Selected;
    let other = AnswerField::Other;
    let listed = "Choose from the listed options.";

    assert_eq!(
        rejected(&single, choice(&["nope"], None)),
        (selected, listed.into())
    );
    assert_eq!(
        rejected(&multi(None, None, true), choice(&["a", "a"], None)),
        (selected, listed.into())
    );
    assert_eq!(
        rejected(&single, choice(&[], None)),
        (selected, "Choose one option.".into())
    );
    assert_eq!(
        rejected(&single, choice(&["pg16", "pg17"], None)),
        (selected, "Choose one option.".into())
    );
    assert_eq!(
        rejected(&single, choice(&["pg16"], Some("x"))),
        (selected, "Choose one option.".into())
    );
    assert_eq!(
        rejected(&multi(None, None, true), choice(&[], None)),
        (selected, "Choose at least one option.".into())
    );
    assert_eq!(
        rejected(&multi(Some(2), None, true), choice(&["a"], None)),
        (selected, "Choose at least 2 options.".into())
    );
    assert_eq!(
        rejected(&multi(Some(0), Some(1), true), choice(&["a", "b"], None)),
        (selected, "Choose at most one option.".into())
    );
    assert_eq!(
        rejected(&multi(None, Some(2), true), choice(&["a", "b", "c"], None)),
        (selected, "Choose at most 2 options.".into())
    );
    assert_eq!(
        rejected(&multi(Some(2), Some(2), true), choice(&["a"], None)),
        (selected, "Choose exactly 2 options.".into())
    );
    assert_eq!(
        rejected(&multi(Some(1), Some(1), true), choice(&[], None)),
        (selected, "Choose exactly one option.".into())
    );
    assert_eq!(
        rejected(&multi(None, None, true), choice(&["a"], Some("   "))),
        (other, "Write your Other answer.".into())
    );
    assert_eq!(
        rejected(
            &multi(None, None, true),
            choice(&["a"], Some(&"x".repeat(1001)))
        ),
        (other, "At most 1,000 characters.".into())
    );
}

#[test]
fn other_is_rejected_when_the_question_has_none() {
    let no_other = multi(Some(0), None, false);
    assert_eq!(
        rejected(&no_other, choice(&["a"], Some("x"))),
        (
            AnswerField::Other,
            "This question has no Other answer.".into()
        )
    );
}

#[test]
fn text_answers_are_rejected_with_the_message_of_their_case() {
    let text = text();
    assert_eq!(
        rejected(&text, QuestionAnswer::Text("   ".into())),
        (AnswerField::Text, "Write an answer.".into())
    );
    assert_eq!(
        rejected(&text, QuestionAnswer::Text("x".repeat(4001))),
        (AnswerField::Text, "At most 4,000 characters.".into())
    );
}

#[test]
fn an_answer_of_the_wrong_shape_does_not_fit() {
    assert_eq!(
        rejected(&single(), QuestionAnswer::Text("x".into())),
        (
            AnswerField::Selected,
            "This answer does not fit the question.".into()
        )
    );
    assert_eq!(
        rejected(&text(), choice(&["a"], None)),
        (
            AnswerField::Text,
            "This answer does not fit the question.".into()
        )
    );
}

#[test]
fn limit_hint_returns_each_row_of_the_table() {
    let cases = [
        (Some(0), None, "Choose any"),
        (Some(0), Some(2), "Choose up to 2"),
        (Some(1), None, "Choose one or more"),
        (None, None, "Choose one or more"),
        (Some(2), None, "Choose at least 2"),
        (Some(1), Some(3), "Choose 1 to 3"),
        (Some(1), Some(1), "Choose one"),
        (Some(2), Some(2), "Choose 2"),
    ];
    for (min, max, hint) in cases {
        let reachable = multi(min, max, true);
        // limits above the reachable count are rejected by `parse`; these all fit 4 choices
        assert_eq!(reachable.limit_hint(), hint, "{min:?} {max:?}");
    }
    assert_eq!(single().limit_hint(), "");
}

#[test]
fn the_character_limit_message_groups_thousands_for_any_limit() {
    for (limit, message) in [
        (999, "At most 999 characters."),
        (1_000, "At most 1,000 characters."),
        (4_000, "At most 4,000 characters."),
        (1_234_567, "At most 1,234,567 characters."),
    ] {
        assert_eq!(at_most_characters(limit), message);
    }
}

#[test]
fn an_answer_survives_its_json_form() {
    let answer = choice(&["api"], Some("also the CLI"));
    let value = serde_json::to_value(&answer).unwrap();
    assert_eq!(value, json!({"selected": ["api"], "other": "also the CLI"}));
    assert_eq!(
        serde_json::from_value::<QuestionAnswer>(value).unwrap(),
        answer
    );

    let none = choice(&["api"], None);
    assert_eq!(
        serde_json::to_value(&none).unwrap(),
        json!({"selected": ["api"], "other": null})
    );

    let text = QuestionAnswer::Text("hi".into());
    let value = serde_json::to_value(&text).unwrap();
    assert_eq!(value, json!({"text": "hi"}));
    assert_eq!(
        serde_json::from_value::<QuestionAnswer>(value).unwrap(),
        text
    );
}
