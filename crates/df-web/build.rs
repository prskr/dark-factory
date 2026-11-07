//! Stages the web assets into `$OUT_DIR/assets/`: the checked-in `assets/` folder, the scripts
//! compiled from `scripts/*.ts` and the Tailwind stylesheet `app.css`. The decisions are in
//! `build_assets.rs`, which has the tests.
//!
//! The cfgs `df_web_scripts` and `df_web_styles` tell the tests which kinds were compiled.

mod build_assets;

use std::env;
use std::path::{Path, PathBuf};

use build_assets::{
    Config, Result, Tool, parse_web_scripts, rerun_inputs, resolve_path, stage_assets,
};

/// An environment variable that may be unset; a value that is not UTF-8 is an error.
fn optional_env(var: &str) -> Result<Option<String>> {
    match env::var(var) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(err @ env::VarError::NotUnicode(_)) => Err(format!("{var}: {err}").into()),
    }
}

fn tool(var: &'static str, default: &str, manifest_dir: &Path, pwd: Option<&Path>) -> Result<Tool> {
    Ok(match optional_env(var)? {
        Some(path) => Tool {
            program: resolve_path(&path, manifest_dir, pwd),
            override_var: Some(var),
        },
        None => Tool {
            program: default.into(),
            override_var: None,
        },
    })
}

fn main() -> Result<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let staged = PathBuf::from(env::var("OUT_DIR")?).join("assets");

    for path in [
        "styles/input.css",
        "scripts",
        ".swcrc",
        "src",
        "assets",
        "build_assets.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    for var in [
        "WEB_SCRIPTS",
        "TAILWIND_APP_CSS",
        "SWC_BIN",
        "TAILWIND_BIN",
        "DF_WEB_REQUIRE_ASSETS",
        "PATH",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rustc-check-cfg=cfg(df_web_scripts)");
    println!("cargo:rustc-check-cfg=cfg(df_web_styles)");

    // Only a lookup hint for `resolve_path`, so any value (even non-UTF-8) is fine.
    let pwd = env::var_os("PWD").map(PathBuf::from);
    let pwd = pwd.as_deref();
    let config = Config {
        require_assets: optional_env("DF_WEB_REQUIRE_ASSETS")?.is_some_and(|v| v == "1"),
        web_scripts: optional_env("WEB_SCRIPTS")?
            .map(|files| parse_web_scripts(&files, &manifest_dir, pwd)),
        tailwind_app_css: optional_env("TAILWIND_APP_CSS")?
            .map(|file| resolve_path(&file, &manifest_dir, pwd)),
        swc: tool("SWC_BIN", "swc", &manifest_dir, pwd)?,
        tailwind: tool("TAILWIND_BIN", "tailwindcss", &manifest_dir, pwd)?,
        manifest_dir,
    };
    for input in rerun_inputs(&config, env::var_os("PATH").as_deref()) {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    let report = stage_assets(&config, &staged)?;
    for cfg in report.cfgs {
        println!("cargo:rustc-cfg={cfg}");
    }
    for warning in report.warnings {
        println!("cargo:warning={warning}");
    }
    Ok(())
}
