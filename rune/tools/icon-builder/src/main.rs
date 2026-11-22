//! Deterministic, font-free SVG rasterization and native icon-container generation.
//! Usage: cargo run --locked --manifest-path rune/tools/icon-builder/Cargo.toml -- OUTPUT_DIRECTORY
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg::{Options, Tree},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const LARGE: &str = include_str!("../../../packaging/assets/Pickle.svg");
const SMALL: &str = include_str!("../../../packaging/assets/Pickle-small.svg");
const SIZES: &[u32] = &[16, 20, 24, 32, 40, 48, 64, 128, 256, 512, 1024];
const ICO_SIZES: &[u32] = &[16, 20, 24, 32, 40, 48, 64, 128, 256];
const ICNS_SIZES: &[(&[u8; 4], u32)] = &[
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic13", 256),
    (b"ic14", 512),
];

fn render(source: &str, size: u32) -> Result<Vec<u8>, String> {
    let tree = Tree::from_str(source, &Options::default())
        .map_err(|_| "Could not parse Pickle's SVG source.")?;
    let mut pixmap = Pixmap::new(size, size).ok_or("Invalid icon dimensions.")?;
    let transform = Transform::from_scale(
        size as f32 / tree.size().width(),
        size as f32 / tree.size().height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap
        .encode_png()
        .map_err(|_| "Could not encode an icon frame.".into())
}
fn frames() -> Result<BTreeMap<u32, Vec<u8>>, String> {
    SIZES
        .iter()
        .map(|size| {
            render(if *size <= 32 { SMALL } else { LARGE }, *size).map(|frame| (*size, frame))
        })
        .collect()
}
fn ico(frames: &BTreeMap<u32, Vec<u8>>) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&(ICO_SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * ICO_SIZES.len() as u32;
    for size in ICO_SIZES {
        let frame = &frames[size];
        // ICO encodes a 256px dimension as zero; PNG frames carry their exact dimensions.
        output.extend_from_slice(&[
            if *size == 256 { 0 } else { *size as u8 },
            if *size == 256 { 0 } else { *size as u8 },
            0,
            0,
        ]);
        output.extend_from_slice(&1u16.to_le_bytes());
        output.extend_from_slice(&32u16.to_le_bytes());
        output.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        output.extend_from_slice(&offset.to_le_bytes());
        offset += frame.len() as u32;
    }
    for size in ICO_SIZES {
        output.extend_from_slice(&frames[size]);
    }
    output
}
fn icns(frames: &BTreeMap<u32, Vec<u8>>) -> Vec<u8> {
    let length = 8 + ICNS_SIZES
        .iter()
        .map(|(_, size)| 8 + frames[size].len())
        .sum::<usize>();
    let mut output = Vec::with_capacity(length);
    output.extend_from_slice(b"icns");
    output.extend_from_slice(&(length as u32).to_be_bytes());
    for (kind, size) in ICNS_SIZES {
        let frame = &frames[size];
        output.extend_from_slice(*kind);
        output.extend_from_slice(&((8 + frame.len()) as u32).to_be_bytes());
        output.extend_from_slice(frame);
    }
    output
}
fn build(directory: &Path) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|_| "Could not create the icon output directory.")?;
    let frames = frames()?;
    for (name, bytes) in [
        ("Pickle.ico", ico(&frames)),
        ("Pickle.icns", icns(&frames)),
        ("Pickle-preview.png", frames[&512].clone()),
        ("Pickle-16.png", frames[&16].clone()),
    ] {
        fs::write(directory.join(name), bytes).map_err(|_| format!("Could not write {name}."))?;
        println!("Generated {name}");
    }
    Ok(())
}
fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 1 {
        eprintln!("Usage: pickle-icon-builder OUTPUT_DIRECTORY");
        std::process::exit(2);
    }
    if let Err(error) = build(&PathBuf::from(&arguments[0])) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png_size(bytes: &[u8]) -> (u32, u32) {
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        (
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        )
    }
    #[test]
    fn frames_are_deterministic_and_font_network_independent() {
        for source in [LARGE, SMALL] {
            assert!(!source.contains("<text"));
            assert!(!source.contains("href="));
            assert!(!source.contains("<image"));
        }
        let first = frames().unwrap();
        let second = frames().unwrap();
        assert_eq!(first, second);
        for (size, frame) in &first {
            assert_eq!(png_size(frame), (*size, *size));
        }
    }
    #[test]
    fn ico_directory_offsets_and_frames_are_valid() {
        let frames = frames().unwrap();
        let output = ico(&frames);
        assert_eq!(&output[..6], &[0, 0, 1, 0, 9, 0]);
        let mut expected_offset = 6 + 16 * ICO_SIZES.len();
        for (index, size) in ICO_SIZES.iter().enumerate() {
            let start = 6 + index * 16;
            let length =
                u32::from_le_bytes(output[start + 8..start + 12].try_into().unwrap()) as usize;
            let offset =
                u32::from_le_bytes(output[start + 12..start + 16].try_into().unwrap()) as usize;
            assert_eq!(offset, expected_offset);
            assert_eq!(png_size(&output[offset..offset + length]), (*size, *size));
            expected_offset += length;
        }
        assert_eq!(expected_offset, output.len());
    }
    #[test]
    fn icns_directory_lengths_and_frames_are_valid() {
        let output = icns(&frames().unwrap());
        assert_eq!(&output[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(output[4..8].try_into().unwrap()) as usize,
            output.len()
        );
        let mut offset = 8;
        for (kind, size) in ICNS_SIZES {
            assert_eq!(&output[offset..offset + 4], *kind);
            let length =
                u32::from_be_bytes(output[offset + 4..offset + 8].try_into().unwrap()) as usize;
            assert_eq!(
                png_size(&output[offset + 8..offset + length]),
                (*size, *size)
            );
            offset += length;
        }
        assert_eq!(offset, output.len());
    }
}
