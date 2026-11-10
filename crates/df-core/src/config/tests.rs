use super::*;
use secrecy::ExposeSecret;
use std::io::Write;
use tempfile::NamedTempFile;

const VALID_FIXTURE: &str = r"
instanceName: homelab-dev
database:
  host: pg.dark-factory.svc
  port: 5432
  database: dark_factory
  user: dark_factory_app
  passwordFile: /run/secrets/db-password
  schema: dark_factory
  maxConnections: 16
  migrate:
    appRole: dark_factory_app
    manageAppRole: true
    appPasswordFile: /run/secrets/app-password
server:
  httpPort: 8080
  agentPort: 9443
  internalGrpcPort: 9444
gateway:
  proxyPort: 3128
  internalGrpcPort: 9444
  healthPort: 8080
sandbox:
  namespace: dark-factory-sandboxes
  instanceNamespace: dark-factory
";

#[test]
fn auth_is_optional_and_roles_claim_defaults_to_roles() {
    assert!(
        InstanceConfig::from_yaml_str(VALID_FIXTURE)
            .unwrap()
            .auth
            .is_none()
    );
    let yaml = format!(
        "{VALID_FIXTURE}publicUrl: https://factory.example.org\nauth:\n  issuerUrl: https://kc.example.org/realms/df\n  clientId: dark-factory\n  clientSecretFile: /run/secrets/oidc/secret\n"
    );
    let auth = InstanceConfig::from_yaml_str(&yaml).unwrap().auth.unwrap();
    assert_eq!(auth.roles_claim, "roles");
    assert_eq!(auth.client_id, "dark-factory");
    assert!(auth.extra_scopes.is_empty());
}

#[test]
fn auth_extra_scopes_are_a_list() {
    let yaml = format!(
        "{VALID_FIXTURE}publicUrl: http://127.0.0.1:8080\nauth:\n  issuerUrl: http://127.0.0.1:5556/dex\n  clientId: dark-factory\n  clientSecretFile: /s\n  extraScopes: [groups, offline_access]\n"
    );
    let auth = InstanceConfig::from_yaml_str(&yaml).unwrap().auth.unwrap();
    assert_eq!(auth.extra_scopes, ["groups", "offline_access"]);
}

const AUTH_SECTION: &str = "auth:\n  issuerUrl: https://kc.example.org/realms/df\n  clientId: dark-factory\n  clientSecretFile: /s\n";

#[test]
fn top_level_public_url_parses() {
    let yaml = format!("{VALID_FIXTURE}publicUrl: https://factory.example.org\n");
    let cfg = InstanceConfig::from_yaml_str(&yaml).unwrap();
    assert_eq!(
        cfg.public_url.as_ref().map(url::Url::as_str),
        Some("https://factory.example.org/")
    );
    assert!(
        InstanceConfig::from_yaml_str(VALID_FIXTURE)
            .unwrap()
            .public_url
            .is_none()
    );
}

#[test]
fn a_public_url_that_is_not_a_url_is_rejected_naming_the_setting() {
    let yaml = format!("{VALID_FIXTURE}publicUrl: not a url\n");
    let err = InstanceConfig::from_yaml_str(&yaml)
        .unwrap_err()
        .to_string();
    assert!(err.contains("publicUrl"), "{err}");
}

#[test]
fn public_url_inside_auth_is_rejected_as_unknown_field() {
    let yaml = format!(
        "{VALID_FIXTURE}publicUrl: https://f.example.org\n{AUTH_SECTION}  publicUrl: https://f.example.org\n"
    );
    let err = InstanceConfig::from_yaml_str(&yaml)
        .unwrap_err()
        .to_string();
    assert!(err.contains("publicUrl"), "{err}");
}

#[test]
fn auth_without_public_url_fails_naming_public_url() {
    let yaml = format!("{VALID_FIXTURE}{AUTH_SECTION}");
    let err = InstanceConfig::from_yaml_str(&yaml)
        .unwrap_err()
        .to_string();
    assert!(err.contains("publicUrl"), "{err}");
}

