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
        // The third-party scripts only: Bazel copies the files of these packages (the CodeMirror
        // packages are bundled by `:editor_bundle`).
        let vendored =
            |package: &String| crate::build_assets::VENDOR.iter().any(|v| v.1 == package);
        for (package, pinned) in manifest["dependencies"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(package, _)| vendored(package))
        {
            let path = package_file(package, "package.json");
            let installed = read(path.clone());
            assert_eq!(
                &installed["version"],
                pinned,
                "{package}: {} is out of date with package.json; run `pnpm install \
                 --frozen-lockfile` in crates/df-web, or update the VENDOR_SCRIPTS source",
                path.display()
            );
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
        // the stylesheet is about 25 KB. With the pages' variants it is about 59 KB, under the
        // 60 KB cap below: the row cover's `after:` and `focus-visible:after:` classes, the
        // Subtasks tab and its chip, the tool call groups, the tool details, the persona
        // avatar palette and sizes and the Markdown field.
        let css = app_css().expect("app.css is embedded");
        assert!(css.len() < 60_000, "app.css is {} bytes", css.len());
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

    /// The editor bundle is the one asset that needs `esbuild` and the `CodeMirror` packages.
    const NO_EDITOR: &str = "needs the editor bundle: esbuild on PATH and pnpm install --frozen-lockfile in crates/df-web (bazel run //:bazel_env)";

    #[tokio::test]
    #[cfg_attr(not(df_web_editor), ignore = "needs the editor bundle")]
    async fn the_editor_bundle_is_served_with_a_content_hash_and_defines_the_editor() {
        let url = asset_url("editor.js");
        assert!(url.starts_with("/assets/editor.js?v="), "{url}");
        assert_eq!(url.len(), "/assets/editor.js?v=".len() + 8, "{url}");

        let response = get_path("/assets/editor.js").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], CACHE_CONTROL);
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/javascript")
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let js = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            js.contains("DfMarkdownEditor"),
            "the bundle defines the global"
        );
    }

    #[test]
    #[cfg_attr(not(df_web_editor), ignore = "needs the editor bundle")]
    fn the_editor_bundle_stays_below_300_kib_gzip() {
        use std::io::Write;
        const LIMIT: usize = 300 * 1024;
        let bundle = get_asset("editor.js").unwrap_or_else(|| panic!("{NO_EDITOR}"));
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        gzip.write_all(&bundle.data).unwrap();
        let size = gzip.finish().unwrap().len();
        assert!(
            size < LIMIT,
            "editor.js is {size} bytes with gzip level 9, the limit is {LIMIT} bytes (300 KiB)"
        );
    }

    #[test]
    fn editor_headings_take_the_function_name_color_of_the_code_theme_and_markers_stay_faint() {
        let source = include_str!("../editor/markdown-editor.ts");
        let heading = source
            .lines()
            .find(|line| line.contains("tag: tags.heading"))
            .unwrap_or_else(|| panic!("no style for headings: {source}"));
        // The classes of syntect's function-name scope: `/assets/code-theme.css` colors them.
        assert!(
            heading.contains("class: 'hl-entity hl-name hl-function cm-md-heading'"),
            "{heading}"
        );
        assert!(!heading.contains("color:"), "{heading}");
        // The theme rule is stronger than the code theme's own (it makes the function name bold),
        // so a heading stays semibold.
        assert!(
            source.contains("'.cm-md-heading': { fontWeight: '600 !important' }"),
            "{source}"
        );
        // The markers (`#`, `-`, backticks, ...) keep the token color, also inside a heading,
        // where the code theme's color would otherwise win by specificity.
        let markers = source
            .lines()
            .find(|line| line.contains("tags.processingInstruction"))
            .unwrap_or_else(|| panic!("no style for markers: {source}"));
        assert!(markers.contains("class: 'cm-md-marker'"), "{markers}");
        assert!(
            source.contains("'.cm-md-marker': { color: 'var(--faint) !important' }"),
            "{source}"
        );
    }

    #[test]
    fn only_inline_code_gets_the_sunk_background_in_the_editor_not_the_body_of_a_fence() {
        let source = include_str!("../editor/markdown-editor.ts");
        // `tags.monospace` also styles the body of a fence without a highlighter (`mermaid`).
        let monospace = source
            .lines()
            .find(|line| line.contains("tag: tags.monospace"))
            .map_or("", |line| line);
        assert!(!monospace.contains("backgroundColor"), "{monospace}");
        // A tag of its own for `InlineCode` carries the background.
        assert!(
            source.contains("styleTags({ InlineCode: inlineCode })"),
            "{source}"
        );
        let inline = source
            .lines()
            .find(|line| line.contains("tag: inlineCode"))
            .unwrap_or_else(|| panic!("no style for inline code: {source}"));
        assert!(
            inline.contains("backgroundColor: 'var(--sunk)'"),
            "{inline}"
        );
    }

    #[test]
    fn markdown_field_opens_on_split_only_from_1280_px_and_falls_back_to_the_editor() {
        let js = script_source("markdownField.ts");
        assert!(js.contains("const WIDE = '(min-width: 1280px)'"), "{js}");
        assert!(
            js.contains("this.view = options.split && this.wide.matches ? 'split' : 'editor'"),
            "{js}"
        );
        // Narrowing below 1280 px while "Split" is shown selects "Editor".
        assert!(
            js.contains("if (this.view === 'split' && !this.wide?.matches) this.select('editor')"),
            "{js}"
        );
        // The view is component state only: the one stored value is the "Highlighting" switch.
        assert!(!js.contains("history."), "{js}");
        for line in js
            .lines()
            .filter(|l| l.contains("localStorage") && !l.trim_start().starts_with("//"))
        {
            assert!(line.contains("PLAIN_KEY"), "{line}");
        }
    }

    #[test]
    fn markdown_field_switch_has_its_own_radio_group_keys() {
        let js = script_source("markdownField.ts");
        for key in ["ArrowLeft", "ArrowRight", "Home", "End"] {
            assert!(js.contains(key), "{key}");
        }
        assert!(js.contains("move(event: KeyboardEvent)"), "{js}");
        assert!(js.contains("[role=\"radio\"]"), "{js}");
    }

    #[test]
    fn question_other_shows_and_focuses_its_input_while_other_is_checked() {
        let js = script_source("questionOther.ts");
        assert!(
            js.contains("Alpine.data('questionOther', (otherValue: string)"),
            "{js}"
        );
        // The form value of Other comes from the server, not from a literal in the script.
        assert!(js.contains("input[value=\"${otherValue}\"]"), "{js}");
        assert!(!js.contains("_other"), "{js}");
        assert!(js.contains("this.open = Boolean(other?.checked)"), "{js}");
        // Focus only when Other just got checked by a click, not while the component starts or
        // when the Other input itself fires `change` on blur.
        assert!(
            js.contains("if (wasOpen || !this.open || !focus) return;"),
            "{js}"
        );
        assert!(js.contains("[data-other-input]')?.focus()"), "{js}");
        // Hiding is a binding on the server markup (`:hidden`), so without the component the
        // input stays visible.
        assert!(!js.contains("style.display"), "{js}");
    }

    #[test]
    fn request_cards_insert_replace_move_the_old_form_and_delete() {
        let js = script_source("requestCards.ts");
        assert!(js.contains("Alpine.data('requestCards'"), "{js}");
        // Insert before the anchor, else at the end of the list.
        assert!(js.contains("anchor.before(card)"), "{js}");
        assert!(js.contains(".append(card)"), "{js}");
        // Replace; the old form moves only when both cards are pending with the same question.
        assert!(js.contains("old.replaceWith(card)"), "{js}");
        assert!(
            js.contains(
                "if (old.dataset.state !== 'pending' || card.dataset.state !== 'pending') return none;"
            ),
            "{js}"
        );
        assert!(
            js.contains("oldForm.dataset.question !== newForm.dataset.question) return none;"),
            "{js}"
        );
        assert!(js.contains("newForm.replaceWith(oldForm)"), "{js}");
        // The moved form gets its focus and selection back once the card is on the page.
        assert!(js.contains("field?.focus()"), "{js}");
        assert!(js.contains("setSelectionRange(start, end)"), "{js}");
        // A keep template removes the list's cards it does not name.
        assert!(js.contains("dataset.requestKeep"), "{js}");
        assert!(
            js.contains("if (!keep.has(card.id)) card.remove();"),
            "{js}"
        );
        // Inserted cards are processed by htmx, or their forms would not post through it.
        assert_eq!(js.matches("htmx.process(card)").count(), 2, "{js}");
        // Delete.
        assert!(js.contains("dataset.requestDelete"), "{js}");
        assert!(js.contains("?.remove()"), "{js}");
    }

    #[test]
    fn markdown_field_plain_tabs_move_with_the_arrow_keys_and_never_attach_the_editor() {
        let js = script_source("markdownField.ts");
        // The same keys serve the radio group of the switch and the tabs of the plain layout.
        assert!(js.contains("[role=\"radio\"], [role=\"tab\"]"), "{js}");
        // The marked field leaves out everything about the highlighting editor.
        assert!(
            js.contains("options: { split?: boolean; plain?: boolean }"),
            "{js}"
        );
        assert!(
            js.contains("if (!options.plain) this.initEditor(this.textarea)"),
            "{js}"
        );
        assert!(js.contains("options.split && this.wide.matches"), "{js}");
    }

    #[test]
    fn markdown_field_previews_with_a_plain_fetch_of_only_its_text() {
        let js = script_source("markdownField.ts");
        assert!(js.contains("new URLSearchParams({ source: text })"), "{js}");
        assert!(js.contains("headers: { 'HX-Request': 'true' }"), "{js}");
        assert!(js.contains("redirect: 'manual'"), "{js}");
        assert!(js.contains("signal: controller.signal"), "{js}");
        assert!(!js.contains("htmx.ajax"), "{js}");
    }

    #[test]
    fn markdown_field_debounces_aborts_and_skips_unchanged_text() {
        let js = script_source("markdownField.ts");
        assert!(js.contains("const DEBOUNCE_MS = 500"), "{js}");
        assert!(
            js.contains("window.setTimeout(() => void this.refresh(), DEBOUNCE_MS)"),
            "{js}"
        );
        // Only "Split" follows the text; the other views request on opening.
        assert!(js.contains("if (this.view !== 'split') return;"), "{js}");
        // One request in flight: a newer one aborts the older, an older response is dropped.
        assert!(js.contains("this.pending?.abort()"), "{js}");
        assert!(
            js.contains("if (this.pending !== controller) return;"),
            "{js}"
        );
        // The server-rendered first preview counts as rendered; the same text needs no request.
        assert!(
            js.contains("this.rendered = this.textarea.defaultValue"),
            "{js}"
        );
        assert!(
            js.contains("if (!force && this.status === 'ok' && text === this.rendered) return;"),
            "{js}"
        );
    }

    #[test]
    fn markdown_field_swaps_only_a_200_and_names_the_failures() {
        let js = script_source("markdownField.ts");
        assert!(js.contains("response.status === 200"), "{js}");
        assert!(
            js.contains("response.status === 413) status = 'too-long'"),
            "{js}"
        );
        assert!(
            js.contains("response.status === 401) status = 'session'"),
            "{js}"
        );
        // The swap happens only in the 200 branch: `html` stays null otherwise.
        assert!(js.contains("if (html !== null) {"), "{js}");
        assert!(js.contains("swap: 'innerMorph'"), "{js}");
        assert!(js.contains("hx-morph-skip"), "{js}");
    }

    #[test]
    fn markdown_field_loads_the_editor_once_from_the_body_and_keeps_the_textarea_when_it_fails() {
        let js = script_source("markdownField.ts");
        assert!(js.contains("document.body.dataset.editorSrc"), "{js}");
        // One load per page, like Mermaid; a failed load can be tried again by the next field.
        assert!(js.contains("editorLoading ??="), "{js}");
        assert!(js.contains("script.onerror"), "{js}");
        assert!(js.contains("editorLoading = null"), "{js}");
        // The failure takes the editor and its switch away; the textarea stays the field.
        assert!(js.contains("this.canEdit = false"), "{js}");
        assert!(
            js.contains("window.DfMarkdownEditor.attach(textarea"),
            "{js}"
        );
    }

    #[test]
    fn markdown_field_attaches_the_editor_from_768_px_on_every_device_unless_switched_off() {
        let js = script_source("markdownField.ts");
        assert!(js.contains("const DESKTOP = '(min-width: 768px)'"), "{js}");
        // The pointer type decides neither the editor nor the layout: no pointer media query.
        assert!(!js.contains("pointer"), "{js}");
        assert!(js.contains("const PLAIN_KEY = 'df.editor.plain'"), "{js}");
        assert!(js.contains("Boolean(src) && this.desktop.matches"), "{js}");
        // Narrowing under 768 px takes the editor away, widening brings it back.
        assert!(
            js.contains("this.desktop.addEventListener('change'"),
            "{js}"
        );
        // `localStorage` can throw (private windows, blocked site data).
        assert!(js.contains("try {") && js.contains("localStorage"), "{js}");
    }

    #[test]
    fn markdown_field_highlighting_switch_stores_the_choice_and_swaps_editor_and_textarea() {
        let js = script_source("markdownField.ts");
        assert!(js.contains("toggle(): void"), "{js}");
        assert!(js.contains("localStorage.setItem(PLAIN_KEY"), "{js}");
        assert!(js.contains("localStorage.removeItem(PLAIN_KEY"), "{js}");
        assert!(js.contains("this.editor.destroy()"), "{js}");
        // The live preview listens to the textarea: the editor fires `input` on it, so there is
        // one listener for both.
        assert!(
            js.contains("this.textarea.addEventListener('input'"),
            "{js}"
        );
    }

    #[test]
    fn markdown_field_hint_names_cmd_on_macos_and_the_editor_describes_itself_with_it() {
        let js = script_source("markdownField.ts");
        assert!(js.contains("'Cmd'") && js.contains("'Ctrl'"), "{js}");
        assert!(js.contains("describedBy: hint ? [hint.id] : []"), "{js}");
        assert!(js.contains("[data-indent-hint]"), "{js}");
    }

    #[test]
    fn markdown_field_grows_the_textarea_where_field_sizing_is_missing() {
        let js = script_source("markdownField.ts");
        assert!(
            js.contains("CSS.supports('field-sizing', 'content')"),
            "{js}"
        );
        assert!(js.contains("new IntersectionObserver"), "{js}");
        assert!(js.contains("window.innerHeight * MAX_HEIGHT"), "{js}");
    }

    #[test]
    fn markdown_field_fallback_sizes_the_textarea_to_its_content_in_split_too() {
        let js = script_source("markdownField.ts");
        // In Split the column caps the height and scrolls, so the field takes its content height;
        // elsewhere the 70 % cap applies.
        assert!(
            js.contains("this.view === 'split' ? content : Math.min(content, window.innerHeight * MAX_HEIGHT)"),
            "{js}"
        );
        // The measure is taken without the column-filling minimum, so a shorter text shrinks it.
        assert!(js.contains("el.style.minHeight = '0'"), "{js}");
        // Entering Split fits the field like entering Editor.
        assert!(
            js.contains("if (view !== 'preview') void this.$nextTick(() => this.fit())"),
            "{js}"
        );
    }

    #[test]
    fn markdown_component_keeps_a_drawn_diagram_across_a_morph_and_redraws_a_changed_source() {
        let js = script_source("markdown.ts");
        // The morph skips the diagram element, so an unchanged fence keeps its iframe.
        assert!(js.contains("setAttribute('hx-morph-skip', '')"), "{js}");
        // The fence is drawn again only when the text of the `<pre>` changes.
        assert!(js.contains("characterData: true"), "{js}");
        assert!(js.contains("text !== this.source"), "{js}");
        assert!(
            js.contains("this.sourceObserver?.disconnect()"),
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
