//! Builds the policy snapshot from the database: active sandboxes, their effective
//! `http-host` allowlists, their Git routes, the forge credentials of those routes, the push
//! policies of their projects, the push check exceptions of their tasks, their inference routes
//! and the model provider credentials of those routes.

use std::collections::{HashMap, HashSet};

use df_core::SecretStore;
use df_core::TaskKey;
use df_core::config::{AgentConfig, NetworkConfig};
use df_db::model_providers::{self, RunModelProvider};
use df_db::push_checks::{self, CheckMode, PushPolicy};
use df_db::{ActiveRun, DbError, HostEntry, PgPool};
use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::{
    ForgeCredential, GitRoute, HostRule, PolicyUpdate, SandboxEntry, Snapshot,
};
use secrecy::ExposeSecret;
use uuid::Uuid;

use super::inference_routes;

/// Instance settings of the Git routes: the same values the run's `Assign` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSettings {
    /// User name sent with the forge token (`agent.gitUsername`).
    pub username: String,
    pub commit_name: String,
    pub commit_email: String,
    /// Routes for `http://` repositories too (`network.allowPlainHttp`).
    pub allow_plain_http: bool,
}

impl GitSettings {
    pub fn from_config(agent: &AgentConfig, network: &NetworkConfig) -> Self {
        Self {
            username: agent.git_username.clone(),
            commit_name: agent.commit_identity.name.clone(),
            commit_email: agent.commit_identity.email.clone(),
            allow_plain_http: network.allow_plain_http,
        }
    }
}

/// The next policy version and the snapshot of the database and the forge secrets as of now.
///
/// The version comes first, so an update never has a higher version than data newer than it.
pub async fn build(
    pool: &PgPool,
    secrets: &dyn SecretStore,
    git: &GitSettings,
) -> Result<PolicyUpdate, DbError> {
    let version = df_db::next_policy_version(pool).await?;
    let runs = df_db::active_runs(pool).await?;
    let entries = df_db::http_host_entries(pool).await?;
    let push = PushData::load(pool, &runs).await?;
    let run_ids: Vec<Uuid> = runs.iter().map(|run| run.run_id).collect();
    let providers = model_providers::run_model_providers(pool, &run_ids).await?;
    let mut update = assemble(version, &runs, &entries, git, &push, &providers);
    if let Some(snapshot) = update.snapshot.as_mut() {
        snapshot.forge_credentials =
            forge_credentials(secrets, &runs, &snapshot.sandboxes, &git.username).await;
        let references = df_db::catalog::list_credential_refs(pool).await?;
        snapshot.provider_credentials =
            inference_routes::credentials(secrets, &snapshot.sandboxes, &references).await;
    }
    Ok(update)
}

/// The stored push policies (by project id) and exceptions (by task id) of the active runs.
#[derive(Debug, Default)]
struct PushData {
    policies: HashMap<Uuid, PushPolicy>,
    exceptions: HashMap<Uuid, Vec<String>>,
}

impl PushData {
    async fn load(pool: &PgPool, runs: &[ActiveRun]) -> Result<Self, DbError> {
        let projects: Vec<Uuid> = runs.iter().map(|run| run.project_id).collect();
        let tasks: Vec<Uuid> = runs.iter().map(|run| run.task_id).collect();
        Ok(Self {
            policies: push_checks::push_policies(pool, &projects).await?,
            exceptions: push_checks::push_check_exceptions(pool, &tasks).await?,
        })
    }
}

/// A run's effective allowlist is the union of the instance entries, the entries of its project
/// and the entries of its task. Every project of a run gets a push policy, the defaults without a
/// stored one. The forge and provider credentials are left empty.
fn assemble(
    version: u64,
    runs: &[ActiveRun],
    entries: &[HostEntry],
    git: &GitSettings,
    push: &PushData,
    providers: &HashMap<Uuid, RunModelProvider>,
) -> PolicyUpdate {
    let sandboxes = runs
        .iter()
        .map(|run| SandboxEntry {
            pod_ip: run.pod_ip.clone(),
            run_id: run.run_id.to_string(),
            task_key: run.task_key.clone(),
            // Never negative (CHECK number >= 1); 0 means unknown.
            run_number: u32::try_from(run.run_number).unwrap_or(0),
            project_key: run.project_key.clone(),
            persona_id: run.persona_id.to_string(),
            hosts: entries
                .iter()
                .filter(|entry| entry.scope.applies_to(run.task_id, run.project_id))
                .map(|entry| HostRule {
                    pattern: entry.host.clone(),
                    port: u32::from(entry.port),
                    scope: entry.scope.as_str().into(),
                    entry_id: entry.id.to_string(),
                })
                .collect(),
            git: git_route(run, git),
            exceptions: push
                .exceptions
                .get(&run.task_id)
                .cloned()
                .unwrap_or_default(),
            inference: providers
                .get(&run.run_id)
                .and_then(|found| inference_routes::route(run, found)),
        })
        .collect();
    let push_policies = runs
        .iter()
        .map(|run| {
            let policy = push.policies.get(&run.project_id).cloned();
            (
                run.project_key.clone(),
                push_policy(policy.unwrap_or_default()),
            )
        })
        .collect();
    PolicyUpdate {
        version,
        snapshot: Some(Snapshot {
            sandboxes,
            forge_credentials: HashMap::new(),
            push_policies,
            provider_credentials: HashMap::new(),
        }),
    }
}