#[test]
fn plain_http_public_url_on_a_remote_host_sets_the_warning_flag() {
    let flag = |url: &str| {
        let yaml = format!("{VALID_FIXTURE}publicUrl: {url}\n");
        InstanceConfig::from_yaml_str(&yaml)
            .unwrap()
            .public_url_is_plain_http_remote()
    };
    assert!(flag("http://factory.internal"));
    assert!(!flag("https://factory.internal"));
    for local in [
        "http://127.0.0.1:8080",
        "http://localhost:8080",
        "http://[::1]:8080",
    ] {
        assert!(!flag(local), "{local}");
    }
    assert!(
        !InstanceConfig::from_yaml_str(VALID_FIXTURE)
            .unwrap()
            .public_url_is_plain_http_remote()
    );
}

#[test]
fn plugins_are_a_map_with_free_form_settings() {
    let yaml = format!(
        "{VALID_FIXTURE}plugins:\n  forgejo:\n    path: /p.wasm\n    callTimeoutSeconds: 5\n    settings:\n      wip-prefixes: 'WIP:'\n  other:\n    path: /o.wasm\n"
    );
    let cfg = InstanceConfig::from_yaml_str(&yaml).unwrap();
    let forgejo = &cfg.plugins["forgejo"];
    assert_eq!(forgejo.path, PathBuf::from("/p.wasm"));
    assert_eq!((forgejo.call_timeout_seconds, forgejo.memory_mib), (5, 64));
    assert_eq!(forgejo.settings["wip-prefixes"], "WIP:");
    assert!(cfg.plugins["other"].settings.is_empty());
    assert!(
        InstanceConfig::from_yaml_str(VALID_FIXTURE)
            .unwrap()
            .plugins
            .is_empty()
    );
}

#[test]
fn plugin_call_timeout_above_300_seconds_is_rejected_naming_key_and_maximum() {
    let yaml = format!(
        "{VALID_FIXTURE}plugins:\n  forgejo:\n    path: /p.wasm\n    callTimeoutSeconds: 600\n"
    );
    let err = InstanceConfig::from_yaml_str(&yaml)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("plugins.forgejo.callTimeoutSeconds") && err.contains("300"),
        "{err}"
    );
    let at_maximum = yaml.replace("600", "300");
    assert!(InstanceConfig::from_yaml_str(&at_maximum).is_ok());
}

#[test]
fn plugin_call_timeout_of_zero_is_rejected_naming_the_key() {
    let yaml = format!(
        "{VALID_FIXTURE}plugins:\n  forgejo:\n    path: /p.wasm\n    callTimeoutSeconds: 0\n"
    );
    let err = InstanceConfig::from_yaml_str(&yaml)
        .unwrap_err()
        .to_string();
    assert!(err.contains("plugins.forgejo.callTimeoutSeconds"), "{err}");
}

fn plugin(yaml_body: &str) -> Result<PluginConfig, ConfigError> {
    let yaml = format!("{VALID_FIXTURE}plugins:\n  github:\n    path: /g.wasm\n{yaml_body}");
    Ok(InstanceConfig::from_yaml_str(&yaml)?.plugins["github"].clone())
}

#[test]
fn plugin_webhook_secret_reference_is_optional_and_generate_defaults_to_false() {
    assert_eq!(plugin("").unwrap().webhook_secret, None);
    let plain = plugin("    webhookSecret:\n      name: wh\n      key: secret\n").unwrap();
    assert_eq!(
        plain.webhook_secret,
        Some(WebhookSecretConfig {
            name: "wh".into(),
            key: "secret".into(),
            generate: false,
        })
    );
    let generated =
        plugin("    webhookSecret:\n      name: wh\n      key: secret\n      generate: true\n")
            .unwrap();
    assert!(generated.webhook_secret.unwrap().generate);
}

