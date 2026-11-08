//! Task artifacts: the types and sniffing of `df-artifact-types`, plus the limits of the
//! instance configuration (spec `task-artifacts`, design D2).
//!
//! The control plane calls [`check_artifact`] on every write and stores the type it detects
//! itself, whatever the runtime reported.

pub use df_artifact_types::{
    ArtifactError, ArtifactKind, MAX_ARTIFACT_BYTES, Sniffed, validate_name,
};
use df_artifact_types::{MAX_IMAGE_SIDE, format_bytes, sniff, text_mime};

use crate::config::ArtifactsConfig;

/// A validated write: the detected type, with the text MIME type derived from `name`.
pub fn check_artifact(
    name: &str,
    bytes: &[u8],
    limits: &ArtifactsConfig,
) -> Result<Sniffed, ArtifactError> {
    validate_name(name)?;
    let size = bytes.len() as u64;
    let mut sniffed = sniff(bytes)?;
    match (sniffed.kind, sniffed.dimensions) {
        (ArtifactKind::Text, _) => {
            if size > limits.max_text_bytes {
                return Err(ArtifactError::TextTooLarge {
                    size,
                    limit: format_bytes(limits.max_text_bytes),
                });
            }
            sniffed.mime_type = text_mime(name);
        }
        (ArtifactKind::Image, dimensions) => {
            if size > limits.max_image_bytes {
                return Err(ArtifactError::ImageTooLarge {
                    size,
                    limit: format_bytes(limits.max_image_bytes),
                });
            }
            if let Some((width, height)) = dimensions
                && (width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE)
            {
                return Err(ArtifactError::ImageTooWide { width, height });
            }
        }
    }
    Ok(sniffed)
}

#[cfg(test)]
mod tests;
