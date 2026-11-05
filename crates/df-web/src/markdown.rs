//! Markdown to HTML for text that agents write: the transcript now, artifacts later.
//!
//! Fenced code blocks with a known language are highlighted by syntect with CSS classes
//! (prefix `hl-`); the colors come from the theme stylesheet ([`CodeTheme`]), so the HTML does
//! not depend on the configured themes.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, LazyLock};

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::response::IntoResponse;
use axum::routing::get;

use comrak::adapters::{CodefenceRendererAdapter, SyntaxHighlighterAdapter};
use comrak::nodes::Sourcepos;
use maud::{Markup, PreEscaped, html};
use sha2::{Digest, Sha256};
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Fenced blocks above this size are shown as plain text; highlighting costs CPU per page view.
const MAX_HIGHLIGHT_BYTES: usize = 64 * 1024;
/// Class prefix of the highlight spans and of the theme stylesheet.
const CLASS_PREFIX: &str = "hl-";
/// Info string of fences that the browser draws as diagrams.
const MERMAID: &str = "mermaid";
/// Path of the code theme stylesheet; the URL in pages adds `?v=<hash>`.
const CODE_THEME_PATH: &str = "/assets/code-theme.css";

static HIGHLIGHTER: LazyLock<Highlighter> = LazyLock::new(|| Highlighter {
    syntaxes: SyntaxSet::load_defaults_newlines(),
});

/// Renders Markdown without raw HTML: markup in the source is escaped, unsafe link schemes are
/// dropped. The text is untrusted, so the browser must not fetch anything by itself: images
/// become plain links and every link carries `rel="noopener noreferrer nofollow"`.
pub fn render(source: &str) -> Markup {
    let mut options = comrak::Options::default();
    options.render.escape = true;
    options.render.r#unsafe = false;
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, source, &options);
    for node in root.descendants() {
        let mut data = node.data.borrow_mut();
        if let comrak::nodes::NodeValue::Image(link) = &data.value {
            data.value = comrak::nodes::NodeValue::Link(link.clone());
        }
    }
    let mut plugins = comrak::options::Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&*HIGHLIGHTER);
    plugins
        .render
        .codefence_renderers
        .insert(MERMAID.to_owned(), &MermaidFence);
    let mut html = String::new();
    comrak::format_html_with_plugins(root, &options, &mut html, &plugins)
        .expect("formatting into a String cannot fail");
    PreEscaped(html.replace("<a href=", "<a rel=\"noopener noreferrer nofollow\" href="))
}

/// A configured code theme is not one of syntect's bundled themes.
#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("unknown code theme {name:?} in {option}; valid themes: {valid}")]
    Unknown {
        option: &'static str,
        name: String,
        valid: String,
    },
    #[error("cannot generate the stylesheet of code theme {name:?}")]
    Css {
        name: String,
        source: syntect::Error,
    },
}

/// The stylesheet of the highlight classes: the `light` theme as is, the `dark` theme inside
/// `prefers-color-scheme: dark`. Fenced blocks (`pre.hl-code`) take their theme's colors and
/// background from it. Names are those of syntect's bundled themes; an unknown name is an error
/// that names the config option.
fn theme_css(light: &str, dark: &str) -> Result<String, ThemeError> {
    let themes = ThemeSet::load_defaults();
    let css = |option, name: &str| {
        let theme = themes.themes.get(name).ok_or_else(|| ThemeError::Unknown {
            option,
            name: name.to_owned(),
            valid: themes.themes.keys().cloned().collect::<Vec<_>>().join(", "),
        })?;
        css_for_theme_with_class_style(
            theme,
            ClassStyle::SpacedPrefixed {
                prefix: CLASS_PREFIX,
            },
        )
        .map_err(|source| ThemeError::Css {
            name: name.to_owned(),
            source,
        })
    };
    let (light, dark) = (
        css("web.codeTheme.light", light)?,
        css("web.codeTheme.dark", dark)?,
    );
    Ok(format!(
        "{light}\n@media (prefers-color-scheme: dark) {{\n{dark}}}\n"
    ))
}

