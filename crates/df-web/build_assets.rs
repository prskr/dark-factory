//! Staging of the web assets, shared by `build.rs` and (through `#[path]`, for its tests) the
//! library. It reads no process state: `build.rs` fills a [`Config`] and prints the [`Report`].
//!
//! For the scripts and the stylesheet each, in this order: the output Bazel built
//! (`WEB_SCRIPTS`, `TAILWIND_APP_CSS`), the tool (`SWC_BIN` / `TAILWIND_BIN`, otherwise `PATH`),
//! otherwise a warning and nothing embedded. A tool that runs and fails is an error. With
//! `DF_WEB_REQUIRE_ASSETS=1` (Bazel) a missing tool is an error too. A `WEB_SCRIPTS` that lists
//! no file is an error.

use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// A tool that compiles one kind of asset.
pub(crate) struct Tool {
    /// What to run: the value of the override variable, or the program name to look up in `PATH`.
    pub(crate) program: PathBuf,
    /// The environment variable that set `program`; `None` for a `PATH` lookup.
    pub(crate) override_var: Option<&'static str>,
}

pub(crate) struct Config {
    pub(crate) manifest_dir: PathBuf,
    pub(crate) require_assets: bool,
    /// Bazel's compiled scripts.
    pub(crate) web_scripts: Option<Vec<PathBuf>>,
    /// Bazel's compiled stylesheet.
    pub(crate) tailwind_app_css: Option<PathBuf>,
    pub(crate) swc: Tool,
    pub(crate) tailwind: Tool,
}

/// What `build.rs` has to tell Cargo.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Report {
    /// Asset kinds that were compiled, as `rustc-cfg` names.
    pub(crate) cfgs: Vec<&'static str>,
    pub(crate) warnings: Vec<String>,
}

const SCRIPTS: Kind = Kind {
    cfg: "df_web_scripts",
    tool: "swc",
    missing: "the component scripts are",
};
const STYLES: Kind = Kind {
    cfg: "df_web_styles",
    tool: "tailwindcss",
    missing: "the stylesheet is",
};

/// Bazel's `$(execpaths)` value of `WEB_SCRIPTS`: space-separated paths, each resolved by
/// [`resolve_path`].
pub(crate) fn parse_web_scripts(
    raw: &str,
    manifest_dir: &Path,
    pwd: Option<&Path>,
) -> Vec<PathBuf> {
    raw.split_whitespace()
        .map(|entry| resolve_path(entry, manifest_dir, pwd))
        .collect()
}

/// Bazel passes paths relative to the execroot (`pwd`); `cargo` runs in the manifest directory.
pub(crate) fn resolve_path(raw: &str, manifest_dir: &Path, pwd: Option<&Path>) -> PathBuf {
    let candidate = PathBuf::from(raw);
    if candidate.exists() {
        return candidate;
    }
    if let Some(from_pwd) = pwd.map(|pwd| pwd.join(raw)).filter(|path| path.exists()) {
        return from_pwd;
    }
    manifest_dir.join(raw)
}

/// Files outside the crate whose change must rerun the build script: Bazel's outputs, the
/// override programs, and, for a tool looked up on `PATH`, every readable `PATH` directory, so
/// that installing or updating the tool there is noticed although `PATH` itself is unchanged.
pub(crate) fn rerun_inputs(config: &Config, path: Option<&OsStr>) -> Vec<PathBuf> {
    let mut inputs: Vec<PathBuf> = config.web_scripts.iter().flatten().cloned().collect();
    inputs.extend(config.tailwind_app_css.clone());
    let mut path_lookup = false;
    for (tool, used) in [
        (&config.swc, config.web_scripts.is_none()),
        (&config.tailwind, config.tailwind_app_css.is_none()),
    ] {
        match (used, tool.override_var) {
            (false, _) => {}
            (true, Some(_)) => inputs.push(tool.program.clone()),
            (true, None) => path_lookup = true,
        }
    }
    if path_lookup {
        // Unreadable directories (`/root/...` in a container) would make Cargo rerun every time.
        for dir in env::split_paths(path.unwrap_or_default()) {
            if fs::read_dir(&dir).is_ok() && !inputs.contains(&dir) {
                inputs.push(dir);
            }
        }
    }
    inputs
}

