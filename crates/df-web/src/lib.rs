//! Dark Factory web UI assets and layout skeleton (`df-web`).

use std::borrow::Cow;

use rust_embed::{EmbeddedFile, RustEmbed};

/// Embedded static assets and compiled `app.css` stylesheet.
#[derive(RustEmbed)]
#[folder = "$OUT_DIR/assets/"]
pub struct Assets;

/// Returns a bundled static asset by relative path.
pub fn get_asset(path: &str) -> Option<EmbeddedFile> {
    Assets::get(path)
}

/// Returns the compiled `app.css` stylesheet embedded at build time.
pub fn app_css() -> Option<Cow<'static, str>> {
    let file = Assets::get("app.css")?;
    match file.data {
        Cow::Borrowed(bytes) => std::str::from_utf8(bytes).ok().map(Cow::Borrowed),
        Cow::Owned(bytes) => String::from_utf8(bytes).ok().map(Cow::Owned),
    }
}

/// Renders the outer shell skeleton containing Tailwind utility classes scanned by ADR-0037.
pub fn shell_placeholder_html(title: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><title>{title}</title><link rel="stylesheet" href="/assets/app.css"></head><body class="min-h-screen bg-slate-950 text-slate-100 grid-cols-[260px_1fr]"><main class="mx-auto max-w-5xl p-6">{title}</main></body></html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_app_css_and_vendor_placeholder() {
        let css = app_css().expect("app.css must be embedded");
        assert!(!css.is_empty());
        assert!(!css.contains("/* fallback empty app.css */"));
        assert!(
            css.contains("tailwindcss")
                || css.contains("grid-cols-")
                || css.contains("bg-slate-950")
        );
        assert!(get_asset("vendor/placeholder.css").is_some());
    }

    #[test]
    fn renders_shell_placeholder() {
        let html = shell_placeholder_html("Dark Factory");
        assert!(html.contains("grid-cols-[260px_1fr]"));
    }

    #[test]
    fn app_css_contains_all_classes_used_in_templates() {
        let css = app_css().expect("app.css must be embedded");
        let required_patterns = [
            "min-h-screen",
            "bg-slate-950",
            "text-slate-100",
            "grid-cols-\\[260px_1fr\\]",
            "mx-auto",
            "max-w-5xl",
            "p-6",
        ];
        for pattern in required_patterns {
            assert!(
                css.contains(pattern),
                "app.css is missing required utility rule `{pattern}`; update assets/app.css"
            );
        }
    }
}
