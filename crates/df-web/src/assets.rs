//! Embedded static assets: vendored htmx/Alpine, compiled Tailwind stylesheet.
//!
//! URLs carry a content hash (`?v=<hash>`), so responses are cached for a year.

use std::borrow::Cow;

use axum::Router;
use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use rust_embed::{EmbeddedFile, RustEmbed};

/// Embedded static assets and compiled `app.css` stylesheet.
#[derive(RustEmbed)]
#[folder = "$OUT_DIR/assets/"]
pub struct Assets;

pub(crate) const CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

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

/// TypeScript sources of the components, for tests that check a component's code. Unlike
/// [`Assets`], they exist in every build, with or without `swc`.
#[cfg(test)]
#[derive(RustEmbed)]
#[folder = "scripts/"]
struct ScriptSources;

/// Source of a component, `script_source("tabs.ts")`.
#[cfg(test)]
pub(crate) fn script_source(path: &str) -> String {
    let file = ScriptSources::get(path).unwrap_or_else(|| panic!("{path} exists in scripts/"));
    String::from_utf8(file.data.into_owned()).unwrap()
}

/// URL of an asset including its content hash, `/assets/app.css?v=1a2b3c4d`.
pub fn asset_url(path: &str) -> String {
    match Assets::get(path) {
        Some(file) => format!(
            "/assets/{path}?v={}",
            hex::encode(&file.metadata.sha256_hash()[..4])
        ),
        None => format!("/assets/{path}"),
    }
}