/// Rebuilds `staged` from `assets/` and the compiled outputs. Whatever an earlier build left
/// there is removed first, so output of a tool that is gone cannot stay embedded.
pub(crate) fn stage_assets(config: &Config, staged: &Path) -> Result<Report> {
    match fs::remove_dir_all(staged) {
        Err(err) if err.kind() != ErrorKind::NotFound => return Err(err.into()),
        _ => {}
    }
    copy_dir_recursive(&config.manifest_dir.join("assets"), staged)?;

    let mut report = Report::default();
    for (kind, built) in [
        (&SCRIPTS, stage_scripts(config, staged)?),
        (&STYLES, stage_styles(config, staged)?),
    ] {
        if built {
            report.cfgs.push(kind.cfg);
        } else if config.require_assets {
            return Err(format!(
                "{} is not available, but DF_WEB_REQUIRE_ASSETS is set",
                kind.tool
            )
            .into());
        } else {
            report.warnings.push(format!(
                "{} not found: {} not embedded and the web UI is incomplete; run `bazel run //:bazel_env` and put .bazel_env/bin on PATH (docs/development.md)",
                kind.tool, kind.missing
            ));
        }
    }
    Ok(report)
}

/// One kind of compiled asset.
struct Kind {
    cfg: &'static str,
    tool: &'static str,
    /// Subject of the warning: what is not embedded.
    missing: &'static str,
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Runs `command`; `Ok(false)` when the tool is not found.
fn run_tool(command: &mut Command, kind: &Kind, tool: &Tool) -> Result<bool> {
    let error = match command.status() {
        Ok(status) if status.success() => return Ok(true),
        Ok(status) => return Err(format!("{} exited with {status}", kind.tool).into()),
        Err(err) => err,
    };
    if let Some(var) = tool.override_var {
        // The user pointed at this program: it has to run.
        return Err(format!("{var}={} could not be run: {error}", tool.program.display()).into());
    }
    match error.kind() {
        // A `PATH` entry the user cannot read (`/root/...` in a container) makes the search for a
        // program that is not installed fail with "permission denied".
        ErrorKind::NotFound | ErrorKind::PermissionDenied => Ok(false),
        _ => Err(format!("{} could not be run: {error}", kind.tool).into()),
    }
}

/// Returns whether the scripts were staged.
fn stage_scripts(config: &Config, staged: &Path) -> Result<bool> {
    if let Some(built) = &config.web_scripts {
        if built.is_empty() {
            return Err("WEB_SCRIPTS is set but lists no compiled script".into());
        }
        for src in built {
            let name = src
                .file_name()
                .ok_or_else(|| format!("WEB_SCRIPTS entry {} has no file name", src.display()))?;
            fs::copy(src, staged.join(name))?;
        }
        return Ok(true);
    }

    let mut sources = Vec::new();
    for entry in fs::read_dir(config.manifest_dir.join("scripts"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "ts") {
            sources.push(path);
        }
    }
    sources.sort();
    for source in &sources {
        let name = source
            .file_stem()
            .ok_or_else(|| format!("scripts/ entry {} has no file name", source.display()))?;
        let mut command = Command::new(&config.swc.program);
        command
            .current_dir(&config.manifest_dir)
            .arg("compile")
            .arg(source)
            .arg("--config-file")
            .arg(config.manifest_dir.join(".swcrc"))
            .arg("--out-file")
            .arg(staged.join(name).with_extension("js"));
        if !run_tool(&mut command, &SCRIPTS, &config.swc)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Returns whether the stylesheet was staged.
fn stage_styles(config: &Config, staged: &Path) -> Result<bool> {
    let out = staged.join("app.css");
    if let Some(built) = &config.tailwind_app_css {
        fs::copy(built, &out)?;
        return Ok(true);
    }

    let mut command = Command::new(&config.tailwind.program);
    command
        .current_dir(&config.manifest_dir)
        .arg("-i")
        .arg(config.manifest_dir.join("styles/input.css"))
        .arg("-o")
        .arg(&out)
        .arg("--minify");
    run_tool(&mut command, &STYLES, &config.tailwind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const NOT_INSTALLED: &str = "df-web-test-tool-that-is-not-installed";

    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            fs::create_dir_all(root.join("assets")).unwrap();
            fs::write(root.join("assets/logo.svg"), "<svg/>").unwrap();
            fs::create_dir_all(root.join("scripts")).unwrap();
            fs::write(root.join("scripts/tabs.ts"), "").unwrap();
            fs::write(root.join("scripts/notes.md"), "").unwrap();
            Self { dir }
        }

        fn staged(&self) -> PathBuf {
            self.dir.path().join("out/assets")
        }

        fn config(&self, swc: &str, tailwind: &str) -> Config {
            Config {
                manifest_dir: self.dir.path().to_owned(),
                require_assets: false,
                web_scripts: None,
                tailwind_app_css: None,
                swc: Tool {
                    program: swc.into(),
                    override_var: None,
                },
                tailwind: Tool {
                    program: tailwind.into(),
                    override_var: None,
                },
            }
        }
    }

    #[test]
    fn missing_tools_warn_by_name_and_embed_nothing_compiled() {
        let fixture = Fixture::new();
        let report = stage_assets(
            &fixture.config(NOT_INSTALLED, NOT_INSTALLED),
            &fixture.staged(),
        )
        .unwrap();
        assert!(report.cfgs.is_empty());
        assert_eq!(report.warnings.len(), 2);
        assert!(
            report.warnings[0].starts_with("swc not found"),
            "{report:?}"
        );
        assert!(
            report.warnings[1].starts_with("tailwindcss not found"),
            "{report:?}"
        );
        let staged = fixture.staged();
        assert!(staged.join("logo.svg").exists(), "checked-in assets stay");
        assert!(!staged.join("tabs.js").exists() && !staged.join("app.css").exists());
    }

    #[test]
    fn a_missing_tool_is_an_error_when_the_assets_are_required() {
        let fixture = Fixture::new();
        let mut config = fixture.config("true", NOT_INSTALLED);
        config.require_assets = true;
        let err = stage_assets(&config, &fixture.staged()).unwrap_err();
        assert!(err.to_string().contains("tailwindcss"), "{err}");
    }

    #[test]
    fn compiled_kinds_emit_their_cfg() {
        let fixture = Fixture::new();
        let scripts_only =
            stage_assets(&fixture.config("true", NOT_INSTALLED), &fixture.staged()).unwrap();
        assert_eq!(scripts_only.cfgs, ["df_web_scripts"]);
        assert_eq!(scripts_only.warnings.len(), 1);
        let both = stage_assets(&fixture.config("true", "true"), &fixture.staged()).unwrap();
        assert_eq!(both.cfgs, ["df_web_scripts", "df_web_styles"]);
        assert!(both.warnings.is_empty());
    }

    #[test]
    fn output_of_an_earlier_build_does_not_outlive_its_tool() {
        let fixture = Fixture::new();
        let staged = fixture.staged();
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("tabs.js"), "old").unwrap();
        fs::write(staged.join("app.css"), "old").unwrap();
        stage_assets(&fixture.config(NOT_INSTALLED, NOT_INSTALLED), &staged).unwrap();
        assert!(!staged.join("tabs.js").exists() && !staged.join("app.css").exists());
        assert!(staged.join("logo.svg").exists());
    }

    #[test]
    fn bazel_outputs_are_copied_without_running_a_tool() {
        let fixture = Fixture::new();
        let built = fixture.dir.path().join("built");
        fs::create_dir_all(&built).unwrap();
        fs::write(built.join("tabs.js"), "js").unwrap();
        fs::write(built.join("app.css"), "css").unwrap();
        let mut config = fixture.config(NOT_INSTALLED, NOT_INSTALLED);
        config.web_scripts = Some(vec![built.join("tabs.js")]);
        config.tailwind_app_css = Some(built.join("app.css"));
        let report = stage_assets(&config, &fixture.staged()).unwrap();
        assert_eq!(report.cfgs, ["df_web_scripts", "df_web_styles"]);
        assert_eq!(
            fs::read_to_string(fixture.staged().join("tabs.js")).unwrap(),
            "js"
        );
        assert_eq!(
            fs::read_to_string(fixture.staged().join("app.css")).unwrap(),
            "css"
        );
    }

    #[test]
    fn relative_script_entries_resolve_against_the_manifest_dir_even_with_a_space_in_it() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("my crate");
        fs::create_dir_all(manifest.join("bin")).unwrap();
        fs::write(manifest.join("bin/tabs.js"), "js").unwrap();
        let paths = parse_web_scripts("bin/tabs.js bin/other.js", &manifest, None);
        assert_eq!(
            paths,
            [manifest.join("bin/tabs.js"), manifest.join("bin/other.js")]
        );
        let fixture = Fixture::new();
        let mut config = fixture.config(NOT_INSTALLED, "true");
        config.web_scripts = Some(paths[..1].to_vec());
        stage_assets(&config, &fixture.staged()).unwrap();
        assert_eq!(
            fs::read_to_string(fixture.staged().join("tabs.js")).unwrap(),
            "js"
        );
    }

