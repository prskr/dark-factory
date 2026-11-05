use super::render;

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
