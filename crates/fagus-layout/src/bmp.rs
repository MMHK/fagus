//! BMP images, read as Chrome reads them (Chrome 157, measured): the Windows headers from
//! `BITMAPINFOHEADER` to `BITMAPV5HEADER` and both OS/2 ones, 1, 2, 4 and 8 bits per pixel with a
//! color table, 16, 24 and 32 bits with or without bit fields, RLE8 and RLE4, bottom-up and
//! top-down. What Chrome does where the format leaves room:
//!
//! - A 32-bit image has an alpha channel only where a mask says which bits are alpha: with
//!   `BI_BITFIELDS` and a header of 56 bytes or more, with `BI_ALPHABITFIELDS`, or with `BI_RGB`
//!   and a `BITMAPV4HEADER` or `BITMAPV5HEADER`. The fourth byte of a plain `BI_RGB` image is
//!   ignored, and an alpha mask of all zero alpha is transparent.
//! - A channel of fewer than 8 bits is scaled to the nearest 8-bit value (5 bits of 16 give 132,
//!   not 128); one of more than 8 keeps its top 8.
//! - A color index past the color table is black, and so are the pixels that RLE skips.
//! - Rows that the data ends before are transparent; a row counts only once all its bytes (with
//!   the padding to four bytes) are there.
//! - Images with embedded JPEG or PNG data, OS/2 Huffman and RLE24 compression are not shown.

/// The format of the pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compression {
    /// Uncompressed, the channels in masks.
    Fields,
    Rle8,
    Rle4,
}

/// What a BMP header says about its pixels.
#[derive(Debug)]
struct Header {
    width: u32,
    height: u32,
    top_down: bool,
    bits: u16,
    compression: Compression,
    /// The bits of red, green, blue and alpha in a pixel of 16 or 32 bits.
    masks: [u32; 4],
    /// The color table, for 8 bits per pixel and fewer.
    palette: Vec<[u8; 3]>,
    /// Where the pixels start.
    offset: usize,
}

/// The width and height of a BMP image, from its header.
pub fn size(data: &[u8]) -> Option<(u32, u32)> {
    header(data).map(|header| (header.width, header.height))
}

/// Decodes a BMP image to 8-bit RGBA, row by row from the top: its width, height and pixels.
/// `None` for data that is not a BMP image Chrome shows, and for images of more than
/// [`crate::MAX_PIXELS`].
pub fn decode(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let header = header(data)?;
    let (width, height) = (header.width as usize, header.height as usize);
    if (width as u64).saturating_mul(height as u64) > crate::MAX_PIXELS {
        return None;
    }
    let pixels = data.get(header.offset..).unwrap_or_default();
    let mut out = vec![0_u8; width.checked_mul(height)?.checked_mul(4)?];
    match header.compression {
        Compression::Fields => fields(&header, pixels, &mut out),
        Compression::Rle8 | Compression::Rle4 => rle(&header, pixels, &mut out),
    }
    Some((header.width, header.height, out))
}

