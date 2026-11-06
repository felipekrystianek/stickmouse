//! Generates the Xcursor theme where every cursor shape is a circle with a centre dot.

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::Path;

pub const THEME_NAME: &str = "stickmouse-circle";
/// The theme's name before the rename. It may still be the active theme right after an upgrade.
pub const OLD_THEME_NAME: &str = "padcursor-circle";

const NOMINAL_SIZES: [u32; 4] = [24, 36, 48, 72];
const REFERENCE_SHAPES: &str = "/usr/share/icons/breeze_cursors/cursors";
const IMAGE_TYPE: u32 = 0xFFFD_0002;
const FILE_HEADER: u32 = 16;
const TOC_ENTRY: u32 = 12;
const IMAGE_HEADER: u32 = 36;
// Used if the reference theme is missing, so the common shapes still become circles.
const CORE_SHAPES: [&str; 12] = [
    "arrow",
    "left_ptr",
    "pointer",
    "hand1",
    "hand2",
    "pointing_hand",
    "text",
    "xterm",
    "ibeam",
    "wait",
    "watch",
    "progress",
];

/// Writes the theme under `icons_dir`, leaving it untouched if it is already up to date.
pub fn install(icons_dir: &Path, scale: f32) -> io::Result<()> {
    let root = icons_dir.join(THEME_NAME);
    let cursors = root.join("cursors");
    fs::create_dir_all(&cursors)?;

    let image = encode(scale);
    let default = cursors.join("default");
    if fs::read(&default).ok().as_deref() != Some(image.as_slice()) {
        fs::write(&default, image)?;
    }

    let shapes: Vec<String> = match fs::read_dir(REFERENCE_SHAPES) {
        Ok(entries) => entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect(),
        Err(_) => CORE_SHAPES.iter().map(ToString::to_string).collect(),
    };
    for shape in shapes.iter().filter(|shape| *shape != "default") {
        let link = cursors.join(shape);
        if fs::symlink_metadata(&link).is_err() {
            symlink("default", link)?;
        }
    }

    fs::write(
        root.join("index.theme"),
        format!("[Icon Theme]\nName={THEME_NAME}\nComment=Stickmouse steering cursor\n"),
    )
}

fn encode(scale: f32) -> Vec<u8> {
    let images: Vec<(u32, u32, Vec<u8>)> = NOMINAL_SIZES
        .iter()
        .map(|&nominal| {
            let size = pixel_size(nominal, scale);
            (nominal, size, render(size))
        })
        .collect();

    let mut out = Vec::new();
    out.extend_from_slice(b"Xcur");
    for value in [FILE_HEADER, 0x1_0000, images.len() as u32] {
        out.extend_from_slice(&value.to_le_bytes());
    }

    let mut position = FILE_HEADER + TOC_ENTRY * images.len() as u32;
    for (nominal, _, pixels) in &images {
        for value in [IMAGE_TYPE, *nominal, position] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        position += IMAGE_HEADER + pixels.len() as u32;
    }
    for (nominal, size, pixels) in &images {
        let hotspot = size / 2;
        for value in [IMAGE_HEADER, IMAGE_TYPE, *nominal, 1, *size, *size, hotspot, hotspot, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(pixels);
    }
    out
}

/// Sizes are odd so the centre dot sits exactly on the hotspot pixel.
fn pixel_size(nominal: u32, scale: f32) -> u32 {
    ((nominal as f32 * scale).round() as u32).max(9) | 1
}

fn render(size: u32) -> Vec<u8> {
    let size_f = size as f32;
    let centre = (size_f - 1.0) / 2.0;
    let radius = size_f * 0.40;
    let stroke = (size_f * 0.08).max(2.0);
    let dot = (size_f * 0.07).max(1.5);
    let outline = (size_f * 0.04).max(1.0);
    let layers = [
        (radius - stroke / 2.0 - outline, radius + stroke / 2.0 + outline, 0.0),
        (radius - stroke / 2.0, radius + stroke / 2.0, 1.0),
        (-1.0, dot + outline, 0.0),
        (-1.0, dot, 1.0),
    ];

    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let distance = (x as f32 - centre).hypot(y as f32 - centre);
            let (mut alpha, mut grey) = (0.0_f32, 0.0_f32);
            for (inner, outer, shade) in layers {
                let coverage = ((distance - inner).min(outer - distance) + 0.5).clamp(0.0, 1.0);
                grey = coverage * shade + grey * (1.0 - coverage);
                alpha = coverage + alpha * (1.0 - coverage);
            }
            let (a, c) = ((alpha * 255.0).round() as u32, (grey * 255.0).round() as u32);
            let argb = (a << 24) | (c << 16) | (c << 8) | c;
            pixels.extend_from_slice(&argb.to_le_bytes());
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(data: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn file_is_valid_xcursor_with_every_nominal_size() {
        let data = encode(1.5);
        assert_eq!(&data[..4], b"Xcur");
        assert_eq!(u32_at(&data, 12), NOMINAL_SIZES.len() as u32);
        for (index, nominal) in NOMINAL_SIZES.iter().enumerate() {
            let toc = 16 + index * 12;
            assert_eq!(u32_at(&data, toc), IMAGE_TYPE);
            assert_eq!(u32_at(&data, toc + 4), *nominal);
            let image = u32_at(&data, toc + 8) as usize;
            assert_eq!(u32_at(&data, image + 8), *nominal);
            let size = u32_at(&data, image + 16);
            assert_eq!(size, pixel_size(*nominal, 1.5));
            assert_eq!(u32_at(&data, image + 24), size / 2, "hotspot is the centre");
        }
    }

    #[test]
    fn scale_changes_the_drawn_size() {
        assert_eq!(pixel_size(24, 1.0), 25);
        assert_eq!(pixel_size(24, 1.5), 37);
        assert_eq!(pixel_size(24, 2.0), 49);
    }

    #[test]
    fn centre_is_white_dot_and_corners_are_transparent() {
        let size = 37;
        let pixels = render(size);
        let at = |x: u32, y: u32| u32_at(&pixels, ((y * size + x) * 4) as usize);
        assert_eq!(at(size / 2, size / 2), 0xFFFF_FFFF);
        assert_eq!(at(0, 0) >> 24, 0);
    }
}
