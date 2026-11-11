//! The settings page of a model provider, `/model-providers/{id}` (`model-providers`, "Model
//! provider settings page"): a back link, the provider's name, the tabs General and Models of
//! [`super::provider_form`] and one "Save model provider".
//!
//! Like the project page, a save swaps only the form and, out of band, the heading and the
//! status line of the header.

use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use df_core::CredentialRef;
use df_db::model_providers::ModelProviderRecord;
use maud::{Markup, html};
use serde::Deserialize;
use uuid::Uuid;

use super::{ProviderForm, provider_form};
use crate::WebState;
use crate::components::FormErrors;
use crate::config_pages::{FormTarget, SettingsBack, saved_response, settings_page_frame};
use crate::layout::{Nav, Shell, layout};
use crate::responses::{internal_error, not_found};

#[derive(Deserialize)]
pub(super) struct PageQuery {
    tab: Option<String>,
}

/// `GET /model-providers/{id}?tab=`: the page. An unknown `tab` opens General.
pub(super) async fn page(
    State(state): State<WebState>,
    shell: Shell,
    Path(id): Path<Uuid>,
    Query(query): Query<PageQuery>,
) -> Response {
    let (record, credentials) = match tokio::try_join!(
        df_db::model_providers::get_model_provider(&state.pool, id),
        df_db::catalog::list_credential_refs(&state.pool)
    ) {
        Ok((Some(record), credentials)) => (record, credentials),
        Ok((None, _)) => return not_found("The model provider"),
        Err(e) => return internal_error(&e),
    };
    let body = settings_page_frame(
        "provider-settings",
        SettingsBack {
            href: "/model-providers",
            label: "Model providers",
        },
        heading(&record),
        "Save model provider",
        form_of(
            &record,
            &credentials,
            query.tab.as_deref().unwrap_or_default(),
        ),
    );
    layout(&record.input.name, Nav::ModelProviderSettings, &shell, body).into_response()
}

/// The answer of a successful save: the form with the stored values and, out of band, the heading
/// (the name may have changed) and the "Model provider saved" status. The tab stays open.
pub(super) fn saved(
    record: &ModelProviderRecord,
    credentials: &[CredentialRef],
    tab: &str,
) -> Response {
    saved_response(
        form_of(record, credentials, tab),
        "provider-heading",
        heading(record),
        "Model provider saved",
    )
}

/// The provider form with the stored values, opened on `tab`.
fn form_of(record: &ModelProviderRecord, credentials: &[CredentialRef], tab: &str) -> Markup {
    provider_form(
        FormTarget::new("/model-providers", Some(record.id)),
        &ProviderForm::from_record(record),
        &FormErrors::default(),
        credentials,
        tab,
    )
}

fn heading(record: &ModelProviderRecord) -> Markup {
    html! {
        div id="provider-heading" class="flex min-w-0 items-center gap-3" {
            h1 class="min-w-0 truncate text-2xl font-semibold" { (record.input.name) }
        }
    }
}
