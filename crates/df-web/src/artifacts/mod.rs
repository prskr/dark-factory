//! Artifacts in the web UI: cards, the rendered view, the split view and the artifact page
//! (`specs/artifact-ui/spec.md`, design D8, D9). The task page uses them now, the chat UI later.

mod routes;
mod view;

pub(crate) use routes::{load_cards, routes};
pub(crate) use view::{
    ArtifactCard, artifact_row, details_heading, split_args, split_pane, tab_panel,
};

#[cfg(test)]
mod tests;
