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

const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";

/// A baseline JPEG of 1 x 1 px.
fn jpeg() -> Vec<u8> {
    hex::decode(
        "ffd8ffdb00430001010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101ffc0000b080001000101011100ffc40014000100000000000000000000000000000000ffc40014100100000000000000000000000000000000ffda0008010100003f003fffd9",
    )
    .unwrap()
}

/// A lossless WebP header for a 5 x 3 image.
fn webp() -> Vec<u8> {
    let bits: u32 = 4 | (2 << 14);
    let mut chunk = vec![0x2f];
    chunk.extend_from_slice(&bits.to_le_bytes());
    chunk.push(0);
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(4 + 8 + u32::try_from(chunk.len()).unwrap()).to_le_bytes());
    out.extend_from_slice(b"WEBPVP8L");
    out.extend_from_slice(&u32::try_from(chunk.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&chunk);
    out
}

#[test]
fn names_follow_the_allowed_characters() {
    for ok in ["findings.md", "a", "login-1_2.png", "A.b", &"x".repeat(100)] {
        assert!(validate_name(ok).is_ok(), "{ok}");
    }
    for bad in [
        "",
        ".env",
        "notes/findings.md",
        "a b",
        "größe.md",
        "..",
        &"x".repeat(101),
    ] {
        let err = validate_name(bad).unwrap_err().to_string();
        assert!(err.contains("A-Z"), "{bad}: {err}");
    }
}

#[test]
fn text_is_detected_and_sniffed_as_plain() {
    let sniffed = sniff("# Heading\nümlaut".as_bytes()).unwrap();
    assert_eq!(sniffed.kind, ArtifactKind::Text);
    assert_eq!(sniffed.dimensions, None);
    assert_eq!(sniff(b"").unwrap().kind, ArtifactKind::Text);
}

#[test]
fn nul_bytes_and_invalid_utf8_are_rejected() {
    assert!(sniff(b"a\0b").is_err());
    assert!(sniff(b"\xc3\x28").is_err());
}

#[test]
fn images_are_detected_with_their_size_from_the_header() {
    let cases: [(Vec<u8>, &str, (u32, u32)); 4] = [
        (png(7, 4), "image/png", (7, 4)),
        (GIF.to_vec(), "image/gif", (1, 1)),
        (jpeg(), "image/jpeg", (1, 1)),
        (webp(), "image/webp", (5, 3)),
    ];
    for (bytes, mime, dimensions) in cases {
        let sniffed = sniff(&bytes).unwrap_or_else(|e| panic!("{mime}: {e}"));
        assert_eq!(sniffed.kind, ArtifactKind::Image, "{mime}");
        assert_eq!(sniffed.mime_type, mime);
        assert_eq!(sniffed.dimensions, Some(dimensions), "{mime}");
    }
}

#[test]
fn svg_zip_and_pdf_are_rejected() {
    let svg = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
    assert!(sniff(svg).is_err());
    assert!(sniff(b"  <SVG></SVG>").is_err());
    assert!(sniff(b"PK\x03\x04\x14\x00\x00\x00").is_err());
    assert!(sniff(b"%PDF-1.7\n\x00\x00").is_err());
}

#[test]
fn the_kind_comes_from_the_content_without_reading_dimensions() {
    // Only the PNG signature: enough for the kind, too short for a header.
    let (kind, mime) = sniff_kind(b"\x89PNG\r\n\x1a\n").unwrap();
    assert_eq!((kind, mime), (ArtifactKind::Image, "image/png"));
    assert!(sniff(b"\x89PNG\r\n\x1a\n").is_err());
    assert_eq!(sniff_kind(b"# text").unwrap().0, ArtifactKind::Text);
    assert!(sniff_kind(b"%PDF-1.7\n\x00").is_err());
}
