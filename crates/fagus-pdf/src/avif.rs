//! The pixels of AVIF images, as Chrome draws them (Chrome 157, measured): the AV1 pictures
//! decoded by rav1d, turned from YUV into RGB as libavif turns them, with the chroma of
//! subsampled pictures upsampled bilinearly (as libyuv does for Chrome), and their colors
//! converted to sRGB where code points name others. An ICC profile is embedded with the pixels
//! instead, as for other images.
//!
//! rav1d is built without its assembly: it decodes with the same integer arithmetic on every
//! platform, so the pixels are the same everywhere (AV1 decoding is exact by its specification).

use fagus_layout::avif::{self, Coded, Nclx};
use re_rav1d::dav1d;

/// An AVIF image decoded: 8-bit RGBA pixels, row by row, and the ICC profile their colors are
/// in.
pub(crate) struct Decoded {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
    pub(crate) icc: Option<Vec<u8>>,
}

/// Decodes an AVIF image to the pixels it shows, cropped by its clean aperture and stored as
/// they are (`irot` and `imir` turn them as an EXIF orientation does); `None` for data that is
/// not an AVIF image Chrome shows, and for images of more than [`fagus_layout::MAX_PIXELS`].
pub(crate) fn decode(data: &[u8]) -> Option<Decoded> {
    let avif = avif::parse(data)?;
    let size = (avif.width as usize, avif.height as usize);
    if u64::from(avif.width) * u64::from(avif.height) > fagus_layout::MAX_PIXELS {
        return None;
    }
    let mut decoder = Decoder::new()?;
    let color = decoder.coded(data, &avif.color)?;
    let alpha = match &avif.alpha {
        Some(alpha) => Some(decoder.coded(data, alpha)?),
        None => None,
    };
    if (color.width, color.height) != size
        || alpha
            .as_ref()
            .is_some_and(|alpha| (alpha.width, alpha.height) != size)
    {
        return None;
    }
    // The code points of the sequence header where the container has none.
    let sequence = |coded: &Coded| {
        let first = match coded {
            Coded::Picture(parts) => parts,
            Coded::Grid { tiles, .. } => tiles.first()?,
        };
        avif::sequence_colors(&avif::bytes(data, first)?)
    };
    let colors = match avif.nclx {
        Some(nclx) => nclx,
        None => sequence(&avif.color)?,
    };
    // An alpha of limited range is stretched to full range, as libavif does.
    let alpha = alpha.map(|planes| {
        let full_range = avif
            .alpha
            .as_ref()
            .and_then(sequence)
            .is_none_or(|colors| colors.full_range);
        (planes, full_range)
    });
    let icc = avif.icc.and_then(|range| data.get(range)).map(<[u8]>::to_vec);
    // The colors of an ICC profile are left to it.
    let conversion = if icc.is_some() {
        Conversion::None
    } else {
        Conversion::new(colors.primaries, colors.transfer)?
    };
    let (width, height) = (avif.crop.0 as usize, avif.crop.1 as usize);
    let pixels = rgba(
        &color,
        alpha.as_ref().map(|(planes, full_range)| (planes, *full_range)),
        colors,
        avif.premultiplied,
        &conversion,
        (width, height),
    )?;
    Some(Decoded {
        width: width as u32,
        height: height as u32,
        pixels,
        icc,
    })
}

/// The planes of a decoded picture, at its bit depth.
struct Planes {
    width: usize,
    height: usize,
    depth: u32,
    /// Whether the chroma planes have half the columns and half the rows; `None` for a
    /// monochrome picture, which has none.
    chroma: Option<(bool, bool)>,
    y: Vec<u16>,
    u: Vec<u16>,
    v: Vec<u16>,
}

impl Planes {
    fn chroma_size(&self) -> (usize, usize) {
        match self.chroma {
            Some((half_x, half_y)) => (
                (self.width + usize::from(half_x)) >> u8::from(half_x),
                (self.height + usize::from(half_y)) >> u8::from(half_y),
            ),
            None => (0, 0),
        }
    }
}

/// An AV1 decoder for the pictures of one image.
struct Decoder(dav1d::Decoder);

impl Decoder {
    fn new() -> Option<Self> {
        let mut settings = dav1d::Settings::new();
        // One thread, as the pictures are decoded side by side with other work.
        settings.set_n_threads(1);
        settings.set_max_frame_delay(1);
        // The highest spatial layer only, as libavif decodes.
        settings.set_all_layers(false);
        settings.set_frame_size_limit(u32::try_from(fagus_layout::MAX_PIXELS).unwrap_or(u32::MAX));
        dav1d::Decoder::with_settings(&settings).ok().map(Self)
    }

    /// A picture or the grid of pictures it makes.
    fn coded(&mut self, data: &[u8], coded: &Coded) -> Option<Planes> {
        match coded {
            Coded::Picture(parts) => self.picture(avif::bytes(data, parts)?),
            Coded::Grid {
                rows,
                columns,
                width,
                height,
                tiles,
            } => self.grid(
                data,
                (*rows as usize, *columns as usize),
                (*width as usize, *height as usize),
                tiles,
            ),
        }
    }