/// The generated code theme stylesheet with its content-hashed URL. Built once at startup from
/// `web.codeTheme`.
#[derive(Debug, Clone)]
pub struct CodeTheme {
    css: Arc<str>,
    url: String,
}

impl CodeTheme {
    /// Generates the stylesheet; fails for a name outside the bundled themes.
    pub fn new(light: &str, dark: &str) -> Result<Self, ThemeError> {
        let css = theme_css(light, dark)?;
        let hash: String = hex::encode(Sha256::digest(css.as_bytes()))
            .chars()
            .take(8)
            .collect();
        Ok(Self {
            css: css.into(),
            url: format!("{CODE_THEME_PATH}?v={hash}"),
        })
    }

    /// URL for the `<link>` in pages, including the content hash.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Route `GET /assets/code-theme.css`, cached like the embedded assets.
    pub fn routes(&self) -> Router {
        let css = self.css.clone();
        Router::new().route(
            CODE_THEME_PATH,
            get(move || async move {
                (
                    [
                        (
                            header::CONTENT_TYPE,
                            HeaderValue::from_static("text/css; charset=utf-8"),
                        ),
                        (
                            header::CACHE_CONTROL,
                            HeaderValue::from_static(crate::assets::CACHE_CONTROL),
                        ),
                    ],
                    css.to_string(),
                )
                    .into_response()
            }),
        )
    }
}

#[cfg(test)]
impl Default for CodeTheme {
    /// The default themes (`InspiredGitHub`, `base16-ocean.dark`).
    fn default() -> Self {
        let defaults = df_core::config::CodeThemes::default();
        Self::new(&defaults.light, &defaults.dark).expect("default themes are bundled")
    }
}

/// Highlights fenced code by its language token. Only the language decides: no fallback to the
/// first line or the content.
struct Highlighter {
    syntaxes: SyntaxSet,
}

impl Highlighter {
    fn highlight(&self, lang: &str, code: &str) -> Option<String> {
        if lang.is_empty() || code.len() > MAX_HIGHLIGHT_BYTES {
            return None;
        }
        let syntax = self.syntaxes.find_syntax_by_token(lang)?;
        let mut generator = ClassedHTMLGenerator::new_with_class_style(
            syntax,
            &self.syntaxes,
            ClassStyle::SpacedPrefixed {
                prefix: CLASS_PREFIX,
            },
        );
        for line in LinesWithEndings::from(code) {
            // A parse error deliberately falls back to plain escaped text: the block is shown
            // unhighlighted rather than failing the message.
            generator
                .parse_html_for_line_which_includes_newline(line)
                .ok()?;
        }
        Some(generator.finalize())
    }
}

impl SyntaxHighlighterAdapter for Highlighter {
    fn write_highlighted(
        &self,
        output: &mut dyn fmt::Write,
        lang: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        match self.highlight(lang.unwrap_or_default(), code) {
            Some(highlighted) => output.write_str(&highlighted),
            None => write!(output, "{}", html! { (code) }.into_string()),
        }
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn fmt::Write,
        _attributes: HashMap<&'static str, std::borrow::Cow<'_, str>>,
    ) -> fmt::Result {
        output.write_str(r#"<pre class="hl-code">"#)
    }

    fn write_code_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, std::borrow::Cow<'_, str>>,
    ) -> fmt::Result {
        comrak::html::write_opening_tag(output, "code", attributes)
    }
}

/// A `mermaid` fence stays a plain code block (no JavaScript, or a failed diagram); the
/// `mermaidDiagram` Alpine component (`assets/markdown.js`) replaces it by the drawn diagram.
struct MermaidFence;

impl CodefenceRendererAdapter for MermaidFence {
    fn write(
        &self,
        output: &mut dyn fmt::Write,
        _lang: &str,
        _meta: &str,
        code: &str,
        _sourcepos: Option<Sourcepos>,
    ) -> fmt::Result {
        let block = html! {
            pre class="hl-code" x-data="mermaidDiagram" { code { (code) } }
        };
        writeln!(output, "{}", block.into_string())
    }
}

#[cfg(test)]
mod tests;
