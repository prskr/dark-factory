//! The MCP part of the policy snapshot: the servers enabled for the projects of active runs, and
//! the credentials they refer to (iter4-mcp-gateway design D7).

use std::collections::{HashMap, HashSet};

use df_core::mcp::McpCredentialScheme;
use df_core::{CredentialRef, SecretStore};
use df_db::mcp_servers::{McpServerRecord, enabled_servers_of_projects};
use df_db::{ActiveRun, DbError, PgPool};
use df_proto::gateway::v1 as pb;
use uuid::Uuid;

/// The servers enabled for each project that has an active run.
#[derive(Debug, Default)]
pub(super) struct McpData {
    by_project: HashMap<Uuid, Vec<McpServerRecord>>,
}

impl McpData {
    pub(super) async fn load(pool: &PgPool, runs: &[ActiveRun]) -> Result<Self, DbError> {
        let projects: Vec<Uuid> = runs.iter().map(|run| run.project_id).collect();
        Ok(Self {
            by_project: enabled_servers_of_projects(pool, &projects).await?,
        })
    }

    /// The ids of the servers enabled for the project of `run`, by name.
    pub(super) fn server_ids(&self, run: &ActiveRun) -> Vec<String> {
        self.by_project
            .get(&run.project_id)
            .into_iter()
            .flatten()
            .map(|server| server.id.to_string())
            .collect()
    }

    /// Every server enabled for at least one active run, once, by id.
    pub(super) fn servers(&self) -> HashMap<String, pb::McpServer> {
        self.by_project
            .values()
            .flatten()
            .map(|server| (server.id.to_string(), server_message(server)))
            .collect()
    }
}

fn server_message(server: &McpServerRecord) -> pb::McpServer {
    let input = &server.input;
    let scheme = match &input.scheme {
        McpCredentialScheme::Bearer => {
            pb::mcp_server::CredentialScheme::Bearer(pb::McpBearerScheme {})
        }
        McpCredentialScheme::Header { name, prefix } => {
            pb::mcp_server::CredentialScheme::Header(pb::McpHeaderScheme {
                name: name.clone(),
                prefix: prefix.clone(),
            })
        }
        McpCredentialScheme::Query { parameter } => {
            pb::mcp_server::CredentialScheme::Query(pb::McpQueryScheme {
                parameter: parameter.clone(),
            })
        }
    };
    pb::McpServer {
        id: server.id.to_string(),
        name: input.name.clone(),
        url: input.url.clone(),
        credential_ref: input.credential_ref_id.to_string(),
        credential_scheme: Some(scheme),
        private_network: input.private_network,
        config_version: u64::try_from(server.config_version).unwrap_or_default(),
    }
}

/// The credentials of `servers`, read once per credential reference. A credential that cannot be
/// read is left out with a warning (never its value); its servers stay, so the gateway answers
/// their calls with its own error.
pub(super) async fn credentials(
    secrets: &dyn SecretStore,
    servers: &HashMap<String, pb::McpServer>,
    references: &[CredentialRef],
) -> HashMap<String, pb::ProviderCredential> {
    let wanted: HashSet<&str> = servers
        .values()
        .map(|server| server.credential_ref.as_str())
        .collect();
    super::inference_routes::credentials_for(secrets, &wanted, references).await
}