fn push_policy(policy: PushPolicy) -> pb::PushPolicy {
    let mode = |mode: CheckMode| -> i32 {
        match mode {
            CheckMode::Reject => pb::CheckMode::Reject,
            CheckMode::Approval => pb::CheckMode::Approval,
        }
        .into()
    };
    pb::PushPolicy {
        protected_globs: policy.protected_globs,
        protected_mode: mode(policy.protected_mode),
        hidden_globs: policy.hidden_globs,
        secret_mode: mode(policy.secret_mode),
        size_mode: mode(policy.size_mode),
        max_blob_bytes: policy.max_blob_bytes,
        linear_history: policy.linear_history,
    }
}

/// The Git route of `run`: its project's repository, the base branch and the branches the run may
/// push, as full ref names. `None` (with a warning) unless the URL is `https://`, or `http://`
/// with `network.allowPlainHttp`.
/// `credential_ref` is the id of the project's forge credential reference, empty without one.
fn git_route(run: &ActiveRun, git: &GitSettings) -> Option<GitRoute> {
    let plain_http_allowed = git.allow_plain_http && has_scheme(&run.repo_url, "http://");
    if !has_scheme(&run.repo_url, "https://") && !plain_http_allowed {
        tracing::warn!(run = %run.run_id, project = %run.project_key, "no Git route: the repository URL is not https");
        return None;
    }
    let key = match run.task_key.parse::<TaskKey>() {
        Ok(key) => key,
        Err(error) => {
            tracing::warn!(run = %run.run_id, %error, "no Git route: invalid task key");
            return None;
        }
    };
    Some(GitRoute {
        upstream_url: run.repo_url.clone(),
        base_branch: format!("refs/heads/{}", run.base_branch),
        task_branch: format!("refs/heads/{}", key.task_branch()),
        wip_branch: format!("refs/heads/{}", key.wip_branch()),
        credential_ref: run
            .forge_credential
            .as_ref()
            .map(|reference| reference.id.to_string())
            .unwrap_or_default(),
        commit_name: git.commit_name.clone(),
        commit_email: git.commit_email.clone(),
    })
}

