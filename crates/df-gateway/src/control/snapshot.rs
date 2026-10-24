//! Builds the policy snapshot from the database: active sandboxes and their effective
//! `http-host` allowlists.

use df_db::{ActiveRun, DbError, HostEntry, PgPool};
use df_proto::gateway::v1::{HostRule, PolicyUpdate, SandboxEntry, Snapshot};

/// The next policy version and the snapshot of the database as of now.
///
/// The version comes first, so an update never has a higher version than data newer than it.
pub async fn build(pool: &PgPool) -> Result<PolicyUpdate, DbError> {
    let version = df_db::next_policy_version(pool).await?;
    let runs = df_db::active_runs(pool).await?;
    let entries = df_db::http_host_entries(pool).await?;
    Ok(assemble(version, &runs, &entries))
}

/// A run's effective allowlist is the union of the instance entries and the entries of its
/// project.
pub fn assemble(version: u64, runs: &[ActiveRun], entries: &[HostEntry]) -> PolicyUpdate {
    let sandboxes = runs
        .iter()
        .map(|run| SandboxEntry {
            pod_ip: run.pod_ip.clone(),
            run_id: run.run_id.to_string(),
            task_key: run.task_key.clone(),
            project_key: run.project_key.clone(),
            persona_id: run.persona_id.to_string(),
            hosts: entries
                .iter()
                .filter(|entry| entry.project_id.is_none_or(|id| id == run.project_id))
                .map(|entry| HostRule {
                    pattern: entry.host.clone(),
                    port: u32::from(entry.port),
                    scope: if entry.project_id.is_some() {
                        "project"
                    } else {
                        "instance"
                    }
                    .into(),
                    entry_id: entry.id.to_string(),
                })
                .collect(),
        })
        .collect();
    PolicyUpdate {
        version,
        snapshot: Some(Snapshot { sandboxes }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn run(project: Uuid, key: &str) -> ActiveRun {
        ActiveRun {
            run_id: Uuid::now_v7(),
            pod_ip: "10.0.0.1".into(),
            task_key: format!("{key}-1"),
            project_id: project,
            project_key: key.into(),
            persona_id: Uuid::now_v7(),
        }
    }

    fn entry(project: Option<Uuid>, host: &str) -> HostEntry {
        HostEntry {
            id: Uuid::now_v7(),
            project_id: project,
            host: host.into(),
            port: 443,
        }
    }

    #[test]
    fn effective_allowlist_is_the_union_of_instance_and_own_project_entries() {
        let (df, other) = (Uuid::now_v7(), Uuid::now_v7());
        let update = assemble(
            9,
            &[run(df, "DF"), run(other, "WEB")],
            &[
                entry(None, "api.anthropic.com"),
                entry(Some(df), "*.crates.io"),
                entry(Some(other), "pypi.org"),
            ],
        );
        assert_eq!(update.version, 9);
        let sandboxes = update.snapshot.unwrap().sandboxes;
        let hosts = |sandbox: &SandboxEntry| -> Vec<(String, String)> {
            sandbox
                .hosts
                .iter()
                .map(|h| (h.pattern.clone(), h.scope.clone()))
                .collect()
        };
        assert_eq!(
            hosts(&sandboxes[0]),
            [
                ("api.anthropic.com".to_string(), "instance".to_string()),
                ("*.crates.io".to_string(), "project".to_string())
            ]
        );
        assert_eq!(
            hosts(&sandboxes[1]),
            [
                ("api.anthropic.com".to_string(), "instance".to_string()),
                ("pypi.org".to_string(), "project".to_string())
            ]
        );
        assert_eq!(sandboxes[0].task_key, "DF-1");
        assert_eq!(sandboxes[0].project_key, "DF");
    }

    #[test]
    fn no_runs_is_an_empty_snapshot() {
        let update = assemble(1, &[], &[entry(None, "x.com")]);
        assert!(update.snapshot.unwrap().sandboxes.is_empty());
    }
}