    #[test]
    fn entries_found_relative_to_the_working_directory_win_over_the_manifest_dir() {
        let dir = tempfile::tempdir().unwrap();
        let (exec, manifest) = (dir.path().join("exec root"), dir.path().join("manifest"));
        fs::create_dir_all(exec.join("bin")).unwrap();
        fs::write(exec.join("bin/tabs.js"), "").unwrap();
        let paths = parse_web_scripts("bin/tabs.js", &manifest, Some(&exec));
        assert_eq!(paths, [exec.join("bin/tabs.js")]);
    }

    #[test]
    fn an_empty_bazel_script_list_is_an_error() {
        let fixture = Fixture::new();
        let mut config = fixture.config(NOT_INSTALLED, "true");
        config.web_scripts = Some(Vec::new());
        let err = stage_assets(&config, &fixture.staged()).unwrap_err();
        assert!(err.to_string().starts_with("WEB_SCRIPTS"), "{err}");
    }

    #[test]
    fn a_change_to_a_bazel_output_or_an_override_reruns_the_build() {
        let fixture = Fixture::new();
        let mut config = fixture.config(NOT_INSTALLED, NOT_INSTALLED);
        config.web_scripts = Some(vec!["/built/tabs.js".into()]);
        config.tailwind = Tool {
            program: "/tools/tailwindcss".into(),
            override_var: Some("TAILWIND_BIN"),
        };
        let path_dir = fixture.dir.path().to_str().unwrap().to_owned();
        assert_eq!(
            rerun_inputs(&config, Some(path_dir.as_ref())),
            [PathBuf::from("/built/tabs.js"), "/tools/tailwindcss".into()]
        );
    }

