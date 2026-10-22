//! Small shared components: form fields, status badges, redirects.

use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};

/// Validation errors by field name; the empty name is a message for the whole form.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FormErrors(Vec<(&'static str, String)>);

impl FormErrors {
    pub fn add(&mut self, field: &'static str, message: impl Into<String>) {
        self.0.push((field, message.into()));
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, field: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(f, _)| *f == field)
            .map(|(_, m)| m.as_str())
    }

    /// Turns the collected errors into a result: `Ok(value)` if there are none.
    pub fn into_result<T>(self, value: T) -> Result<T, Self> {
        if self.is_empty() {
            Ok(value)
        } else {
            Err(self)
        }
    }
}

const INPUT_CLASS: &str = "min-h-11 w-full rounded border border-slate-700 bg-slate-900 px-3 py-2 text-base text-slate-100";

fn field_shell(
    label: &str,
    name: &str,
    errors: &FormErrors,
    hint: Option<&str>,
    control: Markup,
) -> Markup {
    html! {
        div class="space-y-1" {
            label for=(name) class="block text-sm font-medium text-slate-300" { (label) }
            (control)
            @if let Some(hint) = hint { p class="text-xs text-slate-500" { (hint) } }
            @if let Some(error) = errors.get(name) {
                p class="text-sm text-red-400" role="alert" { (error) }
            }
        }
    }
}

/// A single-line input.
pub fn text_field(
    label: &str,
    name: &str,
    value: &str,
    errors: &FormErrors,
    hint: Option<&str>,
) -> Markup {
    field_shell(
        label,
        name,
        errors,
        hint,
        html! {
            input id=(name) name=(name) type="text" value=(value) class=(INPUT_CLASS);
        },
    )
}

/// A multi-line input.
pub fn textarea_field(
    label: &str,
    name: &str,
    value: &str,
    rows: u8,
    errors: &FormErrors,
) -> Markup {
    field_shell(
        label,
        name,
        errors,
        None,
        html! {
            textarea id=(name) name=(name) rows=(rows) class=(INPUT_CLASS) { (value) }
        },
    )
}

/// A select with an optional empty choice.
pub fn select_field(
    label: &str,
    name: &str,
    options: &[(String, String)],
    selected: &str,
    empty_label: Option<&str>,
    errors: &FormErrors,
) -> Markup {
    field_shell(
        label,
        name,
        errors,
        None,
        html! {
            select id=(name) name=(name) class=(INPUT_CLASS) {
                @if let Some(empty) = empty_label { option value="" selected[selected.is_empty()] { (empty) } }
                @for (value, text) in options {
                    option value=(value) selected[value == selected] { (text) }
                }
            }
        },
    )
}

/// A form-wide error message, if any.
pub fn form_error(errors: &FormErrors) -> Markup {
    html! {
        @if let Some(error) = errors.get("") {
            p class="rounded border border-red-700 bg-red-950 px-3 py-2 text-sm text-red-200" role="alert" { (error) }
        }
    }
}

/// The submit button of a form.
pub fn submit_button(label: &str) -> Markup {
    html! {
        button type="submit" class="min-h-11 rounded bg-sky-600 px-4 py-2 font-medium text-white hover:bg-sky-500" { (label) }
    }
}

/// A coloured status label.
pub fn status_badge(status: &str) -> Markup {
    let class = match status {
        "running" => "bg-sky-900 text-sky-200",
        "completed" | "succeeded" => "bg-emerald-900 text-emerald-200",
        "failed" => "bg-red-900 text-red-200",
        "rate_limited" | "limit_reached" => "bg-amber-900 text-amber-200",
        _ => "bg-slate-800 text-slate-300",
    };
    html! { span class={"inline-block rounded px-2 py-0.5 text-xs font-medium " (class)} { (status.replace('_', " ")) } }
}

/// Response telling htmx to navigate to `location` (used after a successful form post).
pub fn htmx_redirect(location: &str) -> Response {
    let mut response = StatusCode::OK.into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert("HX-Redirect", value);
    }
    response
}

/// An HTML fragment that htmx swaps although the request failed validation (htmx 4 swaps
/// every status except 204 and 304).
pub fn invalid(fragment: Markup) -> Response {
    (StatusCode::UNPROCESSABLE_ENTITY, fragment).into_response()
}

/// A page-level 404.
pub fn not_found(what: &str) -> Response {
    crate::auth::error_page(
        StatusCode::NOT_FOUND,
        "Not found",
        &format!("{what} does not exist."),
    )
}

/// Maps an unexpected database error to an error page and logs it.
pub fn internal_error(error: &dyn std::fmt::Display) -> Response {
    tracing::error!(%error, "request failed");
    crate::auth::error_page(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Error",
        "Something went wrong. Try again.",
    )
}