#[test]
fn plugin_webhook_secret_file_is_rejected_as_unknown_field() {
    let err = plugin("    webhookSecretFile: /var/run/webhooks/github\n")
        .unwrap_err()
        .to_string();
    assert!(err.contains("webhookSecretFile"), "{err}");
}

#[test]
fn plugin_hosts_accept_a_plain_string() {
    let cfg = plugin("    hosts:\n      - ghe.example.org\n").unwrap();
    assert_eq!(cfg.hosts, [HostRoute::Host("ghe.example.org".into())]);
}

#[test]
fn plugin_hosts_accept_host_with_api_url() {
    let cfg = plugin(
        "    hosts:\n      - host: github.com\n        apiUrl: https://api.github.com\n      - ghe.example.org\n",
    )
    .unwrap();
    assert_eq!(
        cfg.hosts,
        [
            HostRoute::WithApiUrl(HostWithApiUrl {
                host: "github.com".into(),
                api_url: "https://api.github.com".into(),
            }),
            HostRoute::Host("ghe.example.org".into()),
        ]
    );
}

#[test]
fn plugin_host_with_api_url_rejects_unknown_keys() {
    for entry in [
        "      - host: a\n        api: b\n",
        "      - host: a\n        apiUrl: https://b\n        api: c\n",
    ] {
        assert!(plugin(&format!("    hosts:\n{entry}")).is_err(), "{entry}");
    }
}

#[test]
fn plugin_auth_scheme_accepts_bearer() {
    let cfg = plugin("    authScheme: bearer\n").unwrap();
    assert_eq!(cfg.auth_scheme, AuthScheme::Bearer.into());
}

#[test]
fn plugin_unknown_auth_scheme_is_kept_for_the_routing_table_to_reject() {
    let cfg = plugin("    authScheme: basic\n").unwrap();
    assert_eq!(cfg.auth_scheme, AuthSchemeSetting::Unknown("basic".into()));
}

#[test]
fn plugin_without_hosts_and_auth_scheme_is_default_with_token() {
    let cfg = plugin("").unwrap();
    assert!(cfg.hosts.is_empty());
    assert_eq!(cfg.auth_scheme, AuthScheme::Token.into());
    assert_eq!(cfg, PluginConfig::new("/g.wasm"));
}

#[test]
fn cluster_targets_default_to_no_namespaces() {
    let cfg: ClusterTargetsConfig = serde_saphyr::from_str("namespaces: [a, b]").unwrap();
    assert!(!cfg.any_namespace);
    assert_eq!(cfg.namespaces, ["a", "b"]);
    assert_eq!(ClusterTargetsConfig::default().namespaces.len(), 0);
}

#[test]
fn sandbox_section_defaults_leave_runtime_class_and_storage_class_unset() {
    let section: SandboxSectionConfig = serde_saphyr::from_str("namespace: sb").unwrap();
    let p = &section.provisioning;
    assert_eq!(p.namespace, "sb");
    assert_eq!(p.storage_class, None);
    assert_eq!(p.runtime_class_name, None);
    assert_eq!(p.pvc_size, "20Gi");
    assert_eq!(p.issuer_kind, "ClusterIssuer");
    assert_eq!(section.endpoints, SandboxEndpointsConfig::default());
    assert_eq!(
        section.provisioning,
        SandboxProvisioningConfig {
            namespace: "sb".into(),
            ..Default::default()
        }
    );
}

#[test]
fn harness_images_come_from_the_parsed_instance_config() {
    let yaml = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n\
                harness:\n  claudeCode:\n    image: registry.example/harness:1\n\
                \x20 opencode:\n    image: registry.example/opencode:2\n";
    let cfg = InstanceConfig::from_yaml_str(yaml).unwrap();
    assert_eq!(
        cfg.harness.images(),
        BTreeMap::from([
            (
                CLAUDE_CODE_HARNESS.to_string(),
                "registry.example/harness:1".to_string()
            ),
            (
                OPENCODE_HARNESS.to_string(),
                "registry.example/opencode:2".to_string()
            )
        ])
    );
}