    /// The pictures of a grid in one, as libavif puts them together: of one size, depth and
    /// chroma, covering the image without a row or column of tiles more than it needs.
    fn grid(
        &mut self,
        data: &[u8],
        (rows, columns): (usize, usize),
        (width, height): (usize, usize),
        tiles: &[Vec<std::ops::Range<usize>>],
    ) -> Option<Planes> {
        if width == 0 || height == 0 || width as u64 * height as u64 > fagus_layout::MAX_PIXELS {
            return None;
        }
        let mut out: Option<Planes> = None;
        let mut first = (0, 0);
        for (index, parts) in tiles.iter().enumerate() {
            let tile = self.picture(avif::bytes(data, parts)?)?;
            if out.is_none() {
                first = (tile.width, tile.height);
                let fits = |count: usize, tile: usize, size: usize| {
                    count * tile >= size && (count - 1) * tile < size
                };
                let (half_x, half_y) = tile.chroma.unwrap_or_default();
                if !fits(columns, first.0, width)
                    || !fits(rows, first.1, height)
                    || (half_x && first.0 % 2 != 0)
                    || (half_y && first.1 % 2 != 0)
                {
                    return None;
                }
                let mut planes = Planes {
                    width,
                    height,
                    depth: tile.depth,
                    chroma: tile.chroma,
                    y: vec![0; width * height],
                    u: Vec::new(),
                    v: Vec::new(),
                };
                let (chroma_width, chroma_height) = planes.chroma_size();
                planes.u = vec![0; chroma_width * chroma_height];
                planes.v = vec![0; chroma_width * chroma_height];
                out = Some(planes);
            }
            let out = out.as_mut()?;
            if (tile.width, tile.height) != first || tile.depth != out.depth || tile.chroma != out.chroma {
                return None;
            }
            let (row, column) = (index / columns, index % columns);
            place(
                &tile.y,
                (tile.width, tile.height),
                &mut out.y,
                (out.width, out.height),
                (column * first.0, row * first.1),
            );
            if let Some((half_x, half_y)) = out.chroma {
                let (chroma_width, chroma_height) = out.chroma_size();
                let tile_chroma = tile.chroma_size();
                let at = (
                    (column * first.0) >> u8::from(half_x),
                    (row * first.1) >> u8::from(half_y),
                );
                place(
                    &tile.u,
                    tile_chroma,
                    &mut out.u,
                    (chroma_width, chroma_height),
                    at,
                );
                place(
                    &tile.v,
                    tile_chroma,
                    &mut out.v,
                    (chroma_width, chroma_height),
                    at,
                );
            }
        }
        out
    }

    /// Decodes one AV1 picture.
    fn picture(&mut self, av1: Vec<u8>) -> Option<Planes> {
        self.0.flush();
        match self.0.send_data(av1, None, None, None) {
            Ok(()) => {}
            Err(error) if error.is_again() => {}
            Err(_) => return None,
        }
        for _ in 0..16 {
            match self.0.get_picture() {
                Ok(picture) => return planes(&picture),
                Err(error) if error.is_again() => match self.0.send_pending_data() {
                    Ok(()) => {}
                    Err(error) if error.is_again() => {}
                    Err(_) => return None,
                },
                Err(_) => return None,
            }
        }
        None
    }
}

/// Copies a plane of `size` into a larger one of `into` at `at`, as far as it reaches.
fn place(plane: &[u16], size: (usize, usize), out: &mut [u16], into: (usize, usize), at: (usize, usize)) {
    if at.0 >= into.0 {
        return;
    }
    let columns = size.0.min(into.0 - at.0);
    for row in 0..size.1.min(into.1.saturating_sub(at.1)) {
        let from = row * size.0;
        let to = (at.1 + row) * into.0 + at.0;
        out[to..to + columns].copy_from_slice(&plane[from..from + columns]);
    }
}

