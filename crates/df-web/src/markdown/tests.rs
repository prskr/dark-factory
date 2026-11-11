use super::{
    ImageArtifacts, highlight_by_extension, render, render_with_hard_breaks, render_with_images,
};

fn html(source: &str) -> String {
    render(source).into_string()
}

#[test]
fn script_is_shown_literally() {
    let out = html("<script>alert(1)</script>");
    assert!(!out.contains("<script>"), "{out}");
    assert!(
        out.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{out}"
    );
}

#[test]
fn image_becomes_a_link() {
    let out = html("![logo](https://example.com/logo.png)");
    assert!(!out.contains("<img"), "{out}");
    assert!(
        out.contains(r#"href="https://example.com/logo.png""#),
        "{out}"
    );
}

#[test]
fn javascript_link_is_dropped() {
    let out = html("[click](javascript:alert(1))");
    assert!(!out.contains("javascript:"), "{out}");
}

#[test]
fn links_carry_rel() {
    let out = html("[a](https://example.com)");
    assert!(
        out.contains(r#"<a rel="noopener noreferrer nofollow" href="https://example.com""#),
        "{out}"
    );
}

#[test]
fn rust_fence_is_highlighted_with_classes() {
    let out = html("```rust\nfn main() { let s = \"x\"; } // c\n```");
    assert!(out.contains(r#"<span class="hl-"#), "{out}");
    assert!(out.contains("hl-keyword"), "{out}");
    assert!(out.contains("hl-string"), "{out}");
    assert!(out.contains("hl-comment"), "{out}");
    assert!(!out.contains("style="), "{out}");
}

#[test]
fn extension_is_a_language() {
    let rs = html("```rs\nfn main() { let x = 1; }\n```");
    let rust = html("```rust\nfn main() { let x = 1; }\n```");
    assert!(rs.contains("hl-keyword"), "{rs}");
    assert_eq!(
        rs.replace("language-rs", "x"),
        rust.replace("language-rust", "x")
    );
}

#[test]
fn fence_without_language_is_plain() {
    let out = html("```\nfn main() {} <b>\n```");
    assert!(!out.contains("<span"), "{out}");
    assert!(out.contains("fn main() {} &lt;b&gt;"), "{out}");
}

#[test]
fn language_is_not_guessed_from_content() {
    let out = html("```\n#!/bin/sh\necho \"x\"\n```");
    assert!(!out.contains("<span"), "{out}");
}

#[test]
fn unknown_language_is_plain() {
    let out = html("```no-such-language\nfn main() {}\n```");
    assert!(!out.contains("<span"), "{out}");
    assert!(out.contains("fn main() {}"), "{out}");
}

#[test]
fn oversized_block_is_plain() {
    let code = "fn f() {}\n".repeat(7_000);
    assert!(code.len() > super::MAX_HIGHLIGHT_BYTES);
    let out = html(&format!("```rust\n{code}```"));
    assert!(!out.contains("<span"), "plain");
}

#[test]
fn markup_inside_a_highlighted_fence_is_escaped() {
    let out = html("```html\n<script>alert(1)</script>\n```");
    assert!(out.contains("<span"), "highlighted: {out}");
    assert!(!out.contains("<script>"), "{out}");
    assert!(out.contains("&lt;"), "{out}");
}

#[test]
fn mermaid_fence_is_an_escaped_plain_block_with_the_component() {
    let out = html("```mermaid\nflowchart LR\nA[\"x\"] --> B<b>\n```");
    assert!(
        out.starts_with(r#"<pre class="hl-code" x-data="mermaidDiagram"><code>flowchart LR"#),
        "{out}"
    );
    assert!(out.contains("A[&quot;x&quot;] --&gt; B&lt;b&gt;"), "{out}");
    assert!(!out.contains("<span") && !out.contains("<b>"), "{out}");
}

#[test]
fn only_mermaid_fences_get_the_component() {
    assert!(!html("```rust\nx\n```").contains("x-data"));
}

#[test]
fn fenced_blocks_carry_the_theme_class() {
    for fence in ["```rust\nx\n```", "```\nx\n```"] {
        assert!(html(fence).contains(r#"<pre class="hl-code""#), "{fence}");
    }
}

#[test]
fn theme_css_has_light_rules_and_dark_rules_in_a_media_query() {
    let css = super::theme_css("InspiredGitHub", "base16-ocean.dark").unwrap();
    let dark = css.find("@media (prefers-color-scheme: dark)").unwrap();
    let (light_part, dark_part) = css.split_at(dark);
    // InspiredGitHub is white, base16-ocean.dark is #2b303b.
    assert!(light_part.contains(".hl-code {"), "{light_part}");
    assert!(
        light_part.contains("background-color: #ffffff"),
        "{light_part}"
    );
    assert!(
        dark_part.contains("background-color: #2b303b"),
        "{dark_part}"
    );
    assert!(dark_part.contains(".hl-keyword"), "{dark_part}");
    assert!(dark_part.trim_end().ends_with('}'));
}

#[tokio::test]
async fn code_theme_route_serves_the_stylesheet_with_immutable_caching() {
    use axum::body::Body;
    use axum::http::{Request, header};
    use tower::ServiceExt;

    let theme = super::CodeTheme::default();
    assert_eq!(theme.url().len(), "/assets/code-theme.css?v=".len() + 8);
    let response = theme
        .routes()
        .oneshot(
            Request::get("/assets/code-theme.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        String::from_utf8(body.to_vec())
            .unwrap()
            .contains(".hl-code")
    );
    // The hash follows the content.
    let other = super::CodeTheme::new("Solarized (light)", "Solarized (dark)").unwrap();
    assert_ne!(other.url(), theme.url());
}

#[test]
fn unknown_theme_names_the_option_and_lists_the_valid_themes() {
    let err = super::theme_css("InspiredGitHub", "Monokai")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("web.codeTheme.dark") && err.contains("Monokai"),
        "{err}"
    );
    assert!(
        err.contains("Solarized (dark)") && err.contains("base16-mocha.dark"),
        "{err}"
    );
    let err = super::theme_css("Nope", "Solarized (dark)")
        .unwrap_err()
        .to_string();
    assert!(err.contains("web.codeTheme.light"), "{err}");
}

#[test]
fn helm_schema_lists_exactly_the_bundled_themes() {
    const SCHEMA: &str = include_str!("../../../../deploy/helm/dark-factory/values.schema.json");
    let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
    let listed: Vec<&str> = schema["$defs"]["codeTheme"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let bundled = syntect::highlighting::ThemeSet::load_defaults();
    let mut bundled: Vec<&str> = bundled.themes.keys().map(String::as_str).collect();
    let mut listed = listed;
    bundled.sort_unstable();
    listed.sort_unstable();
    assert_eq!(listed, bundled);
}

fn with_images(source: &str) -> String {
    let names = vec!["login.png".to_string()];
    render_with_images(
        source,
        &ImageArtifacts {
            task_key: "DF-21",
            names: &names,
        },
    )
    .into_string()
}

#[test]
fn an_image_of_the_same_task_is_shown_inline() {
    let out = with_images("![Login](login.png)");
    assert!(
        out.contains(
            r#"<img loading="lazy" src="/tasks/DF-21/artifacts/login.png/raw" alt="Login""#
        ),
        "{out}"
    );
}

#[test]
fn other_images_stay_links_with_artifact_rendering() {
    for source in [
        "![x](../DF-20/shot.png)",
        "![x](https://example.com/x.png)",
        "![x](other.png)",
        "![x](Login.png)",
        "![x](login%2Epng)",
        "![x](/tasks/DF-20/artifacts/login.png/raw)",
    ] {
        let out = with_images(source);
        assert!(!out.contains("<img"), "{source}: {out}");
        assert!(out.contains("<a rel="), "{source}: {out}");
    }
}

#[test]
fn render_still_turns_every_image_into_a_link() {
    let out = html("![Login](login.png)");
    assert!(
        !out.contains("<img") && out.contains(r#"href="login.png""#),
        "{out}"
    );
}

#[test]
fn text_is_highlighted_by_extension_or_escaped() {
    let rust = highlight_by_extension("rs", "fn main() {}").into_string();
    assert!(rust.contains("hl-"), "{rust}");
    let unknown = highlight_by_extension("zzz", "<b>x</b>").into_string();
    assert_eq!(
        unknown,
        r#"<pre class="hl-code"><code>&lt;b&gt;x&lt;/b&gt;</code></pre>"#
    );
    let markdown = highlight_by_extension("md", "# Title\n`code`").into_string();
    assert!(markdown.contains("hl-"), "{markdown}");
}

fn typed(source: &str) -> String {
    render_with_hard_breaks(source).into_string()
}

#[test]
fn a_single_newline_becomes_a_line_break_only_with_hard_breaks() {
    let out = typed("first\nsecond");
    assert!(out.contains("first<br />\nsecond"), "{out}");
    assert!(!html("first\nsecond").contains("<br"));
}

#[test]
fn several_blank_lines_are_one_paragraph_break() {
    let out = typed("a\n\n\n\nb");
    assert_eq!(out.matches("<p>").count(), 2, "{out}");
    assert!(!out.contains("<br"), "{out}");
}

#[test]
fn hard_breaks_keep_the_untrusted_input_rules() {
    let out = typed("<b>x</b> [click](javascript:alert(1)) ![logo](https://example.com/l.png)");
    assert!(
        out.contains("&lt;b&gt;x&lt;/b&gt;") && !out.contains("<b>"),
        "{out}"
    );
    assert!(!out.contains("javascript:"), "{out}");
    assert!(
        !out.contains("<img") && out.contains(r#"href="https://example.com/l.png""#),
        "{out}"
    );
    assert!(
        out.contains(r#"<a rel="noopener noreferrer nofollow" href=""#),
        "{out}"
    );
}
