use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            let _ = fs::remove_file(&dst_path);
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn resolve_path(raw: &str, manifest_dir: &Path) -> PathBuf {
    let candidate = PathBuf::from(raw);
    if candidate.exists() {
        return candidate;
    }
    if let Ok(pwd) = env::var("PWD") {
        let from_pwd = PathBuf::from(pwd).join(raw);
        if from_pwd.exists() {
            return from_pwd;
        }
    }
    manifest_dir.join(raw)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let staged_assets = out_dir.join("assets");
    fs::create_dir_all(&staged_assets)?;

    println!("cargo:rerun-if-changed=styles/input.css");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-env-changed=TAILWIND_APP_CSS");
    println!("cargo:rerun-if-env-changed=TAILWIND_BIN");

    let assets_dir = manifest_dir.join("assets");
    if assets_dir.exists() {
        copy_dir_recursive(&assets_dir, &staged_assets)?;
    }

    let app_css_out = staged_assets.join("app.css");

    if let Ok(pregenerated) = env::var("TAILWIND_APP_CSS") {
        let src = resolve_path(&pregenerated, &manifest_dir);
        let _ = fs::remove_file(&app_css_out);
        fs::copy(&src, &app_css_out)?;
        return Ok(());
    }

    let input_css = manifest_dir.join("styles/input.css");
    let tailwind_bin = env::var("TAILWIND_BIN")
        .ok()
        .map(|p| resolve_path(&p, &manifest_dir))
        .unwrap_or_else(|| PathBuf::from("tailwindcss"));

    let status = Command::new(&tailwind_bin)
        .current_dir(&manifest_dir)
        .arg("-i")
        .arg(&input_css)
        .arg("-o")
        .arg(&app_css_out)
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => {
            if app_css_out.exists() && fs::metadata(&app_css_out)?.len() > 100 {
                println!(
                    "cargo:warning=tailwindcss exited with status {s}; using pregenerated assets/app.css"
                );
                Ok(())
            } else {
                Err(format!(
                    "tailwindcss exited with error status {s} while compiling styles/input.css"
                )
                .into())
            }
        }
        Err(err) => {
            if app_css_out.exists() && fs::metadata(&app_css_out)?.len() > 100 {
                println!(
                    "cargo:warning=tailwindcss not found ({err}); using pregenerated assets/app.css"
                );
                Ok(())
            } else {
                Err(format!(
                    "tailwindcss is not available ({err}) and no pregenerated app.css found in assets/; \
                     install tailwindcss or set TAILWIND_APP_CSS / TAILWIND_BIN"
                )
                .into())
            }
        }
    }
}