/// The planes of a decoded picture.
fn planes(picture: &dav1d::Picture) -> Option<Planes> {
    use dav1d::{PixelLayout, PlanarImageComponent};
    let (width, height) = (picture.width() as usize, picture.height() as usize);
    let depth = picture.bit_depth() as u32;
    if !matches!(depth, 8 | 10 | 12) || width == 0 || height == 0 {
        return None;
    }
    let chroma = match picture.pixel_layout() {
        PixelLayout::I400 => None,
        PixelLayout::I420 => Some((true, true)),
        PixelLayout::I422 => Some((true, false)),
        PixelLayout::I444 => Some((false, false)),
    };
    let wide = depth > 8;
    let read = |component: PlanarImageComponent, (columns, rows): (usize, usize)| -> Option<Vec<u16>> {
        let plane = picture.plane(component);
        let stride = picture.stride(component) as usize;
        let bytes = if wide { 2 } else { 1 };
        if stride < columns * bytes || plane.len() < stride * (rows - 1) + columns * bytes {
            return None;
        }
        let mut values = Vec::with_capacity(columns * rows);
        for row in plane.chunks(stride).take(rows) {
            let row = &row[..columns * bytes];
            if wide {
                values.extend(
                    row.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| u16::from_ne_bytes(*pair)),
                );
            } else {
                values.extend(row.iter().map(|value| u16::from(*value)));
            }
        }
        Some(values)
    };
    let mut planes = Planes {
        width,
        height,
        depth,
        chroma,
        y: read(PlanarImageComponent::Y, (width, height))?,
        u: Vec::new(),
        v: Vec::new(),
    };
    if chroma.is_some() {
        let size = planes.chroma_size();
        planes.u = read(PlanarImageComponent::U, size)?;
        planes.v = read(PlanarImageComponent::V, size)?;
    }
    Some(planes)
}

/// How YUV becomes RGB (ITU-T H.273 and libavif).
#[derive(Clone, Copy)]
enum Matrix {
    /// G, B and R stored as Y, U and V.
    Identity,
    /// Luma from red and blue with these weights.
    Weights(f32, f32),
    YCgCo,
    /// YCgCo-R, lossless: the RGB of `bits` fewer bits than YUV (2 for -Re, 1 for -Ro).
    YCgCoR(u32),
}

impl Matrix {
    fn new(code: u16, primaries: u16) -> Option<Self> {
        Some(match code {
            0 => Self::Identity,
            1 => Self::Weights(0.2126, 0.0722),
            4 => Self::Weights(0.30, 0.11),
            7 => Self::Weights(0.212, 0.087),
            8 => Self::YCgCo,
            9 => Self::Weights(0.2627, 0.0593),
            // Derived from the primaries: the luminance of red and blue.
            12 => {
                let [_, luma, _] = to_xyz(primaries)?;
                Self::Weights(luma[0] as f32, luma[2] as f32)
            }
            16 => Self::YCgCoR(2),
            17 => Self::YCgCoR(1),
            // Constant luminance and ICtCp, which libavif does not convert.
            10 | 11 | 13 | 14 => return None,
            // BT.601, and where the matrix is unspecified or reserved.
            _ => Self::Weights(0.299, 0.114),
        })
    }
}