    #[test]
    fn a_tool_looked_up_on_path_reruns_the_build_when_a_readable_path_directory_changes() {
        let fixture = Fixture::new();
        let (bin, other) = (
            fixture.dir.path().join("bin"),
            fixture.dir.path().join("other"),
        );
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&other).unwrap();
        let missing = fixture.dir.path().join("missing");
        let path = env::join_paths([&bin, &missing, &other, &bin]).unwrap();
        let config = fixture.config(NOT_INSTALLED, NOT_INSTALLED);
        assert_eq!(rerun_inputs(&config, Some(&path)), [bin, other]);
    }

    #[test]
    fn a_tool_that_runs_and_fails_is_an_error() {
        let fixture = Fixture::new();
        let err =
            stage_assets(&fixture.config("false", NOT_INSTALLED), &fixture.staged()).unwrap_err();
        assert!(err.to_string().starts_with("swc exited"), "{err}");
    }

    #[test]
    fn an_override_that_cannot_run_is_an_error_naming_the_variable() {
        let fixture = Fixture::new();
        let not_executable = fixture.dir.path().join("swc");
        fs::write(&not_executable, "").unwrap();
        fs::set_permissions(&not_executable, fs::Permissions::from_mode(0o644)).unwrap();
        for program in [not_executable, fixture.dir.path().join("absent")] {
            let mut config = fixture.config(NOT_INSTALLED, NOT_INSTALLED);
            config.swc = Tool {
                program,
                override_var: Some("SWC_BIN"),
            };
            let err = stage_assets(&config, &fixture.staged()).unwrap_err();
            assert!(err.to_string().starts_with("SWC_BIN="), "{err}");
        }
    }
}
