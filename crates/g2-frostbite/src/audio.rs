//! EA audio in the alpha: every sound is EA-XAS v1 ADPCM in an "SPS" stream (EAAudioCore),
//! found at a segment's `SamplesOffset` inside its chunk. The stream is a run of big-endian
//! blocks: a `0x48` header block (codec, channels, rate, sample count), `0x44` data blocks
//! (sample count, then per channel 0x4C-byte frames of 128 samples), and a `0x45` end.
//!
//! The XAS decoder is ported from vgmstream (`src/coding/ea_xas_decoder.c`, r2117):
//!
//! Copyright (c) 2008-2025 Adam Gashlin, Fastelbja, Ronny Elfert, bnnm, Christopher Snowhill,
//! NicknineTheEagle, bxaimc, Thealexbarney, CyberBotX, EdnessP, et al.
//!
//! Permission to use, copy, modify, and distribute this software for any purpose with or
//! without fee is hereby granted, provided that the above copyright notice and this
//! permission notice appear in all copies. THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR
//! DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
//! MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL,
//! DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
//! OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS
//! ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

use crate::Error;

/// Decoded PCM, channels interleaved.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }

    /// A 16-bit PCM WAV file.
    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let block_align = self.channels * 2;
        let mut w = Vec::with_capacity(44 + data_len as usize);
        w.extend(b"RIFF");
        w.extend((36 + data_len).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(self.channels.to_le_bytes());
        w.extend(self.sample_rate.to_le_bytes());
        w.extend((self.sample_rate * u32::from(block_align)).to_le_bytes());
        w.extend(block_align.to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend(data_len.to_le_bytes());
        for s in &self.samples {
            w.extend(s.to_le_bytes());
        }
        w
    }
}

const XAS1: u32 = 4;
const FRAME: usize = 0x4c;
const FRAME_SAMPLES: usize = 128;

/// Decodes one SPS stream starting at `bytes[0]`.
pub fn decode_sps(bytes: &[u8]) -> Result<Pcm, Error> {
    let be = |at: usize| -> Result<u32, Error> {
        Ok(u32::from_be_bytes(bytes.get(at..at + 4).ok_or(Error::Truncated)?.try_into().unwrap()))
    };
    let head = be(0)?;
    if head >> 24 != 0x48 {
        return Err(Error::Bad("SPS stream does not start with a header block"));
    }
    let (h1, h2) = (be(4)?, be(8)?);
    let version = h1 >> 28;
    let codec = (h1 >> 24) & 0x0f;
    let channels = ((h1 >> 18) & 0x3f) as u16 + 1;
    let sample_rate = h1 & 0x3_ffff;
    let total = (h2 & 0x1fff_ffff) as usize;
    if version != 1 || codec != XAS1 {
        return Err(Error::Unsupported(format!("EA audio version {version} codec {codec}")));
    }
    let ch = usize::from(channels);
    let mut samples = Vec::with_capacity(total * ch);
    let mut at = (head & 0x00ff_ffff) as usize;
    let mut decoded = 0;
    while decoded < total {
        let block = be(at)?;
        let (id, size) = (block >> 24, (block & 0x00ff_ffff) as usize);
        match id {
            0x45 => break,
            0x44 => {}
            _ => return Err(Error::Bad("unexpected SPS block")),
        }
        if size < 8 {
            return Err(Error::Bad("SPS block too small"));
        }
        let block_samples = be(at + 4)? as usize;
        let body = bytes.get(at + 8..at + size).ok_or(Error::Truncated)?;
        let frames = block_samples.div_ceil(FRAME_SAMPLES);
        if body.len() < frames * FRAME * ch {
            return Err(Error::Truncated);
        }
        let take = block_samples.min(total - decoded);
        let mut pcm = vec![0i16; frames * FRAME_SAMPLES * ch];
        for f in 0..frames {
            for c in 0..ch {
                let frame = &body[(f * ch + c) * FRAME..][..FRAME];
                let out = xas_frame(frame.try_into().unwrap());
                for (i, s) in out.iter().enumerate() {
                    pcm[(f * FRAME_SAMPLES + i) * ch + c] = *s;
                }
            }
        }
        samples.extend_from_slice(&pcm[..take * ch]);
        decoded += take;
        at += size;
    }
    if decoded != total {
        return Err(Error::Bad("SPS stream ended early"));
    }
    Ok(Pcm { sample_rate, channels, samples })
}

/// CD-XA filter pairs; XAS uses the first four.
const COEFS: [[f32; 2]; 4] = [[0.0, 0.0], [0.9375, 0.0], [1.796875, -0.8125], [1.53125, -0.859375]];