/// Turns the planes into 8-bit RGBA, `size` of them from the top left corner.
fn rgba(
    color: &Planes,
    alpha: Option<(&Planes, bool)>,
    colors: Nclx,
    premultiplied: bool,
    conversion: &Conversion,
    (width, height): (usize, usize),
) -> Option<Vec<u8>> {
    let matrix = if color.chroma.is_none() {
        Matrix::Weights(0.299, 0.114)
    } else {
        Matrix::new(colors.matrix, colors.primaries)?
    };
    let depth = color.depth;
    let most = ((1_u32 << depth) - 1) as f32;
    let scale = (1_u32 << (depth - 8)) as f32;
    // Where values start and how many there are: limited range leaves room around them.
    let (luma_bias, luma_range, chroma_range) = if colors.full_range {
        (0.0, most, most)
    } else {
        (16.0 * scale, 219.0 * scale, 224.0 * scale)
    };
    let chroma_bias = (1_u32 << (depth - 1)) as f32;
    let luma = |value: u16| ((f32::from(value) - luma_bias) / luma_range).clamp(0.0, 1.0);
    let chroma = |value: u16| ((f32::from(value) - chroma_bias) / chroma_range).clamp(-0.5, 0.5);
    let (chroma_width, chroma_height) = color.chroma_size();
    let (half_x, half_y) = color.chroma.unwrap_or_default();
    // The chroma at a pixel, from the nearest sample and its neighbour towards the pixel,
    // weighted 3 to 1 in each direction that is subsampled.
    let neighbours = |position: usize, half: bool, count: usize| -> (usize, usize, u32, u32) {
        if !half {
            return (position, position, 4, 0);
        }
        let nearest = (position >> 1).min(count - 1);
        let other = if position.is_multiple_of(2) {
            nearest.saturating_sub(1)
        } else {
            (nearest + 1).min(count - 1)
        };
        (nearest, other, 3, 1)
    };
    let alpha = alpha.map(|(alpha, full_range)| {
        let most = ((1_u32 << alpha.depth) - 1) as f32;
        let scale = (1_u32 << (alpha.depth - 8)) as f32;
        let (bias, range) = if full_range {
            (0.0, most)
        } else {
            (16.0 * scale, 219.0 * scale)
        };
        (alpha, bias, range)
    });
    let mut pixels = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let (near_y, far_y, weight_near_y, weight_far_y) = neighbours(y, half_y, chroma_height.max(1));
        for x in 0..width {
            let at = y * color.width + x;
            let value_y = color.y[at];
            let (u, v) = if color.chroma.is_some() {
                let (near_x, far_x, weight_near_x, weight_far_x) = neighbours(x, half_x, chroma_width.max(1));
                let sample = |plane: &[u16]| {
                    let value = |column: usize, row: usize| u32::from(plane[row * chroma_width + column]);
                    ((weight_near_x * weight_near_y * value(near_x, near_y)
                        + weight_far_x * weight_near_y * value(far_x, near_y)
                        + weight_near_x * weight_far_y * value(near_x, far_y)
                        + weight_far_x * weight_far_y * value(far_x, far_y)
                        + 8)
                        >> 4) as u16
                };
                (sample(&color.u), sample(&color.v))
            } else {
                (chroma_bias as u16, chroma_bias as u16)
            };
            let mut rgb = match matrix {
                Matrix::Identity => [luma(v), luma(value_y), luma(u)],
                Matrix::Weights(red, blue) => {
                    let (luma, cb, cr) = (luma(value_y), chroma(u), chroma(v));
                    let green = 1.0 - red - blue;
                    [
                        luma + 2.0 * (1.0 - red) * cr,
                        luma - (2.0 * (red * (1.0 - red) * cr + blue * (1.0 - blue) * cb)) / green,
                        luma + 2.0 * (1.0 - blue) * cb,
                    ]
                }
                Matrix::YCgCo => {
                    let (luma, cg, co) = (luma(value_y), chroma(u), chroma(v));
                    let t = luma - cg;
                    [t + co, luma + cg, t - co]
                }
                Matrix::YCgCoR(bits) => {
                    let most = (1_i32 << depth.checked_sub(bits)?) - 1;
                    let bias = 1_i32 << (depth - 1);
                    let (cg, co) = (i32::from(u) - bias, i32::from(v) - bias);
                    let t = i32::from(value_y) - (cg >> 1);
                    let green = (t + cg).clamp(0, most);
                    let blue = (t - (co >> 1)).clamp(0, most);
                    let red = (blue + co).clamp(0, most);
                    [red, green, blue].map(|value| value as f32 / most as f32)
                }
            };
            for value in &mut rgb {
                *value = value.clamp(0.0, 1.0);
            }
            let opacity = match alpha {
                Some((alpha, bias, range)) => ((f32::from(alpha.y[at]) - bias) / range).clamp(0.0, 1.0),
                None => 1.0,
            };
            if premultiplied {
                for value in &mut rgb {
                    *value = if opacity > 0.0 {
                        (*value / opacity).min(1.0)
                    } else {
                        0.0
                    };
                }
            }
            let [red, green, blue] = conversion.apply(rgb);
            pixels.extend([red, green, blue, (opacity * 255.0).round() as u8]);
        }
    }
    Some(pixels)
}

/// How decoded colors become the sRGB colors Chrome draws.
enum Conversion {
    /// They are sRGB (or in an ICC profile's space): each value rounded to 8 bits.
    None,
    /// Through linear light: each value linearized (`linear`, by 16-bit value), HDR light tone
    /// mapped in the primaries of BT.2020 (`hdr` converts to them), the primaries converted to
    /// those of sRGB (`matrix`), and the result encoded as sRGB (`encode`, by 16-bit linear
    /// value).
    Linear {
        linear: Vec<f32>,
        /// For HLG: the luminance of the source primaries, for the system gamma.
        hlg: Option<[f32; 3]>,
        hdr: Option<[[f32; 3]; 3]>,
        matrix: [[f32; 3]; 3],
        encode: Vec<u8>,
    },
}

/// SDR white in PQ and HLG, in nits (ITU-R BT.2408).
const SDR_WHITE: f64 = 203.0;

/// The peak Chrome tone maps HDR light from, in nits.
const HDR_PEAK: f64 = 1000.0;

/// How Chrome 157 maps HDR light to SDR (measured): linear light of SDR white (1) and above at
/// these points becomes these, linear in between; below SDR white, half of it, and above the
/// peak, 1.
const TONE: [(f32, f32); 13] = [
    (1.0, 0.5),
    (1.25, 0.5899),
    (1.5, 0.6546),
    (1.75, 0.7024),
    (2.0, 0.7427),
    (2.25, 0.7785),
    (2.5, 0.8102),
    (2.75, 0.8396),
    (3.0, 0.8632),
    (3.5, 0.9071),
    (4.0, 0.946),
    (4.5, 0.9753),
    ((HDR_PEAK / SDR_WHITE) as f32, 1.0),
];