fn header(data: &[u8]) -> Option<Header> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let i32_at = |at: usize| Some(i32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?));
    if data.get(..2)? != b"BM" {
        return None;
    }
    let offset = u32_at(10)? as usize;
    let size = u32_at(14)? as usize;
    // OS/2 1.x has a header of 12 bytes with sizes of 16 bits; OS/2 2.x one of 16 to 64 bytes;
    // Windows ones of 40, 52, 56, 108 and 124 bytes (and more, read as 124).
    let os2 = !matches!(size, 40 | 52 | 56 | 108) && size < 124;
    let (width, height, bits, kind, used) = if size == 12 {
        let (width, height) = (u16_at(18)?, u16_at(20)?);
        (i64::from(width), i64::from(height), u16_at(24)?, 0, 0)
    } else if size >= 16 {
        let kind = if size >= 20 { u32_at(30)? } else { 0 };
        let used = if size >= 36 { u32_at(46)? } else { 0 };
        (
            i64::from(i32_at(18)?),
            i64::from(i32_at(22)?),
            u16_at(28)?,
            kind,
            used,
        )
    } else {
        return None;
    };
    if width <= 0 || height == 0 || offset < size.saturating_add(14) {
        return None;
    }
    // BI_BITFIELDS and BI_ALPHABITFIELDS have masks (in OS/2 2.x, 3 is Huffman 1D).
    let bit_fields = matches!(kind, 3 | 6) && !os2;
    let compression = match (kind, bits) {
        (0, 1 | 2 | 4 | 8 | 16 | 24 | 32) => Compression::Fields,
        (1, 8) => Compression::Rle8,
        (2, 4) => Compression::Rle4,
        (3 | 6, 16 | 32) if bit_fields => Compression::Fields,
        _ => return None,
    };
    let masks = if bit_fields {
        // In the header from 52 bytes on (alpha from 56), else after it (alpha with
        // BI_ALPHABITFIELDS).
        let at = if size >= 52 { 54 } else { size + 14 };
        let alpha = if size >= 56 {
            u32_at(66)?
        } else if kind == 6 && size < 52 {
            u32_at(at + 12)?
        } else {
            0
        };
        [u32_at(at)?, u32_at(at + 4)?, u32_at(at + 8)?, alpha]
    } else if bits == 16 {
        [0x7C00, 0x03E0, 0x001F, 0]
    } else {
        // A 32-bit BI_RGB image takes the alpha mask of a V4 or V5 header.
        let alpha = if bits == 32 && size >= 108 { u32_at(66)? } else { 0 };
        [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, alpha]
    };
    let mut palette = Vec::new();
    if bits <= 8 {
        let most = 1_u32 << bits;
        let colors = if size != 12 && used > 0 && used <= most {
            used
        } else {
            most
        };
        let entry = if size == 12 { 3 } else { 4 };
        let start = 14 + size;
        let table = data.get(start..start + colors as usize * entry)?;
        palette = table
            .chunks_exact(entry)
            .map(|bgr| [bgr[2], bgr[1], bgr[0]])
            .collect();
    }
    Some(Header {
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height.unsigned_abs()).ok()?,
        top_down: height < 0,
        bits,
        compression,
        masks,
        palette,
        offset,
    })
}

/// The value of the bits of `mask` in `pixel`, as 8 bits: fewer are scaled to the nearest value,
/// more keep their top 8. `None` for an empty mask.
fn channel(pixel: u32, mask: u32) -> Option<u8> {
    if mask == 0 {
        return None;
    }
    let shift = mask.trailing_zeros();
    let length = 32 - mask.leading_zeros() - shift;
    let value = u64::from((pixel & mask) >> shift);
    Some(match length {
        8 => value as u8,
        9.. => (value >> (length - 8)) as u8,
        _ => {
            let most = (1_u64 << length) - 1;
            ((value * 510 + most) / (2 * most)).min(255) as u8
        }
    })
}

/// The row of the image that stored row `row` is, counted from the top.
fn row_from_top(header: &Header, row: usize) -> usize {
    if header.top_down {
        row
    } else {
        header.height as usize - 1 - row
    }
}

/// Decodes uncompressed pixels into `out`, rows that the data ends before left transparent.
fn fields(header: &Header, pixels: &[u8], out: &mut [u8]) {
    let width = header.width as usize;
    let bits = usize::from(header.bits);
    // Each row is padded to four bytes.
    let stride = (width * bits).div_ceil(32) * 4;
    for (row, line) in pixels
        .chunks_exact(stride)
        .take(header.height as usize)
        .enumerate()
    {
        let start = row_from_top(header, row) * width * 4;
        let target = &mut out[start..start + width * 4];
        for (column, pixel) in target.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *pixel = match bits {
                1 | 2 | 4 | 8 => {
                    let at = column * bits;
                    let index = (line[at / 8] >> (8 - bits - at % 8)) & ((1 << bits) - 1) as u8;
                    let [r, g, b] = header
                        .palette
                        .get(usize::from(index))
                        .copied()
                        .unwrap_or_default();
                    [r, g, b, 255]
                }
                24 => {
                    let [b, g, r] = [line[column * 3], line[column * 3 + 1], line[column * 3 + 2]];
                    [r, g, b, 255]
                }
                _ => {
                    let value = if bits == 16 {
                        u32::from(u16::from_le_bytes([line[column * 2], line[column * 2 + 1]]))
                    } else {
                        u32::from_le_bytes(line[column * 4..column * 4 + 4].try_into().unwrap_or_default())
                    };
                    let [r, g, b, a] = header.masks.map(|mask| channel(value, mask));
                    [r.unwrap_or(0), g.unwrap_or(0), b.unwrap_or(0), a.unwrap_or(255)]
                }
            };
        }
    }
}

