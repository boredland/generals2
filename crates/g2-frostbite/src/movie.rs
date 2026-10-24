//! EA VP6 movies (`MVhd`): a run of little-endian tagged blocks (`MVhd` header, `MV0K` key
//! frame, `MV0F` frame), optionally a second VP6 stream holding the alpha matte (`ALhd`,
//! `AL0K`, `AL0F`, used by the talking-head portraits), and an EA "SCxl" audio stream
//! (`SCHl` header, `SCCl` count, `SCDl` data). ffmpeg reads the video; it fails on this
//! audio, so the audio is decoded here and the importer muxes the two.
//!
//! The SCHl header is "GSTR", a 4-byte platform word, then a tagged list (tag, length,
//! big-endian value; 0xFD opens the audio section, 0xFF ends it). The fields used are 0x82
//! channel count, 0x84 sample rate and 0xA0 codec. Codec 0x0A is EA-XA v2: each `SCDl`
//! holds a big-endian sample count, one big-endian offset per channel, then per channel
//! frames of a filter/shift byte and 15 bytes of nibbles (28 samples), or `0xEE`, two
//! history samples and 28 raw big-endian samples. Decoder ported from vgmstream
//! (`src/coding/ea_xa_decoder.c`, r2117; ISC licence, notice in `audio.rs`), and checked to
//! match it sample for sample. Codec 0x17 (EALayer3, MPEG audio in EA's own framing) is
//! reported as unsupported.

use crate::Error;
use crate::audio::Pcm;

pub struct Movie {
    /// The colour stream: only `MVhd`/`MV0K`/`MV0F`.
    pub video: Vec<u8>,
    /// The alpha matte as its own VP6 stream (tags renamed to `MV..` so ffmpeg reads it).
    pub alpha: Option<Vec<u8>>,
    pub audio: Option<Result<Pcm, Error>>,
}

pub fn split(bytes: &[u8]) -> Result<Movie, Error> {
    if !bytes.starts_with(b"MVhd") {
        return Err(Error::Bad("not an EA VP6 movie"));
    }
    let mut video = Vec::with_capacity(bytes.len());
    let mut alpha = Vec::new();
    let mut header = None;
    let mut data = Vec::new();
    let mut p = 0;
    while p < bytes.len() {
        let block = bytes.get(p..p + 8).ok_or(Error::Truncated)?;
        let size = u32::from_le_bytes(block[4..8].try_into().unwrap()) as usize;
        if size < 8 {
            return Err(Error::Bad("movie block too small"));
        }
        let body = bytes.get(p..p + size).ok_or(Error::Truncated)?;
        match &block[..4] {
            b"MVhd" | b"MV0K" | b"MV0F" => video.extend_from_slice(body),
            b"ALhd" | b"AL0K" | b"AL0F" => {
                alpha.extend_from_slice(b"MV");
                alpha.extend_from_slice(&body[2..]);
            }
            b"SCHl" => header = Some(body),
            b"SCDl" => data.push(body),
            b"SCCl" | b"SCEl" => {}
            other => return Err(Error::Unsupported(format!("movie block {:?}", String::from_utf8_lossy(other)))),
        }
        p += size;
    }
    Ok(Movie { video, alpha: (!alpha.is_empty()).then_some(alpha), audio: header.map(|h| decode_audio(h, &data)) })
}

fn decode_audio(header: &[u8], blocks: &[&[u8]]) -> Result<Pcm, Error> {
    let (mut channels, mut sample_rate, mut codec) = (1usize, 22_050u32, None);
    if header.get(8..12) != Some(b"GSTR") {
        return Err(Error::Unsupported("movie audio header is not GSTR".into()));
    }
    let mut p = 16;
    while let Some(&tag) = header.get(p) {
        p += 1;
        match tag {
            0xff => break,
            0xfc..=0xfe => continue,
            _ => {}
        }
        let n = usize::from(*header.get(p).ok_or(Error::Truncated)?);
        let value =
            header.get(p + 1..p + 1 + n).ok_or(Error::Truncated)?.iter().fold(0u32, |v, &b| v << 8 | u32::from(b));
        match tag {
            0x82 => channels = value as usize,
            0x84 => sample_rate = value,
            0xa0 => codec = Some(value),
            _ => {}
        }
        p += 1 + n;
    }
    if codec != Some(0x0a) {
        return Err(Error::Unsupported(format!("movie audio codec {codec:#x?}")));
    }
    if channels == 0 || channels > 8 {
        return Err(Error::Bad("movie audio channel count"));
    }
    let mut per_channel = vec![Vec::new(); channels];
    let mut hist = vec![(0i32, 0i32); channels];
    for b in blocks {
        let be = |at: usize| -> Result<usize, Error> {
            Ok(u32::from_be_bytes(b.get(at..at + 4).ok_or(Error::Truncated)?.try_into().unwrap()) as usize)
        };
        let samples = be(8)?;
        for c in 0..channels {
            let start = 12 + 4 * channels + be(12 + 4 * c)?;
            xa_v2(b.get(start..).ok_or(Error::Truncated)?, samples, &mut hist[c], &mut per_channel[c])?;
        }
    }
    let frames = per_channel[0].len();
    let mut samples = Vec::with_capacity(frames * channels);
    for i in 0..frames {
        for ch in &per_channel {
            samples.push(ch[i]);
        }
    }
    Ok(Pcm { sample_rate, channels: channels as u16, samples })
}