impl Conversion {
    /// The conversion of colors of `primaries` and `transfer` (ITU-T H.273); `None` for the
    /// transfer functions Chrome does not show (logarithmic, IEC 61966-2-4 and BT.1361).
    fn new(primaries: u16, transfer: u16) -> Option<Self> {
        let srgb_primaries = !matches!(primaries, 4..=9 | 11 | 12 | 22);
        // Chrome draws the BT.709 family of transfer functions as sRGB.
        let srgb_transfer = !matches!(transfer, 4 | 5 | 7..=12 | 16..=18);
        if matches!(transfer, 9..=12) {
            return None;
        }
        if srgb_primaries && srgb_transfer {
            return Some(Self::None);
        }
        let curve = |value: f64| -> f64 {
            match transfer {
                4 => libm::pow(value, 2.2),
                5 => libm::pow(value, 2.8),
                7 if value < 4.0 * 0.0228 => value / 4.0,
                7 => libm::pow((value + 0.1115) / 1.1115, 1.0 / 0.45),
                8 => value,
                16 => pq(value) / SDR_WHITE,
                17 => 52.37 / 48.0 * libm::pow(value, 2.6),
                18 => hlg(value),
                _ => srgb_linear(value),
            }
        };
        let linear = (0..=u16::MAX)
            .map(|value| curve(f64::from(value) / 65535.0) as f32)
            .collect();
        let encode = (0..=u16::MAX)
            .map(|value| {
                let encoded = srgb_encode(f64::from(value) / 65535.0);
                (encoded * 255.0).round().clamp(0.0, 255.0) as u8
            })
            .collect();
        let source = if srgb_primaries { 1 } else { primaries };
        let hlg = (transfer == 18).then(|| {
            let [_, luma, _] = to_xyz(source).unwrap_or_default();
            luma.map(|value| value as f32)
        });
        // Chrome tone maps by the largest component in the primaries of BT.2020 (Chrome 157,
        // measured).
        let (hdr, matrix) = if matches!(transfer, 16 | 18) {
            (Some(between(source, 9)?), between(9, 1)?)
        } else {
            (None, between(source, 1)?)
        };
        Some(Self::Linear {
            linear,
            hlg,
            hdr,
            matrix,
            encode,
        })
    }

    fn apply(&self, rgb: [f32; 3]) -> [u8; 3] {
        match self {
            Self::None => rgb.map(|value| (value * 255.0).round() as u8),
            Self::Linear {
                linear,
                hlg,
                hdr,
                matrix,
                encode,
            } => {
                let mut light = rgb.map(|value| linear[(value * 65535.0).round() as usize]);
                // HLG is scene light: displayed on the peak with the system gamma of 1.2.
                if let Some(luma) = hlg {
                    let luminance = luma[0] * light[0] + luma[1] * light[1] + luma[2] * light[2];
                    let gain = (HDR_PEAK / SDR_WHITE) as f32 * libm::powf(luminance.max(0.0), 0.2);
                    light = light.map(|value| value * gain);
                }
                if let Some(rec2020) = hdr {
                    light = transform(rec2020, light);
                    let most = light[0].max(light[1]).max(light[2]);
                    if most > 0.0 {
                        let factor = tone(most) / most;
                        light = light.map(|value| value * factor);
                    }
                }
                transform(matrix, light)
                    .map(|value| encode[(value.clamp(0.0, 1.0) * 65535.0).round() as usize])
            }
        }
    }
}

fn transform(matrix: &[[f32; 3]; 3], [r, g, b]: [f32; 3]) -> [f32; 3] {
    matrix.map(|row| row[0] * r + row[1] * g + row[2] * b)
}

/// HDR light (1 for SDR white) as Chrome maps it to SDR (see [`TONE`]).
fn tone(light: f32) -> f32 {
    let (first, _) = TONE[0];
    if light <= first {
        return light * 0.5;
    }
    for pair in TONE.windows(2) {
        let [(x0, y0), (x1, y1)] = [pair[0], pair[1]];
        if light <= x1 {
            return y0 + (light - x0) / (x1 - x0) * (y1 - y0);
        }
    }
    1.0
}

/// The light of a PQ value, in nits (SMPTE ST 2084).
fn pq(value: f64) -> f64 {
    let (m1, m2) = (2610.0 / 16384.0, 2523.0 / 4096.0 * 128.0);
    let (c1, c2, c3) = (3424.0 / 4096.0, 2413.0 / 4096.0 * 32.0, 2392.0 / 4096.0 * 32.0);
    let power = libm::pow(value, 1.0 / m2);
    libm::pow((power - c1).max(0.0) / (c2 - c3 * power), 1.0 / m1) * 10000.0
}

/// The scene light of an HLG value, from 0 to 1 (ITU-R BT.2100).
fn hlg(value: f64) -> f64 {
    let a = 0.178_832_77;
    let b = 1.0 - 4.0 * a;
    let c = 0.5 - a * libm::log(4.0 * a);
    if value <= 0.5 {
        value * value / 3.0
    } else {
        (libm::exp((value - c) / a) + b) / 12.0
    }
}

fn srgb_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        libm::pow((value + 0.055) / 1.055, 2.4)
    }
}

fn srgb_encode(value: f64) -> f64 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * libm::pow(value, 1.0 / 2.4) - 0.055
    }
}