fn has_scheme(url: &str, scheme: &str) -> bool {
    url.get(..scheme.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
}

/// The forge credentials referenced by the built Git routes, read once per credential reference
/// and keyed by its id. A credential that cannot be read is left out with a warning (never its
/// value); its routes stay, so the Git proxy answers their requests with its own error.
async fn forge_credentials(
    secrets: &dyn SecretStore,
    runs: &[ActiveRun],
    sandboxes: &[SandboxEntry],
    username: &str,
) -> HashMap<String, ForgeCredential> {
    let wanted: HashSet<&str> = sandboxes
        .iter()
        .filter_map(|sandbox| sandbox.git.as_ref())
        .map(|route| route.credential_ref.as_str())
        .filter(|reference| !reference.is_empty())
        .collect();
    let mut credentials = HashMap::new();
    let mut seen = HashSet::new();
    let references = runs.iter().filter_map(|run| run.forge_credential.as_ref());
    for reference in references {
        let key = reference.id.to_string();
        if !wanted.contains(key.as_str()) || !seen.insert(key.clone()) {
            continue;
        }
        match secrets.get(&reference.secret).await {
            Ok(token) => {
                credentials.insert(
                    key,
                    ForgeCredential {
                        username: username.to_string(),
                        password: token.expose_secret().to_string(),
                    },
                );
            }
            Err(error) => {
                tracing::warn!(%error, credential = %reference.name, "forge credential unavailable; Git route without credential");
            }
        }
    }
    credentials
}

#[cfg(test)]
mod tests {
    use super::*;
    use df_db::HostScope;
    use uuid::Uuid;

    fn run(project: Uuid, key: &str) -> ActiveRun {
        ActiveRun {
            run_id: Uuid::now_v7(),
            pod_ip: "10.0.0.1".into(),
            task_id: Uuid::now_v7(),
            task_key: format!("{key}-1"),
            run_number: 2,
            project_id: project,
            project_key: key.into(),
            persona_id: Uuid::now_v7(),
            repo_url: "https://example.test/r.git".into(),
            base_branch: "main".into(),
            forge_credential: None,
        }
    }

    fn settings() -> GitSettings {
        GitSettings {
            username: "dark-factory".into(),
            commit_name: "Dark Factory".into(),
            commit_email: "factory@example.test".into(),
            allow_plain_http: false,
        }
    }

    fn assemble(version: u64, runs: &[ActiveRun], entries: &[HostEntry]) -> PolicyUpdate {
        super::assemble(
            version,
            runs,
            entries,
            &settings(),
            &PushData::default(),
            &HashMap::new(),
        )
    }

    fn entry(scope: HostScope, host: &str) -> HostEntry {
        HostEntry {
            id: Uuid::now_v7(),
            scope,
            host: host.into(),
            port: 443,
        }
    }

    #[test]
    fn effective_allowlist_is_the_union_of_instance_own_project_and_own_task_entries() {
        let (df, other) = (Uuid::now_v7(), Uuid::now_v7());
        let (df_run, other_run) = (run(df, "DF"), run(other, "WEB"));
        let update = assemble(
            9,
            &[df_run.clone(), other_run],
            &[
                entry(HostScope::Instance, "api.anthropic.com"),
                entry(HostScope::Project(df), "*.crates.io"),
                entry(HostScope::Project(other), "pypi.org"),
                entry(HostScope::Task(df_run.task_id), "files.pythonhosted.org"),
                entry(HostScope::Task(Uuid::now_v7()), "registry.npmjs.org"),
            ],
        );
        assert_eq!(update.version, 9);
        let sandboxes = update.snapshot.unwrap().sandboxes;
        assert!(sandboxes.iter().all(|sandbox| sandbox.run_number == 2));
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
                ("*.crates.io".to_string(), "project".to_string()),
                ("files.pythonhosted.org".to_string(), "task".to_string())
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
        let update = assemble(1, &[], &[entry(HostScope::Instance, "x.com")]);
        assert!(update.snapshot.unwrap().sandboxes.is_empty());
    }

    #[test]
    fn a_run_gets_full_ref_names_and_the_commit_identity_in_its_git_route() {
        let update = assemble(1, &[run(Uuid::now_v7(), "DF")], &[]);
        let route = update.snapshot.unwrap().sandboxes[0].git.clone().unwrap();
        assert_eq!(route.upstream_url, "https://example.test/r.git");
        assert_eq!(route.base_branch, "refs/heads/main");
        assert_eq!(route.task_branch, "refs/heads/factory/task/DF-1");
        assert_eq!(route.wip_branch, "refs/heads/factory/wip/DF-1");
        assert_eq!(route.credential_ref, "");
        assert_eq!(
            (route.commit_name.as_str(), route.commit_email.as_str()),
            ("Dark Factory", "factory@example.test")
        );
    }

    #[test]
    fn a_repository_that_is_not_https_gets_no_git_route() {
        let mut ssh = run(Uuid::now_v7(), "DF");
        ssh.repo_url = "ssh://git@example.test/r.git".into();
        let update = assemble(1, &[ssh], &[]);
        assert_eq!(update.snapshot.unwrap().sandboxes[0].git, None);
    }

    #[test]
    fn a_plain_http_repository_gets_a_git_route_only_when_plain_http_is_allowed() {
        let mut plain = run(Uuid::now_v7(), "DF");
        plain.repo_url = "http://forgejo.forgejo.svc:3000/factory/demo".into();
        let runs = [plain];
        let update = assemble(1, &runs, &[]);
        assert_eq!(update.snapshot.unwrap().sandboxes[0].git, None);

        let allowing = GitSettings {
            allow_plain_http: true,
            ..settings()
        };
        let update = super::assemble(
            1,
            &runs,
            &[],
            &allowing,
            &PushData::default(),
            &HashMap::new(),
        );
        let route = update.snapshot.unwrap().sandboxes[0].git.clone().unwrap();
        assert_eq!(
            route.upstream_url,
            "http://forgejo.forgejo.svc:3000/factory/demo"
        );
    }

    #[test]
    fn every_project_of_a_run_gets_one_push_policy_with_the_defaults_unless_stored() {
        let (df, web) = (Uuid::now_v7(), Uuid::now_v7());
        let runs = [run(df, "DF"), run(df, "DF"), run(web, "WEB")];
        let stored = PushPolicy {
            protected_globs: vec!["deploy/**".into()],
            protected_mode: CheckMode::Reject,
            hidden_globs: vec!["talos/**".into()],
            max_blob_bytes: 1024,
            linear_history: true,
            ..PushPolicy::default()
        };
        let push = PushData {
            policies: HashMap::from([(web, stored)]),
            ..PushData::default()
        };

        let update = super::assemble(1, &runs, &[], &settings(), &push, &HashMap::new());

        let policies = update.snapshot.unwrap().push_policies;
        assert_eq!(policies.len(), 2);
        let defaults = &policies["DF"];
        assert_eq!(
            defaults.protected_globs,
            push_checks::DEFAULT_PROTECTED_GLOBS
        );
        assert_eq!(
            (
                defaults.protected_mode(),
                defaults.secret_mode(),
                defaults.size_mode()
            ),
            (
                pb::CheckMode::Approval,
                pb::CheckMode::Approval,
                pb::CheckMode::Approval
            )
        );
        assert_eq!(defaults.max_blob_bytes, 5 * 1024 * 1024);
        assert!(!defaults.linear_history);
        let web = &policies["WEB"];
        assert_eq!(web.protected_globs, ["deploy/**"]);
        assert_eq!(web.hidden_globs, ["talos/**"]);
        assert!(defaults.hidden_globs.is_empty());
        assert_eq!(
            (web.protected_mode(), web.secret_mode()),
            (pb::CheckMode::Reject, pb::CheckMode::Approval)
        );
        assert_eq!((web.max_blob_bytes, web.linear_history), (1024, true));
    }

    #[test]
    fn a_sandbox_gets_the_exceptions_of_its_task_only() {
        let project = Uuid::now_v7();
        let (first, second) = (run(project, "DF"), run(project, "DF"));
        let push = PushData {
            exceptions: HashMap::from([(first.task_id, vec!["path:CODEOWNERS".to_string()])]),
            ..PushData::default()
        };

        let update = super::assemble(
            1,
            &[first, second],
            &[],
            &settings(),
            &push,
            &HashMap::new(),
        );

        let sandboxes = update.snapshot.unwrap().sandboxes;
        assert_eq!(sandboxes[0].exceptions, ["path:CODEOWNERS"]);
        assert!(sandboxes[1].exceptions.is_empty());
    }

    /// Counts lookups and answers every reference with its secret name.
    #[derive(Default)]
    struct CountingSecrets(std::sync::Mutex<Vec<String>>);

    #[async_trait::async_trait]
    impl SecretStore for CountingSecrets {
        async fn get(
            &self,
            reference: &df_core::SecretKeyRef,
        ) -> Result<secrecy::SecretString, df_core::SecretError> {
            self.0.lock().unwrap().push(reference.name.clone());
            Ok(reference.name.clone().into())
        }

        async fn get_or_create(
            &self,
            reference: &df_core::SecretKeyRef,
            _labels: &std::collections::BTreeMap<String, String>,
        ) -> Result<secrecy::SecretString, df_core::SecretError> {
            self.get(reference).await
        }
    }

    fn credential(secret_name: &str) -> df_core::CredentialRef {
        df_core::CredentialRef {
            id: df_core::CredentialRefId::new(),
            name: secret_name.into(),
            secret: df_core::SecretKeyRef {
                name: secret_name.into(),
                key: "token".into(),
            },
        }
    }

    #[tokio::test]
    async fn only_credentials_of_built_git_routes_are_read_once_each() {
        let shared = credential("shared");
        let mut first = run(Uuid::now_v7(), "DF");
        first.forge_credential = Some(shared.clone());
        let mut second = run(Uuid::now_v7(), "WEB");
        second.forge_credential = Some(shared.clone());
        let mut no_route = run(Uuid::now_v7(), "SSH");
        no_route.repo_url = "ssh://git@example.test/r.git".into();
        no_route.forge_credential = Some(credential("unused"));
        let runs = [first, second, no_route];
        let update = assemble(1, &runs, &[]);
        let secrets = CountingSecrets::default();

        let credentials = forge_credentials(
            &secrets,
            &runs,
            &update.snapshot.unwrap().sandboxes,
            "dark-factory",
        )
        .await;

        assert_eq!(*secrets.0.lock().unwrap(), ["shared"]);
        assert_eq!(
            credentials.keys().collect::<Vec<_>>(),
            [&shared.id.to_string()]
        );
    }
}