#[test]
fn opencode_defaults_and_overrides_come_from_the_instance_config() {
    let base = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
    let cfg = InstanceConfig::from_yaml_str(base).unwrap();
    assert_eq!(
        cfg.harness.opencode.adapter_command,
        ["/opt/harness/opencode/bin/opencode", "acp"]
    );
    assert_eq!(
        cfg.harness.opencode.image,
        "ghcr.io/prskr/dark-factory/opencode-harness:2.2.5"
    );
    assert!(cfg.harness.opencode.env.is_empty());
    assert_eq!(cfg.harness.images().len(), 2);

    let yaml = format!(
        "{base}harness:\n  opencode:\n    image: registry.example/oc:1\n    adapterCommand: [/bin/oc]\n    env:\n      A: b\n"
    );
    let cfg = InstanceConfig::from_yaml_str(&yaml).unwrap();
    assert_eq!(
        cfg.harness.images()[OPENCODE_HARNESS],
        "registry.example/oc:1"
    );
    assert_eq!(
        cfg.harness.adapter_command(OPENCODE_HARNESS),
        Some(&["/bin/oc".to_string()][..])
    );
    assert_eq!(cfg.harness.env(OPENCODE_HARNESS).unwrap()["A"], "b");
    assert!(cfg.harness.adapter_command("nope").is_none());
}