/// The chromaticities of red, green, blue and white of color primaries (ITU-T H.273).
fn chromaticities(primaries: u16) -> Option<[(f64, f64); 4]> {
    const D65: (f64, f64) = (0.3127, 0.3290);
    const C: (f64, f64) = (0.310, 0.316);
    Some(match primaries {
        1 => [(0.64, 0.33), (0.30, 0.60), (0.15, 0.06), D65],
        4 => [(0.67, 0.33), (0.21, 0.71), (0.14, 0.08), C],
        5 => [(0.64, 0.33), (0.29, 0.60), (0.15, 0.06), D65],
        6 | 7 => [(0.630, 0.340), (0.310, 0.595), (0.155, 0.070), D65],
        8 => [(0.681, 0.319), (0.243, 0.692), (0.145, 0.049), C],
        9 => [(0.708, 0.292), (0.170, 0.797), (0.131, 0.046), D65],
        11 => [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060), (0.314, 0.351)],
        12 => [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060), D65],
        22 => [(0.630, 0.340), (0.295, 0.605), (0.155, 0.077), D65],
        _ => return None,
    })
}

/// The matrix from linear RGB of `primaries` to CIE XYZ (white of luminance 1).
fn to_xyz(primaries: u16) -> Option<[[f64; 3]; 3]> {
    let [red, green, blue, white] = chromaticities(primaries)?;
    let xyz = |(x, y): (f64, f64)| [x / y, 1.0, (1.0 - x - y) / y];
    let columns = [xyz(red), xyz(green), xyz(blue)];
    let base = [
        [columns[0][0], columns[1][0], columns[2][0]],
        [columns[0][1], columns[1][1], columns[2][1]],
        [columns[0][2], columns[1][2], columns[2][2]],
    ];
    // Each primary scaled so that together they make the white.
    let scale = multiply_vector(&invert(&base)?, xyz(white));
    Some(base.map(|row| [row[0] * scale[0], row[1] * scale[1], row[2] * scale[2]]))
}

/// The matrix from linear RGB of primaries `from` to those of `to`, adapted to the white of
/// `to` with the Bradford transform where the whites differ, as ICC color management does.
fn between(from: u16, to: u16) -> Option<[[f32; 3]; 3]> {
    let [.., white] = chromaticities(from)?;
    let [.., target_white] = chromaticities(to)?;
    let from = to_xyz(from)?;
    let to = invert(&to_xyz(to)?)?;
    let bradford = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let xyz = |(x, y): (f64, f64)| [x / y, 1.0, (1.0 - x - y) / y];
    let (source, target) = (
        multiply_vector(&bradford, xyz(white)),
        multiply_vector(&bradford, xyz(target_white)),
    );
    let gains = [
        [target[0] / source[0], 0.0, 0.0],
        [0.0, target[1] / source[1], 0.0],
        [0.0, 0.0, target[2] / source[2]],
    ];
    let adapt = multiply(&invert(&bradford)?, &multiply(&gains, &bradford));
    let matrix = multiply(&to, &multiply(&adapt, &from));
    Some(matrix.map(|row| row.map(|value| value as f32)))
}

fn multiply(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for (row, out) in out.iter_mut().enumerate() {
        for (column, value) in out.iter_mut().enumerate() {
            *value = (0..3).map(|k| a[row][k] * b[k][column]).sum();
        }
    }
    out
}

