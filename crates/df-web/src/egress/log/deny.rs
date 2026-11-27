//! The "Deny always" action of the traffic page (`specs/egress-log`, "Deny from the log"): the
//! button of a row and the handler that creates the deny rule for exactly the row's host.

use axum::Extension;
use axum::extract::{Form, State};
use axum::http::StatusCode;
use axum::response::Response;
use df_core::host::validate_exact_host;
use df_db::DbError;
use df_db::catalog::Actor;
use maud::{Markup, html};
use serde::Deserialize;

use super::context::deny_rule_href;
use super::toast;
use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::{error_toast, toast_answer};
use crate::responses::internal_error;

/// The handler that creates the deny rule.
pub(in crate::egress) const DENY: &str = "/egress/log/deny";

/// A control that creates the deny rule for `host`; `class` and `content` are its look. It
/// answers with a toast and swaps nothing.
pub(super) fn button(host: &str, class: &str, content: Markup) -> Markup {
    html! {
        button type="button" hx-post=(DENY) hx-vals=(serde_json::json!({ "host": host }).to_string())
            hx-swap="none" aria-label={"Deny always " (host)} class=(class) { (content) }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(in crate::egress) struct DenyForm {
    host: String,
}

/// `POST /egress/log/deny`: creates the deny rule for the exact host, like the rule dialog
/// (validated, audited). A rule that exists already is as good as a new one.
pub(in crate::egress) async fn create(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<DenyForm>,
) -> Response {
    let host = match validate_exact_host(&form.host) {
        Ok(host) => host,
        Err(message) => {
            return toast_answer(
                StatusCode::UNPROCESSABLE_ENTITY,
                error_toast("Rule not created", message),
            );
        }
    };
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    match df_db::egress::add_deny_rule(&state.pool, &host, actor).await {
        Ok(_) | Err(DbError::Conflict(_)) => toast_answer(StatusCode::OK, created_toast(&host)),
        Err(e) => internal_error(&e),
    }
}

fn created_toast(host: &str) -> Markup {
    toast::created(
        "Deny rule created",
        html! { span class="font-mono" { (host) } " is denied. No requests are raised for it." },
        &deny_rule_href(host),
    )
}