#[test]
fn opencode_rejects_unknown_keys() {
    let base = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
    let err = InstanceConfig::from_yaml_str(&format!(
        "{base}harness:\n  opencode:\n    settingsJson: x\n"
    ))
    .unwrap_err();
    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn blank_harness_image_counts_as_not_configured() {
    let yaml = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n\
                harness:\n  claudeCode:\n    image: \"  \"\n  opencode:\n    image: \"\"\n";
    let cfg = InstanceConfig::from_yaml_str(yaml).unwrap();
    assert!(cfg.harness.images().is_empty());
}

#[test]
fn removed_runtime_delivery_key_is_ignored() {
    let section: SandboxSectionConfig =
        serde_saphyr::from_str("namespace: sb\nruntimeDelivery: initContainer").unwrap();
    assert_eq!(section.provisioning.namespace, "sb");
}

#[test]
fn sandbox_log_level_defaults_to_info_and_reads_explicit_value() {
    let section: SandboxSectionConfig = serde_saphyr::from_str("namespace: sb").unwrap();
    assert_eq!(section.provisioning.log_level, "info");
    let section: SandboxSectionConfig =
        serde_saphyr::from_str("logLevel: info,factory_agent=debug").unwrap();
    assert_eq!(section.provisioning.log_level, "info,factory_agent=debug");
}

#[test]
fn sandbox_section_empty_strings_mean_unset() {
    let section: SandboxSectionConfig =
        serde_saphyr::from_str("storageClass: \"\"\nruntimeClassName: \"  \"").unwrap();
    assert_eq!(section.provisioning.storage_class, None);
    assert_eq!(section.provisioning.runtime_class_name, None);

    let section: SandboxSectionConfig =
        serde_saphyr::from_str("storageClass: fast\nruntimeClassName: kata").unwrap();
    assert_eq!(section.provisioning.storage_class.as_deref(), Some("fast"));
    assert_eq!(
        section.provisioning.runtime_class_name.as_deref(),
        Some("kata")
    );
}

#[test]
fn sandbox_section_reads_provisioning_and_endpoint_keys_from_one_table() {
    let section: SandboxSectionConfig = serde_saphyr::from_str(
        "namespace: sb\ncontrolPlaneIp: 10.0.0.1\ngatewayServiceName: gw\ncontrolPlaneTlsServerName: cp.local",
    )
    .unwrap();
    assert_eq!(section.provisioning.namespace, "sb");
    assert_eq!(
        section.endpoints.control_plane_ip,
        Some("10.0.0.1".parse().unwrap())
    );
    assert_eq!(
        section.endpoints.gateway_service_name.as_deref(),
        Some("gw")
    );
    assert_eq!(
        section.endpoints.control_plane_tls_server_name.as_deref(),
        Some("cp.local")
    );
}

#[test]
fn loads_valid_instance_config_fixture_and_reads_secret_file() {
    let mut pw_file = NamedTempFile::new().unwrap();
    writeln!(pw_file, "s3cr3t-pass").unwrap();

    let yaml = VALID_FIXTURE.replace("/run/secrets/db-password", pw_file.path().to_str().unwrap());
    let mut cfg_file = NamedTempFile::new().unwrap();
    write!(cfg_file, "{yaml}").unwrap();

    let cfg = InstanceConfig::from_file(cfg_file.path()).expect("should load valid config");
    assert_eq!(cfg.instance_name, "homelab-dev");
    assert_eq!(cfg.database.host, "pg.dark-factory.svc");
    assert_eq!(cfg.database.port, 5432);
    assert_eq!(cfg.database.database, "dark_factory");
    assert_eq!(cfg.database.user, "dark_factory_app");
    assert_eq!(cfg.database.schema, "dark_factory");
    assert_eq!(cfg.database.max_connections, 16);
    assert!(cfg.database.migrate.manage_app_role);
    assert_eq!(cfg.server.http_port, 8080);
    assert_eq!(cfg.server.agent_port, 9443);
    assert_eq!(cfg.server.internal_grpc_port, 9444);
    assert_eq!(cfg.gateway.proxy_port, 3128);

    let secret = cfg.database.read_password().unwrap();
    assert_eq!(secret.expose_secret(), "s3cr3t-pass");
}

#[test]
fn gateway_section_has_defaults_derived_from_the_instance_name() {
    let minimal = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
    let gateway = InstanceConfig::from_yaml_str(minimal).unwrap().gateway;
    assert_eq!(gateway.buffer_size, 10_000);
    assert_eq!(gateway.buffer_max_age_seconds, 3600);
    assert!(gateway.blocked_cidrs.is_empty() && gateway.headless_service.is_none());
    assert_eq!(gateway.control_plane_name("df"), "df-control-plane");
    assert_eq!(gateway.identity_name("df"), "df-gateway");

    let yaml = format!(
        "{minimal}gateway:\n  controlPlaneHost: cp.svc\n  identity: gw\n  headlessService: gw-headless.ns.svc\n  bufferSize: 50\n  bufferMaxAgeSeconds: 120\n  blockedCidrs: [\"10.42.0.0/16\"]\n"
    );
    let gateway = InstanceConfig::from_yaml_str(&yaml).unwrap().gateway;
    assert_eq!(gateway.control_plane_name("df"), "cp.svc");
    assert_eq!(gateway.identity_name("df"), "gw");
    assert_eq!(gateway.buffer_size, 50);
    assert_eq!(gateway.buffer_max_age_seconds, 120);
    assert_eq!(gateway.blocked_cidrs, ["10.42.0.0/16"]);
}

#[test]
fn network_section_defaults_to_https_only_and_rejects_unknown_keys() {
    let minimal = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
    assert!(
        !InstanceConfig::from_yaml_str(minimal)
            .unwrap()
            .network
            .allow_plain_http
    );
    let on = format!("{minimal}network:\n  allowPlainHttp: true\n");
    assert!(
        InstanceConfig::from_yaml_str(&on)
            .unwrap()
            .network
            .allow_plain_http
    );
    let unknown = format!("{minimal}network:\n  plainHttp: true\n");
    assert!(InstanceConfig::from_yaml_str(&unknown).is_err());
}

#[test]
fn git_proxy_defaults_to_port_8443_on_the_gateway_name() {
    let minimal = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
    let gateway = InstanceConfig::from_yaml_str(minimal).unwrap().gateway;
    assert_eq!(gateway.git_port, 8443);
    assert_eq!(gateway.git_host_name("df"), "df-gateway");

    let yaml = format!("{minimal}gateway:\n  gitPort: 9443\n  gitHost: df-gateway.df.svc\n");
    let gateway = InstanceConfig::from_yaml_str(&yaml).unwrap().gateway;
    assert_eq!(gateway.git_port, 9443);
    assert_eq!(gateway.git_host_name("df"), "df-gateway.df.svc");
}

#[test]
fn web_section_defaults_to_hidden_blocks_off_and_rejects_unknown_keys() {
    let minimal = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
    let cfg = InstanceConfig::from_yaml_str(minimal).unwrap();
    assert!(!cfg.web.show_hidden_blocks);

    let on = format!("{minimal}web:\n  showHiddenBlocks: true\n");
    assert!(
        InstanceConfig::from_yaml_str(&on)
            .unwrap()
            .web
            .show_hidden_blocks
    );

    let unknown = format!("{minimal}web:\n  showHidden: true\n");
    assert!(InstanceConfig::from_yaml_str(&unknown).is_err());
}

#[test]
fn code_themes_default_and_can_be_set_individually() {
    let minimal = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
    let themes = InstanceConfig::from_yaml_str(minimal)
        .unwrap()
        .web
        .code_theme;
    assert_eq!(themes.light, "InspiredGitHub");
    assert_eq!(themes.dark, "base16-ocean.dark");

    let yaml = format!("{minimal}web:\n  codeTheme:\n    dark: Solarized (dark)\n");
    let themes = InstanceConfig::from_yaml_str(&yaml).unwrap().web.code_theme;
    assert_eq!(themes.light, "InspiredGitHub");
    assert_eq!(themes.dark, "Solarized (dark)");

    let unknown = format!("{minimal}web:\n  codeTheme:\n    mid: x\n");
    assert!(InstanceConfig::from_yaml_str(&unknown).is_err());
}

#[test]
fn run_orchestration_sections_have_defaults_and_can_be_overridden() {
    let minimal = r"
database:
  host: pg
  database: d
  user: u
  passwordFile: /p
";
    let cfg = InstanceConfig::from_yaml_str(minimal).unwrap();
    assert!(cfg.agent.tls.is_none());
    assert_eq!(cfg.agent.commit_identity.name, "Dark Factory");
    assert_eq!(cfg.agent.git_username, "dark-factory");
    assert_eq!(cfg.runs.max_wall_clock_seconds, 7200);
    assert_eq!(cfg.runs.connect_timeout_seconds, 600);
    assert_eq!(cfg.runs.cancel_grace_seconds, 300);
    assert_eq!(cfg.runs.keep_alive_seconds, 600);
    assert_eq!(cfg.runs.max_concurrent, 4);
    assert_eq!(cfg.runs.max_auto_resumes, 3);
    assert_eq!(
        cfg.harness.claude_code.adapter_command,
        [
            "/opt/harness/claude-code/bin/node",
            "/opt/harness/claude-code/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js"
        ]
    );
    assert_eq!(
        cfg.harness.claude_code.image,
        "ghcr.io/prskr/dark-factory/claude-harness:0.95.0"
    );

    let custom = format!(
        r#"{minimal}
agent:
  tls:
    certFile: /tls/tls.crt
    keyFile: /tls/tls.key
    clientCaFile: /tls/ca.crt
runs:
  maxWallClockSeconds: 60
  keepAliveSeconds: 30
  maxConcurrent: 1
  maxAutoResumes: 0
harness:
  claudeCode:
    image: registry.example/harness:1
    env:
      DISABLE_AUTOUPDATER: "1"
"#
    );
    let cfg = InstanceConfig::from_yaml_str(&custom).unwrap();
    let tls = cfg.agent.tls.unwrap();
    assert_eq!(tls.client_ca_file, PathBuf::from("/tls/ca.crt"));
    assert_eq!(cfg.runs.max_wall_clock_seconds, 60);
    assert_eq!(cfg.runs.keep_alive_seconds, 30);
    assert_eq!(cfg.runs.max_concurrent, 1);
    assert_eq!(cfg.runs.max_auto_resumes, 0);
    assert_eq!(cfg.runs.connect_timeout_seconds, 600);
    assert_eq!(cfg.harness.claude_code.env["DISABLE_AUTOUPDATER"], "1");
    assert_eq!(cfg.harness.claude_code.image, "registry.example/harness:1");
}

#[test]
fn volume_retention_defaults_to_seven_and_thirty_days_and_can_be_overridden() {
    let minimal = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
    let cfg = InstanceConfig::from_yaml_str(minimal).unwrap();
    assert_eq!(cfg.volumes.retain_after_close_seconds, 7 * 24 * 3600);
    assert_eq!(cfg.volumes.idle_ttl_seconds, 30 * 24 * 3600);

    let custom =
        format!("{minimal}volumes:\n  retainAfterCloseSeconds: 60\n  idleTtlSeconds: 120\n");
    let cfg = InstanceConfig::from_yaml_str(&custom).unwrap();
    assert_eq!(cfg.volumes.retain_after_close_seconds, 60);
    assert_eq!(cfg.volumes.idle_ttl_seconds, 120);

    let typo = format!("{minimal}volumes:\n  idleTtl: 120\n");
    assert!(InstanceConfig::from_yaml_str(&typo).is_err());
}

#[test]
fn a_concurrency_limit_of_zero_is_rejected() {
    let yaml = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\nruns:\n  maxConcurrent: 0\n";
    let err = InstanceConfig::from_yaml_str(yaml).unwrap_err();
    assert!(err.to_string().contains("runs.maxConcurrent"), "{err}");
}

#[test]
fn typos_in_agent_and_harness_sections_are_rejected() {
    let base = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
    for section in [
        "agent:\n  gitUserName: bot\n",
        "harness:\n  claude_code:\n    env: {}\n",
    ] {
        let err = InstanceConfig::from_yaml_str(&format!("{base}{section}")).unwrap_err();
        assert!(
            err.to_string().contains("unknown field"),
            "{section}: {err}"
        );
    }
}

#[test]
fn missing_required_field_yields_error_naming_field() {
    let missing_password_file = r"
database:
  host: pg.dark-factory.svc
  database: dark_factory
  user: dark_factory_app
";
    let err = InstanceConfig::from_yaml_str(missing_password_file).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("passwordFile"),
        "expected error to name missing field `passwordFile`, got: {msg}"
    );
}