fn multiply_vector(a: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    a.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

fn invert(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let cofactor = |r0: usize, r1: usize, c0: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    let adjugate = [
        [cofactor(1, 2, 1, 2), -cofactor(0, 2, 1, 2), cofactor(0, 1, 1, 2)],
        [-cofactor(1, 2, 0, 2), cofactor(0, 2, 0, 2), -cofactor(0, 1, 0, 2)],
        [cofactor(1, 2, 0, 1), -cofactor(0, 2, 0, 1), cofactor(0, 1, 0, 1)],
    ];
    let determinant = m[0][0] * adjugate[0][0] + m[0][1] * adjugate[1][0] + m[0][2] * adjugate[2][0];
    (determinant.abs() > 1e-12).then(|| adjugate.map(|row| row.map(|value| value / determinant)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!(
            "{}/../../tests/corpus/images/media/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(&path).unwrap_or_else(|_| panic!("{path}"))
    }

    fn decoded(name: &str) -> Decoded {
        decode(&fixture(name)).unwrap_or_else(|| panic!("{name} decodes"))
    }

    fn pixel(image: &Decoded, x: usize, y: usize) -> [u8; 4] {
        let at = (y * image.width as usize + x) * 4;
        image.pixels[at..at + 4].try_into().expect("a pixel")
    }

    fn close(found: [u8; 4], expected: [u8; 4], tolerance: u8) -> bool {
        found
            .iter()
            .zip(expected)
            .all(|(found, expected)| found.abs_diff(expected) <= tolerance)
    }

    /// The pixels of the ramp the lossless fixtures store: 16 grays, then two colors.
    fn ramp(x: usize, y: usize) -> [u8; 4] {
        match y {
            0..8 => {
                let gray = (x / 16 * 17) as u8;
                [gray, gray, gray, 255]
            }
            8..16 => [200, 100, 50, 255],
            _ => [50, 150, 220, 255],
        }
    }

    #[test]
    fn decodes_lossless_pictures_exactly() {
        let image = decoded("avif/lossless.avif");
        assert_eq!((image.width, image.height, image.icc.is_none()), (256, 24, true));
        for y in 0..24 {
            for x in 0..256 {
                assert_eq!(pixel(&image, x, y), ramp(x, y), "{x} {y}");
            }
        }
        // An ICC profile is embedded with the pixels, which stay as they are.
        let image = decoded("avif/icc.avif");
        assert_eq!(image.icc.as_deref().map(<[u8]>::len), Some(736));
        assert_eq!(pixel(&image, 136, 2), ramp(136, 2));
        assert_eq!(pixel(&image, 0, 20), ramp(0, 20));
    }

    /// The quadrants of the fixtures of 24 × 16 pixels: red, green, blue and white (or yellow),
    /// with a black corner of 4 × 4 at the top left.
    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    #[test]
    fn decodes_subsampled_and_deep_pictures() {
        // 4:2:0, 8 bits; 4:2:2, 12 bits of limited range, BT.709.
        for name in ["avif/opaque.avif", "avif/twelve-bit.avif"] {
            let image = decoded(name);
            assert_eq!((image.width, image.height), (24, 16), "{name}");
            for (x, y, expected) in [
                (1, 1, BLACK),
                (8, 6, RED),
                (18, 3, GREEN),
                (5, 12, BLUE),
                (18, 12, WHITE),
            ] {
                let found = pixel(&image, x, y);
                assert!(close(found, expected, 12), "{name} {x} {y}: {found:?}");
            }
        }
        // Monochrome: gray.
        let image = decoded("avif/gray.avif");
        for x in (0..256).step_by(16) {
            let [r, g, b, a] = pixel(&image, x + 8, 3);
            assert!(r == g && g == b && a == 255);
            assert!(r.abs_diff(ramp(x, 0)[0]) <= 4, "{x}: {r}");
        }
    }

    #[test]
    fn decodes_alpha_straight_or_premultiplied() {
        // Quadrants of red, green, blue and yellow, fading from transparent on the left to
        // opaque on the right.
        for name in ["avif/alpha.avif", "avif/premultiplied.avif", "avif/ten-bit.avif"] {
            let image = decoded(name);
            let found = pixel(&image, 22, 12);
            assert!(close(found, [240, 192, 32, 245], 12), "{name}: {found:?}");
            let found = pixel(&image, 16, 3);
            assert!(close(found, [48, 160, 80, 170], 16), "{name}: {found:?}");
            let [.., a] = pixel(&image, 0, 8);
            assert!(a < 16, "{name}: {a}");
        }
    }

    #[test]
    fn decodes_grids_crops_and_first_frames() {
        let image = decoded("avif/grid.avif");
        assert_eq!((image.width, image.height), (128, 128));
        for (x, y, expected) in [
            (10, 10, RED),
            (100, 30, GREEN),
            (30, 100, BLUE),
            (100, 100, [255, 255, 0, 255]),
        ] {
            assert!(close(pixel(&image, x, y), expected, 8), "{x} {y}");
        }
        // The clean aperture: 16 × 10 from the top left.
        let image = decoded("avif/cropped.avif");
        assert_eq!((image.width, image.height), (16, 10));
        assert_eq!(pixel(&image, 15, 0), GREEN);
        assert_eq!(pixel(&image, 0, 9), BLUE);
        // Stored as they are: `irot` and `imir` turn them as an orientation.
        let image = decoded("avif/turned.avif");
        assert_eq!(
            (image.width, image.height, pixel(&image, 23, 15)),
            (24, 16, WHITE)
        );
        // The first frame of an animation, red; the second is blue.
        let image = decoded("avif/animated.avif");
        assert_eq!(pixel(&image, 5, 5), RED);
    }

    #[test]
    fn converts_colors_as_chrome_draws_them() {
        // Chrome 157 (measured), with the ramp's gray of 136 and its two colors.
        let srgb = |value: u8| f32::from(value) / 255.0;
        let mut conversions = Vec::new();
        let mut convert = |primaries, transfer, [r, g, b]: [u8; 3]| {
            let index = conversions
                .iter()
                .position(|(key, _)| *key == (primaries, transfer))
                .unwrap_or_else(|| {
                    let conversion = Conversion::new(primaries, transfer).expect("supported");
                    conversions.push(((primaries, transfer), conversion));
                    conversions.len() - 1
                });
            conversions[index].1.apply([srgb(r), srgb(g), srgb(b)])
        };
        let measured: [(u16, u16, [u8; 3], [u8; 3]); 16] = [
            (4, 13, [232, 93, 30], [0, 158, 225]),
            (5, 13, [203, 100, 51], [38, 150, 219]),
            (6, 13, [196, 102, 49], [65, 150, 220]),
            (8, 13, [222, 92, 32], [0, 151, 225]),
            (9, 13, [241, 76, 29], [0, 157, 227]),
            (11, 13, [211, 93, 32], [0, 152, 226]),
            (12, 13, [215, 93, 31], [0, 153, 226]),
            (22, 13, [202, 102, 49], [44, 152, 220]),
            (1, 1, [200, 100, 50], [50, 150, 220]),
            (1, 2, [200, 100, 50], [50, 150, 220]),
            (1, 14, [200, 100, 50], [50, 150, 220]),
            (9, 14, [241, 76, 29], [0, 157, 227]),
            (1, 5, [189, 76, 26], [26, 131, 212]),
            (1, 7, [206, 115, 68], [68, 161, 224]),
            (1, 16, [255, 51, 8], [3, 83, 255]),
            (9, 18, [230, 48, 17], [0, 118, 221]),
        ];
        for (primaries, transfer, first, second) in measured {
            for (color, expected) in [([200, 100, 50], first), ([50, 150, 220], second)] {
                let found = convert(primaries, transfer, color);
                assert!(
                    found
                        .iter()
                        .zip(expected)
                        .all(|(found, expected)| found.abs_diff(expected) <= 2),
                    "{primaries}/{transfer}: {found:?}, Chrome {expected:?}"
                );
            }
        }
        // The gray ramp through transfer functions, HDR ones tone mapped.
        let ramps: [(u16, [u8; 16]); 5] = [
            (
                5,
                [
                    0, 2, 12, 27, 44, 61, 78, 97, 115, 134, 154, 173, 193, 214, 234, 255,
                ],
            ),
            (
                7,
                [
                    0, 35, 52, 69, 85, 101, 117, 133, 148, 164, 179, 195, 210, 225, 240, 255,
                ],
            ),
            (
                17,
                [
                    0, 3, 17, 35, 53, 71, 89, 108, 127, 146, 166, 185, 205, 225, 245, 255,
                ],
            ),
            (
                16,
                [
                    0, 1, 6, 18, 34, 54, 80, 112, 153, 200, 229, 250, 255, 255, 255, 255,
                ],
            ),
            (
                18,
                [
                    0, 3, 16, 31, 46, 61, 76, 91, 107, 126, 150, 179, 207, 225, 241, 255,
                ],
            ),
        ];
        for (transfer, expected) in ramps {
            for (step, expected) in expected.into_iter().enumerate() {
                let gray = (step * 17) as u8;
                let [found, ..] = convert(1, transfer, [gray, gray, gray]);
                assert!(
                    found.abs_diff(expected) <= 2,
                    "{transfer} {gray}: {found}, Chrome {expected}"
                );
            }
        }
        // Gamma 2.2 and linear light.
        assert_eq!(convert(1, 4, [136, 204, 0]), [137, 205, 0]);
        assert_eq!(convert(1, 8, [136, 204, 0]), [193, 231, 0]);
        // Logarithmic transfer, IEC 61966-2-4 and BT.1361 are not shown.
        for transfer in 9..=12 {
            assert!(Conversion::new(1, transfer).is_none());
        }
    }

    #[test]
    fn converts_fixtures_with_code_points() {
        let image = decoded("avif/p3.avif");
        assert!(close(pixel(&image, 8, 12), [215, 93, 31, 255], 2));
        let image = decoded("avif/pq.avif");
        assert!(close(pixel(&image, 136, 2), [153, 153, 153, 255], 2));
        let image = decoded("avif/hlg.avif");
        assert!(close(pixel(&image, 136, 2), [107, 107, 107, 255], 2));
    }

    #[test]
    fn decodes_to_the_same_pixels_everywhere() {
        // FNV-1a of the pixels: rav1d without assembly and the conversions decode the same
        // bits on every platform, which CI checks on x86-64 and ARM64.
        let hash = |name: &str| {
            decoded(name)
                .pixels
                .iter()
                .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                    (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
                })
        };
        let hashes = ["photo.avif", "stamp-turned.avif", "avif/p3.avif", "avif/hlg.avif"].map(hash);
        assert_eq!(
            hashes,
            [
                3_895_048_584_640_867_519,
                6_560_016_209_010_261_228,
                9_981_793_842_451_365_669,
                2_715_252_348_005_858_597
            ]
        );
    }

    #[test]
    fn refuses_broken_data() {
        let data = fixture("avif/opaque.avif");
        // The AV1 data cut short, and changed.
        assert!(decode(&data[..data.len() - 40]).is_none());
        let mut changed = data.clone();
        let end = changed.len();
        for byte in &mut changed[end - 60..end - 20] {
            *byte ^= 0x5A;
        }
        let _ = decode(&changed);
    }
}
