//! Task artifacts: names, content types and the limits both the control plane and the sandbox
//! runtime need (spec `task-artifacts`, design D2). A leaf crate: it depends only on `image`
//! (header reads) and `thiserror`. `df-core` adds the configured limits (`check_artifact`).

use std::fmt;
use std::io::Cursor;

use image::{ImageFormat, ImageReader};
use thiserror::Error;

/// Longest artifact name.
pub const MAX_NAME_CHARS: usize = 100;

/// Largest width and height of an image artifact in pixels. It follows the model APIs, so it is
/// no option.
pub const MAX_IMAGE_SIDE: u32 = 8000;

/// What the runtime reads from a file for `write_artifact`, and the largest value of
/// `artifacts.maxTextBytes` and `artifacts.maxImageBytes`.
pub const MAX_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024;

/// The kind of an artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Text,
    Image,
}

impl ArtifactKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
        }
    }
}

impl fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What [`sniff`] found in a piece of content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sniffed {
    pub kind: ArtifactKind,
    pub mime_type: &'static str,
    /// Pixel size, for images.
    pub dimensions: Option<(u32, u32)>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ArtifactError {
    #[error(
        "invalid artifact name `{0}`: use 1 to {MAX_NAME_CHARS} characters from A-Z, a-z, 0-9, `.`, `_` and `-`, not starting with `.`"
    )]
    InvalidName(String),
    #[error(
        "unsupported content: an artifact is UTF-8 text without NUL bytes, or a PNG, JPEG, WebP or GIF image ({0})"
    )]
    Unsupported(&'static str),
    #[error("the text is {size} bytes, over the limit of {limit} (artifacts.maxTextBytes)")]
    TextTooLarge { size: u64, limit: String },
    #[error("the image is {size} bytes, over the limit of {limit} (artifacts.maxImageBytes)")]
    ImageTooLarge { size: u64, limit: String },
    #[error("the image is {width} x {height} px, over the limit of {MAX_IMAGE_SIDE} px per side")]
    ImageTooWide { width: u32, height: u32 },
    #[error(
        "the task already has {0} artifacts (artifacts.maxPerTask); replace an existing artifact instead"
    )]
    TooMany(u32),
}

/// Checks an artifact name.
pub fn validate_name(name: &str) -> Result<(), ArtifactError> {
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(ArtifactError::InvalidName(name.chars().take(120).collect()))
    }
}

/// Detects the type of `bytes` from their content only, without reading image dimensions: an
/// image of a supported format, else UTF-8 text without NUL. SVG is text but rejected, as it can
/// carry scripts. Returns the kind and the MIME type (`text/plain` for text).
pub fn sniff_kind(bytes: &[u8]) -> Result<(ArtifactKind, &'static str), ArtifactError> {
    Ok(detect(bytes)?.kind_and_mime())
}

/// Like [`sniff_kind`], and for an image its pixel size from the header.
pub fn sniff(bytes: &[u8]) -> Result<Sniffed, ArtifactError> {
    let detected = detect(bytes)?;
    let (kind, mime_type) = detected.kind_and_mime();
    let dimensions = match detected {
        Detected::Image { format, .. } => Some(
            ImageReader::with_format(Cursor::new(bytes), format)
                .into_dimensions()
                .map_err(|_error| ArtifactError::Unsupported("the image header is damaged"))?,
        ),
        Detected::Text => None,
    };
    Ok(Sniffed {
        kind,
        mime_type,
        dimensions,
    })
}

/// The MIME type stored for a text artifact: Markdown for `*.md`, else plain text.
#[must_use]
pub fn text_mime(name: &str) -> &'static str {
    if name.strip_suffix(".md").is_some() {
        "text/markdown"
    } else {
        "text/plain"
    }
}

/// A byte count for error messages: `3.75 MiB`, `1 MiB`, `512 bytes`.
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if bytes < MIB {
        return format!("{bytes} bytes");
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "limits are far below 2^52 bytes"
    )]
    let mib = bytes as f64 / MIB as f64;
    let text = format!("{mib:.2}");
    format!("{} MiB", text.trim_end_matches('0').trim_end_matches('.'))
}

/// What the content is, detected once.
enum Detected {
    Image {
        format: ImageFormat,
        mime: &'static str,
    },
    Text,
}

impl Detected {
    fn kind_and_mime(&self) -> (ArtifactKind, &'static str) {
        match self {
            Self::Image { mime, .. } => (ArtifactKind::Image, mime),
            Self::Text => (ArtifactKind::Text, "text/plain"),
        }
    }
}

fn detect(bytes: &[u8]) -> Result<Detected, ArtifactError> {
    let image = image::guess_format(bytes)
        .ok()
        .and_then(|format| image_mime(format).map(|mime| Detected::Image { format, mime }));
    if let Some(image) = image {
        return Ok(image);
    }
    if bytes.contains(&0) {
        return Err(ArtifactError::Unsupported("the content has NUL bytes"));
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(ArtifactError::Unsupported("the content is not UTF-8"));
    };
    if looks_like_svg(text) {
        return Err(ArtifactError::Unsupported("SVG is not supported"));
    }
    Ok(Detected::Text)
}

fn image_mime(format: ImageFormat) -> Option<&'static str> {
    match format {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::WebP => Some("image/webp"),
        ImageFormat::Gif => Some("image/gif"),
        _ => None,
    }
}

fn looks_like_svg(text: &str) -> bool {
    let head: String = text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .chars()
        .take(1024)
        .collect::<String>()
        .to_ascii_lowercase();
    head.starts_with("<svg")
        || ((head.starts_with("<?xml") || head.starts_with("<!doctype svg"))
            && head.contains("<svg"))
}

#[cfg(test)]
mod tests;
