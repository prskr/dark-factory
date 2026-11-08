use std::io::Cursor;

use image::{ImageFormat, RgbImage};

use super::*;

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    RgbImage::new(width, height)
        .write_to(&mut out, ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// A baseline JPEG of 1 x 1 px.
fn jpeg() -> Vec<u8> {
    hex::decode(
        "ffd8ffdb00430001010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101ffc0000b080001000101011100ffc40014000100000000000000000000000000000000ffc40014100100000000000000000000000000000000ffda0008010100003f003fffd9",
    )
    .unwrap()
}

#[test]
fn text_mime_depends_on_the_extension_only() {
    let limits = ArtifactsConfig::default();
    assert_eq!(
        check_artifact("a.md", b"x", &limits).unwrap().mime_type,
        "text/markdown"
    );
    assert_eq!(
        check_artifact("a.rs", b"x", &limits).unwrap().mime_type,
        "text/plain"
    );
    assert_eq!(
        check_artifact("shot.png", &jpeg(), &limits)
            .unwrap()
            .mime_type,
        "image/jpeg"
    );
}

#[test]
fn size_limits_name_the_limit() {
    let limits = ArtifactsConfig::default();
    let text = vec![b'a'; 1024 * 1024 + 1];
    let err = check_artifact("a.txt", &text, &limits)
        .unwrap_err()
        .to_string();
    assert!(err.contains("1 MiB"), "{err}");

    let big_image = ArtifactsConfig {
        max_image_bytes: 100,
        ..limits
    };
    let err = check_artifact("a.png", &png(64, 64), &big_image)
        .unwrap_err()
        .to_string();
    assert!(err.contains("100 bytes"), "{err}");
    assert_eq!(format_bytes(limits.max_image_bytes), "3.75 MiB");
}

#[test]
fn image_sides_over_8000_px_are_rejected() {
    let err = check_artifact("a.png", &png(9000, 600), &ArtifactsConfig::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("8000 px"), "{err}");
    assert!(check_artifact("a.png", &png(8000, 10), &ArtifactsConfig::default()).is_ok());
}
