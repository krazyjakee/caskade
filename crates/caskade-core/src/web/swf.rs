//! A Flash movie's stage size, from its header (SWF 19, "SWF header"):
//! the frame size is the first thing after the 8 bytes every movie starts
//! with, compressed with zlib in a `CWS` movie. LZMA movies (`ZWS`) aren't
//! read: their page's size has to do.

use super::GameSize;
use std::io::Read;

/// The stage size of the movie `reader` starts at, in pixels.
pub fn stage_size(mut reader: impl Read) -> Option<GameSize> {
    let mut header = [0; 8];
    reader.read_exact(&mut header).ok()?;
    // A RECT: 5 bits of field size, then four fields that wide.
    let mut rect = [0; 17];
    match &header[..3] {
        b"FWS" => read_some(&mut reader, &mut rect)?,
        b"CWS" => read_some(flate2::read::ZlibDecoder::new(reader), &mut rect)?,
        _ => return None,
    }
    let mut bits = Bits {
        bytes: &rect,
        at: 0,
    };
    let width = bits.take(5)?;
    let [x_min, x_max, y_min, y_max] = [(); 4].map(|()| bits.signed(width));
    let twips = |min: Option<i64>, max: Option<i64>| u32::try_from((max? - min?) / 20).ok();
    GameSize::new(twips(x_min, x_max)?, twips(y_min, y_max)?)
}

/// Fills as much of `buf` as `reader` has: a tiny movie's RECT can be
/// shorter than the most a RECT can take.
fn read_some(mut reader: impl Read, buf: &mut [u8]) -> Option<()> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]).ok()? {
            0 => break,
            read => filled += read,
        }
    }
    (filled > 0).then_some(())
}

/// Big-endian bits, as SWF packs them.
struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn take(&mut self, count: u32) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            let byte = self.bytes.get(self.at / 8)?;
            let bit = (byte >> (7 - self.at % 8)) & 1;
            value = (value << 1) | u32::from(bit);
            self.at += 1;
        }
        Some(value)
    }

    fn signed(&mut self, count: u32) -> Option<i64> {
        if count == 0 {
            return Some(0);
        }
        let value = i64::from(self.take(count)?);
        let sign = 1i64 << (count - 1);
        Some(if value & sign != 0 {
            value - (sign << 1)
        } else {
            value
        })
    }
}

#[cfg(test)]
mod tests {
    use super::stage_size;
    use crate::web::GameSize;
    use std::io::Write;

    /// A movie's header and RECT: 0..width and 0..height pixels, in twips.
    fn movie(signature: &[u8; 3], width: u32, height: u32) -> Vec<u8> {
        let fields = [0, width * 20, 0, height * 20];
        let size = 16u32;
        let mut bits = Vec::new();
        bits.extend((0..5).rev().map(|bit| (size >> bit) & 1));
        for field in fields {
            bits.extend((0..size).rev().map(|bit| (field >> bit) & 1));
        }
        let rect: Vec<u8> = bits
            .chunks(8)
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0u8, |byte, (i, bit)| byte | ((*bit as u8) << (7 - i)))
            })
            .collect();
        let mut body = rect;
        body.extend([0; 64]);
        let mut file = signature.to_vec();
        file.extend([10, 0, 0, 0, 0]);
        if signature == b"CWS" {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&body).unwrap();
            file.extend(encoder.finish().unwrap());
        } else {
            file.extend(body);
        }
        file
    }

    #[test]
    fn a_movie_says_how_big_its_stage_is() {
        let size = GameSize::new(640, 480);
        assert_eq!(stage_size(&movie(b"FWS", 640, 480)[..]), size);
        assert_eq!(stage_size(&movie(b"CWS", 640, 480)[..]), size);
        assert_eq!(
            stage_size(&movie(b"CWS", 550, 400)[..]),
            GameSize::new(550, 400)
        );
        assert_eq!(stage_size(&movie(b"ZWS", 640, 480)[..]), None);
        assert_eq!(stage_size(&b"<html>"[..]), None);
        assert_eq!(
            stage_size(&movie(b"FWS", 1, 1)[..]),
            None,
            "too small to play"
        );
    }
}