const XA_COEFS: [(i32, i32); 4] = [(0, 0), (240, 0), (460, -208), (392, -220)];

/// Decodes `count` samples of one channel's EA-XA v2 frames, carrying the filter history.
fn xa_v2(mut frames: &[u8], count: usize, hist: &mut (i32, i32), out: &mut Vec<i16>) -> Result<(), Error> {
    let mut left = count;
    while left > 0 {
        let take = left.min(28);
        let info = *frames.first().ok_or(Error::Truncated)?;
        if info == 0xee {
            let raw = frames.get(1..61).ok_or(Error::Truncated)?;
            let be = |i: usize| i16::from_be_bytes([raw[2 * i], raw[2 * i + 1]]);
            *hist = (be(0).into(), be(1).into());
            out.extend((0..take).map(|i| be(2 + i)));
            frames = &frames[61..];
        } else {
            let nibbles = frames.get(1..15).ok_or(Error::Truncated)?;
            let (c1, c2) = XA_COEFS.get(usize::from(info >> 4)).copied().unwrap_or((0, 0));
            let shift = u32::from(info & 0x0f) + 8;
            for i in 0..take {
                let byte = nibbles[i / 2];
                let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0x0f };
                let s = ((i32::from(nibble) << 28) >> shift) + c1 * hist.0 + c2 * hist.1;
                let s = (s >> 8).clamp(i16::MIN.into(), i16::MAX.into());
                out.push(s as i16);
                *hist = (s, hist.0);
            }
            frames = &frames[15..];
        }
        left -= take;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adpcm_frame_filters_with_history_and_raw_frames_reset_it() {
        // Filter 1 (240/256), shift 4 + 8: nibble 1 -> (1 << 28) >> 12 = 65536.
        let mut frame = vec![0x14, 0x10];
        frame.extend([0u8; 13]);
        let mut hist = (256, 0);
        let mut out = Vec::new();
        xa_v2(&frame, 2, &mut hist, &mut out).unwrap();
        // (65536 + 240 * 256) >> 8 = 496, then (0 + 240 * 496) >> 8 = 465.
        assert_eq!(out, [496, 465]);
        assert_eq!(hist, (465, 496));

        let mut raw = vec![0xee];
        for v in [7i16, -7].into_iter().chain((0..28).map(|i| i as i16 * 100)) {
            raw.extend(v.to_be_bytes());
        }
        out.clear();
        xa_v2(&raw, 28, &mut hist, &mut out).unwrap();
        assert_eq!(out[..3], [0, 100, 200]);
        assert_eq!(hist, (7, -7));
    }

    fn block(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut b = tag.to_vec();
        b.extend(((body.len() + 8) as u32).to_le_bytes());
        b.extend(body);
        b
    }

    #[test]
    fn split_keeps_video_blocks_and_decodes_interleaved_audio() {
        let mut movie = block(b"MVhd", b"vp60....");
        // GSTR header: platform word, then audio section: 2 channels, 48 kHz, codec 0x0A.
        let gstr =
            [b"GSTR".as_slice(), &[1, 0, 0, 0, 0xfd, 0x82, 1, 2, 0x84, 3, 0x00, 0xbb, 0x80, 0xa0, 1, 0x0a, 0xff]];
        movie.extend(block(b"SCHl", &gstr.concat()));
        movie.extend(block(b"MV0K", b"key"));
        // One SCDl: 28 samples, channel 1 starts 61 bytes after channel 0; raw frames.
        let mut d = 28u32.to_be_bytes().to_vec();
        d.extend(0u32.to_be_bytes());
        d.extend(61u32.to_be_bytes());
        for c in 0..2i16 {
            d.push(0xee);
            d.extend([0u8; 4]);
            for i in 0..28i16 {
                d.extend((i + 1000 * c).to_be_bytes());
            }
        }
        movie.extend(block(b"SCDl", &d));
        movie.extend(block(b"MV0F", b"frame"));
        let m = split(&movie).unwrap();
        assert_eq!(m.video, [block(b"MVhd", b"vp60...."), block(b"MV0K", b"key"), block(b"MV0F", b"frame")].concat());
        let a = m.audio.unwrap().unwrap();
        assert_eq!((a.channels, a.sample_rate, a.frames()), (2, 48_000, 28));
        assert_eq!(a.samples[..4], [0, 1000, 1, 1001]);
    }

    #[test]
    fn alpha_matte_is_its_own_stream_and_other_audio_codecs_are_reported() {
        let mut movie = block(b"MVhd", b"colour");
        movie.extend(block(b"ALhd", b"matte"));
        movie.extend(block(b"AL0K", b"akey"));
        let gstr = [b"GSTR".as_slice(), &[1, 0, 0, 0, 0xfd, 0xa0, 1, 0x17, 0xff]];
        movie.extend(block(b"SCHl", &gstr.concat()));
        let m = split(&movie).unwrap();
        assert_eq!(m.alpha.unwrap(), [block(b"MVhd", b"matte"), block(b"MV0K", b"akey")].concat());
        assert!(matches!(m.audio, Some(Err(Error::Unsupported(_)))));
    }

    #[test]
    fn unknown_blocks_and_truncation_are_errors() {
        let mut movie = block(b"MVhd", b"x");
        movie.extend(block(b"WHAT", b""));
        assert!(split(&movie).is_err());
        let movie = block(b"MVhd", b"xxxx");
        assert!(split(&movie[..movie.len() - 1]).is_err());
    }
}
