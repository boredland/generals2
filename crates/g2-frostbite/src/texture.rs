//! Texture resources: a 128-byte header (version 10) naming a pixel chunk, which holds the
//! mip chain largest first. Format codes were identified by decoding the alpha's own files
//! and checking the images and chunk sizes; codes this build doesn't use are rejected.

use crate::{Alpha, Entry, Error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Code 0 (colour) and 19 (normal maps).
    Bc1,
    Bc3,
    Bc4,
    Bc5,
    Rgba8,
    L8,
}

impl Format {
    fn from_code(code: u32) -> Option<Format> {
        Some(match code {
            0 | 19 => Format::Bc1,
            3 => Format::Bc3,
            4 => Format::Bc4,
            18 => Format::Bc5,
            10 => Format::Rgba8,
            11 => Format::L8,
            _ => return None,
        })
    }

    /// Bytes of one level of `w`×`h`.
    fn level_size(self, w: u32, h: u32) -> usize {
        let blocks = (w.div_ceil(4) * h.div_ceil(4)) as usize;
        match self {
            Format::Bc1 | Format::Bc4 => blocks * 8,
            Format::Bc3 | Format::Bc5 => blocks * 16,
            Format::Rgba8 => (w * h * 4) as usize,
            Format::L8 => (w * h) as usize,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// 0 = 2D, 1 = cube, 2 = volume.
    pub kind: u32,
    pub format_code: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub slices: u32,
    pub mips: u32,
    pub chunk: [u8; 16],
}

impl Header {
    pub fn parse(b: &[u8]) -> Result<Header, Error> {
        if b.len() < 44 {
            return Err(Error::Truncated);
        }
        let u32_at = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let u16_at = |i: usize| u32::from(u16::from_le_bytes(b[i..i + 2].try_into().unwrap()));
        if u32_at(0) != 10 {
            return Err(Error::Unsupported(format!("texture header version {}", u32_at(0))));
        }
        Ok(Header {
            kind: u32_at(4),
            format_code: u32_at(8),
            width: u16_at(16),
            height: u16_at(18),
            depth: u16_at(20),
            slices: u16_at(22),
            mips: u32::from(b[26]),
            chunk: b[28..44].try_into().unwrap(),
        })
    }
}

/// The largest level as straight RGBA8 rows.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decodes a texture res's largest level. Only flat 2D textures: cube and volume maps are
/// sky and post-process lookups, nothing a unit or building uses.
pub fn decode(alpha: &Alpha, e: &Entry) -> Result<Image, Error> {
    let h = Header::parse(&alpha.entry_bytes(e)?)?;
    if h.kind != 0 || h.slices > 1 || h.depth > 1 {
        return Err(Error::Unsupported(format!("texture kind {} with {} slices", h.kind, h.slices)));
    }
    let format = Format::from_code(h.format_code)
        .ok_or_else(|| Error::Unsupported(format!("texture format {}", h.format_code)))?;
    let pixels = alpha.chunk(&h.chunk)?;
    decode_level(format, h.width, h.height, &pixels)
}

pub fn decode_level(format: Format, w: u32, h: u32, data: &[u8]) -> Result<Image, Error> {
    if w == 0 || h == 0 {
        return Err(Error::Bad("empty texture"));
    }
    let data = data.get(..format.level_size(w, h)).ok_or(Error::Truncated)?;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    match format {
        Format::Rgba8 => rgba.copy_from_slice(data),
        Format::L8 => {
            for (px, &l) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(data) {
                px.copy_from_slice(&[l, l, l, 255]);
            }
        }
        _ => {
            let stride = if matches!(format, Format::Bc1 | Format::Bc4) { 8 } else { 16 };
            for (i, block) in data.chunks_exact(stride).enumerate() {
                let texels = match format {
                    Format::Bc1 => bc1(block.try_into().unwrap(), true),
                    Format::Bc3 => {
                        let mut t = bc1(block[8..].try_into().unwrap(), false);
                        for (px, a) in t.iter_mut().zip(bc4(block[..8].try_into().unwrap())) {
                            px[3] = a;
                        }
                        t
                    }
                    Format::Bc4 => bc4(block.try_into().unwrap()).map(|v| [v, v, v, 255]),
                    // Two-channel normal maps; blue is left at full so the image reads as
                    // the familiar normal-map purple.
                    Format::Bc5 => {
                        let (r, g) = (bc4(block[..8].try_into().unwrap()), bc4(block[8..].try_into().unwrap()));
                        std::array::from_fn(|k| [r[k], g[k], 255, 255])
                    }
                    Format::Rgba8 | Format::L8 => unreachable!(),
                };
                let blocks_wide = w.div_ceil(4);
                let (bx, by) = (i as u32 % blocks_wide * 4, i as u32 / blocks_wide * 4);
                for (k, texel) in texels.iter().enumerate() {
                    let (x, y) = (bx + k as u32 % 4, by + k as u32 / 4);
                    if x < w && y < h {
                        let at = ((y * w + x) * 4) as usize;
                        rgba[at..at + 4].copy_from_slice(texel);
                    }
                }
            }
        }
    }
    Ok(Image { width: w, height: h, rgba })
}

fn rgb565(c: u16) -> [u32; 3] {
    let (r, g, b) = (u32::from(c >> 11), u32::from((c >> 5) & 63), u32::from(c & 31));
    [r * 255 / 31, g * 255 / 63, b * 255 / 31]
}

/// One BC1 block. `punch_through` enables the 3-colour + transparent mode when the first
/// endpoint is not greater; BC3's colour half always uses four colours.
fn bc1(b: &[u8; 8], punch_through: bool) -> [[u8; 4]; 16] {
    let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let lerp = |num: u32, den: u32| -> [u8; 4] {
        let c = |i: usize| ((p0[i] * (den - num) + p1[i] * num) / den) as u8;
        [c(0), c(1), c(2), 255]
    };
    let palette = if c0 > c1 || !punch_through {
        [lerp(0, 3), lerp(3, 3), lerp(1, 3), lerp(2, 3)]
    } else {
        [lerp(0, 2), lerp(2, 2), lerp(1, 2), [0, 0, 0, 0]]
    };
    let bits = u32::from_le_bytes(b[4..8].try_into().unwrap());
    std::array::from_fn(|i| palette[(bits >> (2 * i) & 3) as usize])
}

/// One BC4 block (also BC3's alpha and each BC5 channel).
fn bc4(b: &[u8; 8]) -> [u8; 16] {
    let (a0, a1) = (u32::from(b[0]), u32::from(b[1]));
    let mut pal = [0u8; 8];
    pal[0] = a0 as u8;
    pal[1] = a1 as u8;
    if a0 > a1 {
        for i in 1..7 {
            pal[i + 1] = (((7 - i as u32) * a0 + i as u32 * a1) / 7) as u8;
        }
    } else {
        for i in 1..5 {
            pal[i + 1] = (((5 - i as u32) * a0 + i as u32 * a1) / 5) as u8;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let mut bits = [0u8; 8];
    bits[..6].copy_from_slice(&b[2..8]);
    let bits = u64::from_le_bytes(bits);
    std::array::from_fn(|i| pal[(bits >> (3 * i) & 7) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc1_four_colour_block_interpolates_endpoints() {
        // Pure red over pure blue. Bits 0xe4 give texels 0..4 the indices 0, 1, 2, 3:
        // the two endpoints, then the 1/3 and 2/3 blends.
        let block = [0x00, 0xf8, 0x1f, 0x00, 0xe4, 0xe4, 0xe4, 0xe4];
        let t = bc1(&block, true);
        assert_eq!(t[0], [255, 0, 0, 255]);
        assert_eq!(t[1], [0, 0, 255, 255]);
        assert_eq!(t[2], [170, 0, 85, 255]);
        assert_eq!(t[3], [85, 0, 170, 255]);
    }

    #[test]
    fn bc1_punch_through_only_outside_bc3() {
        // c0 <= c1 selects three colours plus transparent black at index 3.
        let block = [0x1f, 0x00, 0x00, 0xf8, 0xff, 0xff, 0xff, 0xff];
        assert_eq!(bc1(&block, true)[0], [0, 0, 0, 0]);
        assert_eq!(bc1(&block, false)[0][3], 255);
    }

    #[test]
    fn bc4_eight_and_six_value_modes() {
        // a0 > a1: index 2 is 6/7 a0 + 1/7 a1.
        let block = [255, 0, 0b010, 0, 0, 0, 0, 0];
        assert_eq!(bc4(&block)[0], 218);
        // a0 <= a1: indices 6 and 7 are 0 and 255.
        let block = [0, 255, 0b110 | 0b111 << 3, 0, 0, 0, 0, 0];
        assert_eq!(&bc4(&block)[..2], &[0, 255]);
    }

    #[test]
    fn levels_smaller_than_a_block_are_cropped() {
        // A 2×2 BC1 texture still occupies one 8-byte block.
        let img = decode_level(Format::Bc1, 2, 2, &[0x00, 0xf8, 0x1f, 0x00, 0, 0, 0, 0]).unwrap();
        assert_eq!(img.rgba.len(), 16);
        assert!(img.rgba.chunks(4).all(|p| p == [255, 0, 0, 255]));
        assert!(decode_level(Format::Bc3, 8, 8, &[0; 63]).is_err());
    }

    #[test]
    fn header_fields_and_unknown_versions() {
        let mut h = vec![0u8; 128];
        h[0] = 10;
        h[8] = 3;
        h[16..24].copy_from_slice(&[0, 2, 0, 1, 1, 0, 1, 0]);
        h[26] = 10;
        h[28..44].copy_from_slice(&[9; 16]);
        let p = Header::parse(&h).unwrap();
        assert_eq!((p.format_code, p.width, p.height, p.depth, p.slices, p.mips), (3, 512, 256, 1, 1, 10));
        assert_eq!(p.chunk, [9; 16]);
        h[0] = 11;
        assert!(Header::parse(&h).is_err());
    }
}