/// Decodes RLE8 or RLE4 pixels into `out`. Pixels that the data skips are black; where it ends
/// before the end-of-bitmap code, the rows from the one it ends in on are transparent.
fn rle(header: &Header, data: &[u8], out: &mut [u8]) {
    const SKIPPED: u16 = u16::MAX;
    let (width, height) = (header.width as usize, header.height as usize);
    let mut indices = vec![SKIPPED; width * height];
    let four = header.compression == Compression::Rle4;
    let (mut x, mut row, mut at) = (0_usize, 0_usize, 0_usize);
    let mut put = |x: usize, row: usize, index: u8| {
        if x < width && row < height {
            indices[row * width + x] = u16::from(index);
        }
    };
    let complete = loop {
        if row >= height {
            break height;
        }
        let (Some(&count), Some(&value)) = (data.get(at), data.get(at + 1)) else {
            break row;
        };
        at += 2;
        match (count, value) {
            (0, 0) => {
                row += 1;
                x = 0;
            }
            (0, 1) => break height,
            (0, 2) => {
                let (Some(&right), Some(&up)) = (data.get(at), data.get(at + 1)) else {
                    break row;
                };
                at += 2;
                x += usize::from(right);
                row += usize::from(up);
            }
            (0, count) => {
                // Absolute mode: `count` pixels as they are, padded to an even number of bytes.
                let count = usize::from(count);
                let bytes = if four { count.div_ceil(2) } else { count };
                let Some(literal) = data.get(at..at + bytes) else {
                    break row;
                };
                for pixel in 0..count {
                    let index = if four {
                        (literal[pixel / 2] >> if pixel % 2 == 0 { 4 } else { 0 }) & 0x0F
                    } else {
                        literal[pixel]
                    };
                    put(x + pixel, row, index);
                }
                x += count;
                at += bytes + bytes % 2;
            }
            (count, value) => {
                for pixel in 0..usize::from(count) {
                    let index = if four {
                        if pixel % 2 == 0 { value >> 4 } else { value & 0x0F }
                    } else {
                        value
                    };
                    put(x + pixel, row, index);
                }
                x += usize::from(count);
            }
        }
    };
    for (row, line) in indices.chunks_exact(width).take(complete).enumerate() {
        let start = row_from_top(header, row) * width * 4;
        for (pixel, index) in out[start..start + width * 4]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(line)
        {
            let [r, g, b] = header
                .palette
                .get(usize::from(*index))
                .copied()
                .unwrap_or_default();
            *pixel = [r, g, b, 255];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A BMP file of a `size`-byte info header, from its fields after the size, the masks or
    /// color table after it and the pixels.
    fn file(info: &[u8], table: &[u8], pixels: &[u8]) -> Vec<u8> {
        let offset = 14 + 4 + info.len() + table.len();
        let mut data = b"BM".to_vec();
        data.extend(((offset + pixels.len()) as u32).to_le_bytes());
        data.extend([0; 4]);
        data.extend((offset as u32).to_le_bytes());
        data.extend(((4 + info.len()) as u32).to_le_bytes());
        data.extend(info);
        data.extend(table);
        data.extend(pixels);
        data
    }

    /// The fields of a 40-byte header after its size, up to the number of colors used.
    fn info(width: i32, height: i32, bits: u16, compression: u32, used: u32) -> Vec<u8> {
        let mut info = Vec::new();
        info.extend(width.to_le_bytes());
        info.extend(height.to_le_bytes());
        info.extend(1_u16.to_le_bytes());
        info.extend(bits.to_le_bytes());
        info.extend(compression.to_le_bytes());
        info.extend([0; 12]);
        info.extend(used.to_le_bytes());
        info.extend([0; 4]);
        info
    }

    /// A 40-byte header grown to `size` bytes, with `masks` from byte 40 on.
    fn grown(mut info: Vec<u8>, size: usize, masks: &[u32]) -> Vec<u8> {
        for mask in masks {
            info.extend(mask.to_le_bytes());
        }
        info.resize(size - 4, 0);
        info
    }

    fn pixels(data: &[u8]) -> Vec<[u8; 4]> {
        let (_, _, pixels) = decode(data).expect("decodes");
        pixels.as_chunks::<4>().0.to_vec()
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];
    const CLEAR: [u8; 4] = [0; 4];
    /// A color table of red, green and blue.
    const TABLE: [u8; 12] = [0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0];

    #[test]
    fn reads_rows_from_the_bottom_or_the_top() {
        // Two rows of one pixel and three bytes of padding each: red stored first.
        let rows = [0, 0, 255, 0, 0, 255, 0, 0];
        let up = file(&info(1, 2, 24, 0, 0), &[], &rows);
        assert_eq!(size(&up), Some((1, 2)));
        assert_eq!(pixels(&up), [GREEN, RED]);
        let down = file(&info(1, -2, 24, 0, 0), &[], &rows);
        assert_eq!(size(&down), Some((1, 2)));
        assert_eq!(pixels(&down), [RED, GREEN]);
    }

    #[test]
    fn looks_colors_up_in_the_color_table() {
        // 1 bit: 9 pixels cross a byte.
        let table = [30, 20, 10, 0, 220, 210, 200, 0];
        let one = file(&info(9, 1, 1, 0, 2), &table, &[0b0101_0101, 0b1000_0000, 0, 0]);
        let dark = [10, 20, 30, 255];
        let light = [200, 210, 220, 255];
        assert_eq!(
            pixels(&one),
            [dark, light, dark, light, dark, light, dark, light, light]
        );
        // 2 and 4 bits, and an index past the table, black.
        let two = file(&info(4, 1, 2, 0, 3), &TABLE, &[0b0001_1011, 0, 0, 0]);
        assert_eq!(pixels(&two), [RED, GREEN, BLUE, BLACK]);
        let four = file(&info(3, 1, 4, 0, 3), &TABLE, &[0x01, 0x20, 0, 0]);
        assert_eq!(pixels(&four), [RED, GREEN, BLUE]);
        let eight = file(&info(4, 1, 8, 0, 3), &TABLE, &[0, 1, 2, 7]);
        assert_eq!(pixels(&eight), [RED, GREEN, BLUE, BLACK]);
        // Without a number of colors, the table has all 256: these 3 are too few.
        assert!(decode(&file(&info(4, 1, 8, 0, 0), &TABLE, &[0, 1, 2, 7])).is_none());
    }

    #[test]
    fn reads_os2_headers() {
        // OS/2 1.x: sizes of 16 bits and a table of three bytes per color, always all of them.
        let mut header = Vec::new();
        header.extend(3_u16.to_le_bytes());
        header.extend(1_u16.to_le_bytes());
        header.extend(1_u16.to_le_bytes());
        header.extend(8_u16.to_le_bytes());
        let mut table = vec![0, 0, 255, 0, 255, 0, 255, 0, 0];
        table.resize(256 * 3, 0);
        let os2 = file(&header, &table, &[0, 1, 2, 0]);
        assert_eq!(size(&os2), Some((3, 1)));
        assert_eq!(pixels(&os2), [RED, GREEN, BLUE]);
        assert!(decode(&file(&header, &table[..9], &[0, 1, 2, 0])).is_none());
        // OS/2 2.x, 64 bytes.
        let v2 = file(&grown(info(1, 1, 24, 0, 0), 64, &[]), &[], &[255, 0, 0, 0]);
        assert_eq!(pixels(&v2), [BLUE]);
    }

    #[test]
    fn scales_channels_to_8_bits_as_chrome() {
        // 5-5-5 without bit fields: red 1, 3, 16 and 31 draw as 8, 25, 132 and 255 in Chrome.
        let pixels_555: Vec<u8> = [1_u16 << 10, 3 << 10, 16 << 10, 0x7FFF]
            .iter()
            .flat_map(|pixel| pixel.to_le_bytes())
            .collect();
        let red: Vec<u8> = pixels(&file(&info(4, 1, 16, 0, 0), &[], &pixels_555))
            .iter()
            .map(|pixel| pixel[0])
            .collect();
        assert_eq!(red, [8, 25, 132, 255]);
        // 5-6-5 bit fields: green 1, 3 and 33 draw as 4, 12 and 134.
        let pixels_565: Vec<u8> = [1_u16 << 5, 3 << 5, 33 << 5, 0xFFFF]
            .iter()
            .flat_map(|pixel| pixel.to_le_bytes())
            .collect();
        let masks: Vec<u8> = [0xF800_u32, 0x07E0, 0x001F]
            .iter()
            .flat_map(|mask| mask.to_le_bytes())
            .collect();
        let green: Vec<u8> = pixels(&file(&info(4, 1, 16, 3, 0), &masks, &pixels_565))
            .iter()
            .map(|pixel| pixel[1])
            .collect();
        assert_eq!(green, [4, 12, 134, 255]);
        // 10 bits keep their top 8: 1023, 512 and 3 draw as 255, 128 and 0.
        assert_eq!(channel(1023 << 20, 0x3FF0_0000), Some(255));
        assert_eq!(channel(512 << 10, 0x000F_FC00), Some(128));
        assert_eq!(channel(3, 0x3FF), Some(0));
        assert_eq!(channel(0xFFFF_FFFF, 0), None);
    }

    #[test]
    fn takes_alpha_only_from_an_alpha_mask() {
        // Red opaque, green half transparent, blue transparent.
        let bgra = [0, 0, 255, 255, 0, 255, 0, 128, 255, 0, 0, 0];
        let half = [0, 255, 0, 128];
        let invisible = [0, 0, 255, 0];
        // BI_RGB with a 40-byte header: the fourth byte is not alpha.
        assert_eq!(
            pixels(&file(&info(3, 1, 32, 0, 0), &[], &bgra)),
            [RED, GREEN, BLUE]
        );
        // BI_RGB with a V4 header takes its alpha mask, a V5 header without one none.
        let masks = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000];
        let v4 = grown(info(3, 1, 32, 0, 0), 108, &masks);
        assert_eq!(pixels(&file(&v4, &[], &bgra)), [RED, half, invisible]);
        let v5 = grown(info(3, 1, 32, 0, 0), 124, &masks[..3]);
        assert_eq!(pixels(&file(&v5, &[], &bgra)), [RED, GREEN, BLUE]);
        // BI_BITFIELDS with masks in a header of 56 bytes, or 52 without alpha.
        let v3 = grown(info(3, 1, 32, 3, 0), 56, &masks);
        assert_eq!(pixels(&file(&v3, &[], &bgra)), [RED, half, invisible]);
        let v2 = grown(info(3, 1, 32, 3, 0), 52, &masks);
        assert_eq!(pixels(&file(&v2, &[], &bgra)), [RED, GREEN, BLUE]);
        // After a 40-byte header: three masks for BI_BITFIELDS, four for BI_ALPHABITFIELDS.
        let after: Vec<u8> = masks.iter().flat_map(|mask| mask.to_le_bytes()).collect();
        let three = file(&info(3, 1, 32, 3, 0), &after[..12], &bgra);
        assert_eq!(pixels(&three), [RED, GREEN, BLUE]);
        let four = file(&info(3, 1, 32, 6, 0), &after, &bgra);
        assert_eq!(pixels(&four), [RED, half, invisible]);
        // All alpha zero stays transparent.
        let one = grown(info(1, 1, 32, 3, 0), 56, &masks);
        let zero = file(&one, &[], &[0, 0, 255, 0]);
        assert_eq!(pixels(&zero), [[255, 0, 0, 0]]);
    }

    #[test]
    fn decodes_rle_as_chrome() {
        // 4 × 3 RLE8: two red then the end of the row; a jump of two, two green, the end of the
        // row; four blue; the end. Skipped pixels are black.
        let rle8 = [2, 0, 0, 0, 0, 2, 2, 0, 2, 1, 0, 0, 4, 2, 0, 1];
        let data = file(&info(4, 3, 8, 1, 3), &TABLE, &rle8);
        assert_eq!(
            pixels(&data),
            [
                BLUE, BLUE, BLUE, BLUE, BLACK, BLACK, GREEN, GREEN, RED, RED, BLACK, BLACK
            ]
        );
        // Absolute runs, padded to even bytes, and RLE4.
        let absolute = file(&info(4, 1, 8, 1, 3), &TABLE, &[0, 3, 0, 1, 2, 0, 1, 1, 0, 1]);
        assert_eq!(pixels(&absolute), [RED, GREEN, BLUE, GREEN]);
        let rle4 = file(&info(4, 1, 4, 2, 3), &TABLE, &[0, 4, 0x01, 0x21, 0, 1]);
        assert_eq!(pixels(&rle4), [RED, GREEN, BLUE, GREEN]);
        let runs4 = file(&info(3, 1, 4, 2, 3), &TABLE, &[3, 0x12, 0, 1]);
        assert_eq!(pixels(&runs4), [GREEN, BLUE, GREEN]);
        // Top-down.
        let down = file(&info(4, -2, 8, 1, 3), &TABLE, &[4, 1, 0, 0, 4, 2, 0, 1]);
        assert_eq!(
            pixels(&down),
            [GREEN, GREEN, GREEN, GREEN, BLUE, BLUE, BLUE, BLUE]
        );
        // Without the end, the row the data ends in and those above are transparent.
        let cut = file(&info(2, 2, 8, 1, 3), &TABLE, &[2, 1, 0, 0, 1, 2]);
        assert_eq!(pixels(&cut), [CLEAR, CLEAR, GREEN, GREEN]);
    }

    #[test]
    fn leaves_rows_the_data_ends_before_transparent() {
        let rows = [0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0];
        assert_eq!(
            pixels(&file(&info(1, 3, 24, 0, 0), &[], &rows[..11])),
            [CLEAR, GREEN, RED]
        );
        assert_eq!(
            pixels(&file(&info(1, 3, 24, 0, 0), &[], &[])),
            [CLEAR, CLEAR, CLEAR]
        );
    }

    #[test]
    fn refuses_what_chrome_does_not_show() {
        // Not BMP, no width, no height, a header that is too short, a depth of 7, RLE8 at 4 bits,
        // PNG compression, Huffman (3 in OS/2 2.x), a pixel offset inside the header.
        assert!(decode(b"PNG").is_none());
        assert!(decode(&file(&info(0, 1, 24, 0, 0), &[], &[0; 4])).is_none());
        assert!(decode(&file(&info(1, 0, 24, 0, 0), &[], &[0; 4])).is_none());
        assert!(decode(&file(&info(-1, 1, 24, 0, 0), &[], &[0; 4])).is_none());
        assert!(decode(&file(&info(1, 1, 24, 0, 0)[..4], &[], &[0; 4])).is_none());
        assert!(decode(&file(&info(1, 1, 7, 0, 0), &[], &[0; 4])).is_none());
        assert!(decode(&file(&info(1, 1, 4, 1, 3), &TABLE, &[0; 4])).is_none());
        assert!(decode(&file(&info(1, 1, 0, 5, 0), &[], &[0; 4])).is_none());
        assert!(decode(&file(&grown(info(1, 1, 32, 3, 0), 64, &[]), &[], &[0; 4])).is_none());
        let mut inside = file(&info(1, 1, 24, 0, 0), &[], &[0; 4]);
        inside[10] = 20;
        assert!(decode(&inside).is_none());
        // Too many pixels to decode.
        let huge = file(&info(100_000, 100_000, 24, 0, 0), &[], &[]);
        assert_eq!(size(&huge), Some((100_000, 100_000)));
        assert!(decode(&huge).is_none());
    }
}