const MINIMAL_DB: &str = r"
database:
  host: pg
  database: d
  user: u
  passwordFile: /p
";

#[test]
fn artifacts_section_defaults_when_missing() {
    let cfg = InstanceConfig::from_yaml_str(MINIMAL_DB).unwrap();
    assert_eq!(cfg.artifacts.max_text_bytes, 1_048_576);
    assert_eq!(cfg.artifacts.max_image_bytes, 3_932_160);
    assert_eq!(cfg.artifacts.max_per_task, 50);
}

#[test]
fn artifacts_options_can_be_overridden() {
    let yaml = format!("{MINIMAL_DB}artifacts:\n  maxTextBytes: 2048\n  maxPerTask: 3\n");
    let cfg = InstanceConfig::from_yaml_str(&yaml).unwrap();
    assert_eq!(cfg.artifacts.max_text_bytes, 2048);
    assert_eq!(cfg.artifacts.max_image_bytes, 3_932_160);
    assert_eq!(cfg.artifacts.max_per_task, 3);
}

#[test]
fn invalid_artifact_limits_are_rejected_naming_the_option() {
    for (body, option) in [
        ("maxPerTask: 0", "artifacts.maxPerTask"),
        ("maxTextBytes: 0", "artifacts.maxTextBytes"),
        ("maxImageBytes: 8388609", "artifacts.maxImageBytes"),
    ] {
        let yaml = format!("{MINIMAL_DB}artifacts:\n  {body}\n");
        let err = InstanceConfig::from_yaml_str(&yaml)
            .unwrap_err()
            .to_string();
        assert!(err.contains(option), "{err}");
    }
}