/// Route `GET /assets/{*path}`.
pub fn routes() -> Router {
    Router::new().route("/assets/{*path}", get(serve_asset))
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

async fn serve_asset(Path(path): Path<String>) -> Response {
    let Some(file) = Assets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let etag = format!("\"{}\"", hex::encode(file.metadata.sha256_hash()));
    let mut response = file.data.into_owned().into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&path)),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_CONTROL),
    );
    if let Ok(etag) = HeaderValue::from_str(&etag) {
        headers.insert(header::ETAG, etag);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn get_path(path: &str) -> Response {
        routes()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    fn radios_and_checkboxes_use_the_accent_text_color() {
        let css = app_css().expect("app.css is embedded");
        let rule = css
            .find("accent-color:var(--accent-text)")
            .map(|at| &css[..at])
            .and_then(|head| head.rsplit('{').nth(1))
            .unwrap_or_default();
        assert!(
            rule.contains("input[type=radio]") && rule.contains("input[type=checkbox]"),
            "{rule}"
        );
    }

    #[test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    fn embeds_the_stylesheet() {
        assert!(!app_css().expect("app.css is embedded").is_empty());
    }

    /// The file `file` of the npm package `package`: Bazel's copy (`VENDOR_SCRIPTS`, set by the
    /// test target) or the installed package of the crate.
    fn package_file(package: &str, file: &str) -> std::path::PathBuf {
        let suffix = std::path::Path::new(package).join(file);
        match std::env::var("VENDOR_SCRIPTS") {
            Ok(list) => list
                .split_whitespace()
                .map(std::path::PathBuf::from)
                .find(|path| path.ends_with(&suffix))
                .unwrap_or_else(|| panic!("{package}/{file} is not in VENDOR_SCRIPTS")),
            Err(_) => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("node_modules")
                .join(suffix),
        }
    }

    #[test]
    #[cfg_attr(
        not(df_web_vendor),
        ignore = "needs third-party scripts: pnpm install --frozen-lockfile in crates/df-web, or VENDOR_SCRIPTS"
    )]
    fn vendored_scripts_are_embedded_byte_for_byte() {
        for (served, package, file) in crate::build_assets::VENDOR {
            let embedded = get_asset(served).unwrap_or_else(|| panic!("{served} is embedded"));
            let published = std::fs::read(package_file(package, file)).unwrap();
            assert!(
                embedded.data.as_ref() == published.as_slice(),
                "{served} differs from {package}/{file}"
            );
        }
    }

    #[test]
    #[cfg_attr(
        not(df_web_vendor),
        ignore = "needs third-party scripts: pnpm install --frozen-lockfile in crates/df-web, or VENDOR_SCRIPTS"
    )]
    fn installed_packages_have_the_versions_of_package_json() {
        let read = |path: std::path::PathBuf| -> serde_json::Value {
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
        };
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../package.json")).unwrap();
        for (package, pinned) in manifest["dependencies"].as_object().unwrap() {
            let installed = read(package_file(package, "package.json"));
            assert_eq!(&installed["version"], pinned, "{package}");
        }
    }

    #[test]
    #[cfg_attr(
        not(df_web_scripts),
        ignore = "needs compiled scripts: swc on PATH (bazel run //:bazel_env)"
    )]
    fn compiled_components_are_embedded_without_typescript_syntax() {
        for script in crate::layout::COMPONENT_SCRIPTS {
            let file = get_asset(script).unwrap_or_else(|| panic!("{script} is embedded"));
            let js = std::str::from_utf8(&file.data).unwrap();
            assert!(js.contains("Alpine.data("), "{script}");
        }
        let tabs = String::from_utf8(get_asset("tabs.js").unwrap().data.into_owned()).unwrap();
        // The source has `initial: string` and `move(event: KeyboardEvent): void`.
        for typescript in [": string", ": void", ": KeyboardEvent", "as HTMLElement"] {
            assert!(!tabs.contains(typescript), "{typescript} in {tabs}");
        }
    }

    #[tokio::test]
    async fn logo_is_served_as_svg_with_cache_headers_and_matches_the_rail_icon() {
        let response = get_path("/assets/logo.svg").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
        assert_eq!(response.headers()[header::CACHE_CONTROL], CACHE_CONTROL);
        let file = get_asset("logo.svg").expect("logo embedded");
        let svg = std::str::from_utf8(&file.data).unwrap();
        let rail_logo = crate::components::logo("x").into_string();
        let paths =
            &rail_logo[rail_logo.find("<path").unwrap()..rail_logo.rfind("</svg>").unwrap()];
        assert!(svg.contains(paths), "logo.svg and Icon::Logo differ");
    }

    #[tokio::test]
    #[cfg_attr(
        not(df_web_scripts),
        ignore = "needs compiled scripts: swc on PATH (bazel run //:bazel_env)"
    )]
    async fn the_component_scripts_are_served_as_javascript() {
        for script in crate::layout::COMPONENT_SCRIPTS {
            let response = get_path(&format!("/assets/{script}")).await;
            assert_eq!(response.status(), StatusCode::OK, "{script}");
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/javascript; charset=utf-8"
            );
        }
    }

    #[test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    fn stylesheet_contains_only_classes_from_the_sources() {
        // Tailwind's automatic source detection scans the working directory, which in a Bazel
        // sandbox is the whole execroot (~90 KB of CSS). Minified with `@source "../src"` only,
        // the stylesheet is about 25 KB (51 KB with the pages' variants, among them the row
        // cover's `after:` and `focus-visible:after:` classes).
        let css = app_css().expect("app.css is embedded");
        assert!(css.len() < 52_000, "app.css is {} bytes", css.len());
    }

    #[test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    fn stylesheet_covers_the_classes_the_pages_use() {
        let css = app_css().expect("app.css is embedded");
        for rule in [
            ".min-h-11",
            ".inline-flex",
            ".hidden",
            "md\\:flex",
            "md\\:hidden",
            ".md-body",
            ".overflow-x-auto",
        ] {
            assert!(
                css.contains(rule),
                "app.css lacks `{rule}`; regenerate assets/app.css with tailwindcss"
            );
        }
    }

    #[test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    fn stylesheet_defines_the_tokens_for_both_themes() {
        let css = app_css().expect("app.css is embedded");
        assert!(css.contains("--app:#fafaf9"), "light tokens");
        let dark = css
            .find("@media (prefers-color-scheme:dark)")
            .expect("dark theme block");
        assert!(css[dark..].contains("--app:#0c0a09"), "dark tokens");
        for utility in [
            ".bg-app",
            ".bg-panel",
            ".text-ink",
            ".border-line",
            ".bg-accent",
        ] {
            assert!(css.contains(utility), "{utility}");
        }
    }

    #[test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    fn transcript_on_a_phone_hides_the_bottom_bar() {
        let css = app_css().expect("app.css is embedded");
        assert!(
            css.contains(r"\[body\[data-view\=task\]_\&\]\:hidden"),
            "message input takes its place"
        );
        assert!(css.contains("focus-visible"), "visible focus ring");
        // The accent-text ring is too dark on the rail (2.5:1); the rail uses the accent fill.
        assert!(
            css.contains(".bg-rail :focus-visible{outline-color:var(--accent)}"),
            "rail focus ring"
        );
    }

    #[test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    fn stylesheet_refers_to_the_control_plane_only() {
        let css = app_css().expect("app.css is embedded");
        assert!(!css.contains("url(http") && !css.contains("@import"));
        assert!(css.contains("url(/assets/fonts/inter-latin-wght-normal.woff2)"));
    }

    #[tokio::test]
    async fn fonts_are_served_with_long_cache_headers_and_their_licences_are_embedded() {
        for font in [
            "fonts/inter-latin-wght-normal.woff2",
            "fonts/jetbrains-mono-latin-wght-normal.woff2",
        ] {
            let response = get_path(&format!("/assets/{font}")).await;
            assert_eq!(response.status(), StatusCode::OK, "{font}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], CACHE_CONTROL);
            assert_eq!(response.headers()[header::CONTENT_TYPE], "font/woff2");
        }
        for licence in ["fonts/LICENSE-Inter.txt", "fonts/LICENSE-JetBrainsMono.txt"] {
            assert!(get_asset(licence).is_some(), "{licence}");
        }
    }

    #[tokio::test]
    #[cfg_attr(
        not(df_web_vendor),
        ignore = "needs third-party scripts: pnpm install --frozen-lockfile in crates/df-web, or VENDOR_SCRIPTS"
    )]
    async fn assets_are_served_with_long_cache_headers() {
        let response = get_path("/assets/vendor/htmx.min.js").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], CACHE_CONTROL);
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/javascript")
        );
        assert!(response.headers().contains_key(header::ETAG));
    }

    #[tokio::test]
    async fn unknown_asset_is_404() {
        assert_eq!(
            get_path("/assets/nope.js").await.status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn markdown_component_keeps_diagrams_isolated() {
        let js = script_source("markdown.ts");
        assert!(js.contains("Alpine.data('mermaidDiagram'"));
        assert!(js.contains("securityLevel: 'sandbox'"));
        // `%%{init}%%` in the diagram source must not be able to lower the level.
        assert!(js.contains("secure: SECURE"), "{js}");
        assert!(js.contains("'securityLevel',"), "{js}");
        // The source reaches the page as text only; innerHTML takes Mermaid's output.
        assert!(js.contains("textContent"));
        for line in js
            .lines()
            .filter(|l| l.contains("innerHTML") && !l.trim_start().starts_with("//"))
        {
            assert!(
                line.contains("svg") && !line.contains("source"),
                "innerHTML from something else than the render result: {line}"
            );
        }
        assert!(
            js.contains("removeEventListener"),
            "listener removed in destroy"
        );
    }

    #[test]
    fn markdown_component_says_so_when_the_renderer_is_missing_or_fails_to_load() {
        let js = script_source("markdown.ts");
        assert!(js.contains("Diagram renderer not available"), "{js}");
        // No `data-mermaid-src` (a build without Mermaid) fails at once, a failed load the same.
        assert!(js.contains("dataset.mermaidSrc"), "{js}");
        assert!(js.contains("script.onerror"), "{js}");
        assert!(js.contains("RendererUnavailable"), "{js}");
    }

    #[test]
    fn markdown_component_restores_its_diagram_after_a_morph() {
        let js = script_source("markdown.ts");
        // A morph resets `hidden` on the `<pre>` and removes the sibling diagram: the component
        // watches both and puts the current view back, without creating a second diagram.
        assert!(js.contains("new MutationObserver"), "{js}");
        assert!(js.contains("attributeFilter: ['hidden']"), "{js}");
        assert!(js.contains("{ childList: true }"), "{js}");
        assert!(
            js.contains("isConnected"),
            "creates a sibling only when it is gone"
        );
        assert!(
            js.contains("this.resizer?.unobserve(element)"),
            "replaced diagrams are unobserved"
        );
        assert!(
            js.contains("this.observer?.disconnect()"),
            "observer ends with the component"
        );
    }

    #[test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    fn asset_url_carries_a_content_hash() {
        let url = asset_url("app.css");
        assert!(url.starts_with("/assets/app.css?v="), "{url}");
        assert_eq!(url.len(), "/assets/app.css?v=".len() + 8);
    }
}