/// One 0x4C-byte frame: four little-endian group headers (a 12-bit-aligned history pair,
/// filter index and shift), then 15 rows of four bytes holding each group's nibbles.
fn xas_frame(frame: &[u8; FRAME]) -> [i16; FRAME_SAMPLES] {
    let mut out = [0i16; FRAME_SAMPLES];
    for group in 0..4 {
        let header = u32::from_le_bytes(frame[group * 4..group * 4 + 4].try_into().unwrap());
        let [c1, c2] = COEFS.get((header & 0x0f) as usize).copied().unwrap_or([0.0, 0.0]);
        let mut hist2 = (header & 0xfff0) as u16 as i16;
        let mut hist1 = ((header >> 16) & 0xfff0) as u16 as i16;
        let shift = (header >> 16) & 0x0f;
        let base = group * 32;
        out[base] = hist2;
        out[base + 1] = hist1;
        for row in 0..15 {
            let byte = frame[16 + row * 4 + group];
            for (i, nibble) in [byte >> 4, byte & 0x0f].into_iter().enumerate() {
                let scaled = i32::from(((u16::from(nibble) << 12) as i16) >> shift);
                // vgmstream sums in float and truncates toward zero, as C's int conversion does.
                let s = (scaled as f32 + f32::from(hist1) * c1 + f32::from(hist2) * c2) as i32;
                let s = s.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
                out[base + 2 + row * 2 + i] = s;
                hist2 = hist1;
                hist1 = s;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_with(group0: u32, nibble_byte: u8) -> [u8; FRAME] {
        let mut f = [0u8; FRAME];
        f[..4].copy_from_slice(&group0.to_le_bytes());
        for row in 0..15 {
            f[16 + row * 4] = nibble_byte;
        }
        f
    }

    #[test]
    fn group_starts_with_its_header_history_then_filters_nibbles() {
        // hist2 = 0x1230, hist1 = 0x0450, filter 1 (0.9375 * hist1), shift 2.
        let f = frame_with(0x0452_1231, 0x10);
        let out = xas_frame(&f);
        assert_eq!(out[0], 0x1230);
        assert_eq!(out[1], 0x0450);
        // First nibble 1: (1 << 12) >> 2 = 1024, plus 0.9375 * 0x450 = 1035 -> 2059.
        assert_eq!(out[2], 2059);
        // Second nibble 0: 0.9375 * 2059 = 1930.3 -> 1930 (truncated).
        assert_eq!(out[3], 1930);
        // Other groups are silent.
        assert!(out[32..].iter().all(|&s| s == 0));
    }

    #[test]
    fn negative_nibbles_sign_extend_and_output_clamps() {
        // Filter 2 with large positive history drives past i16::MAX.
        let f = frame_with(0x7ff0_7ff2, 0x70);
        let out = xas_frame(&f);
        assert_eq!(out[2], i16::MAX);
        // Nibble 0xF is -1: (0xF000 as i16) >> 0 = -4096, no filter.
        let f = frame_with(0, 0xf0);
        assert_eq!(xas_frame(&f)[2], -4096);
    }

    fn stream(channels: u32, total: u32, blocks: &[u32]) -> Vec<u8> {
        let mut s = Vec::new();
        s.extend((0x4800_000c_u32).to_be_bytes());
        s.extend(((1 << 28) | (XAS1 << 24) | ((channels - 1) << 18) | 48_000).to_be_bytes());
        s.extend(total.to_be_bytes());
        for &n in blocks {
            let frames = (n as usize).div_ceil(FRAME_SAMPLES);
            let size = 8 + frames * FRAME * channels as usize;
            s.extend((0x4400_0000 | size as u32).to_be_bytes());
            s.extend(n.to_be_bytes());
            for f in 0..frames * channels as usize {
                // Channel c of every frame: constant history 0x10 * (c + 1).
                let c = (f % channels as usize) as u32 + 1;
                let mut frame = [0u8; FRAME];
                for g in 0..4 {
                    frame[g * 4..g * 4 + 4].copy_from_slice(&((0x10 * c) | (0x10 * c) << 16).to_le_bytes());
                }
                s.extend(frame);
            }
        }
        s.extend(0x4500_0004_u32.to_be_bytes());
        s
    }

    #[test]
    fn blocks_interleave_channels_and_trim_to_the_header_sample_count() {
        let s = stream(2, 200, &[128, 128]);
        let pcm = decode_sps(&s).unwrap();
        assert_eq!((pcm.sample_rate, pcm.channels, pcm.frames()), (48_000, 2, 200));
        // Interleaved: left frames carry 0x10, right 0x20 as their first samples.
        assert_eq!(&pcm.samples[..4], &[0x10, 0x20, 0x10, 0x20]);
        let wav = pcm.to_wav();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 200 * 2 * 2);
    }

    #[test]
    fn short_streams_and_other_codecs_are_errors() {
        let s = stream(1, 300, &[128]);
        assert!(decode_sps(&s).is_err());
        let mut s = stream(1, 128, &[128]);
        s[4] = 0x15; // codec 5: EALayer3
        assert!(matches!(decode_sps(&s), Err(Error::Unsupported(_))));
        assert!(decode_sps(&s[..10]).is_err());
    }
}
