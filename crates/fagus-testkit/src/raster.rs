//! Pages of a PDF as pixels, drawn by Poppler's `pdftoppm`, and how much two of them differ.
//!
//! Both sides of every comparison (a test and its reference, or Chrome's PDF and Fagus') go
//! through the same rasterizer, so what is compared is the PDF, not the rasterizer.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// An RGB image, 8 bits per channel, row by row.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Image({}x{})", self.width, self.height)
    }
}

impl Image {
    /// Writes the image as a PNG file.
    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        let mut data = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut data, self.width as u32, self.height as u32);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
            writer
                .write_image_data(&self.pixels)
                .map_err(|error| error.to_string())?;
            writer.finish().map_err(|error| error.to_string())?;
        }
        std::fs::write(path, data).map_err(|error| error.to_string())
    }

    /// The value of a channel, white outside the image.
    fn channel(&self, x: usize, y: usize, channel: usize) -> u8 {
        if x < self.width && y < self.height {
            self.pixels[(y * self.width + x) * 3 + channel]
        } else {
            255
        }
    }
}

/// Reads a binary PPM (`P6`, as `pdftoppm` writes it) with a maximum value of 255.
pub fn read_ppm(data: &[u8]) -> Result<Image, String> {
    let mut position = 0;
    let mut fields = [0_usize; 3];
    if data.get(..2) != Some(b"P6") {
        return Err("not a binary PPM".to_owned());
    }
    position += 2;
    for field in &mut fields {
        // Whitespace and comments up to the number.
        loop {
            match data.get(position) {
                Some(byte) if byte.is_ascii_whitespace() => position += 1,
                Some(b'#') => {
                    while data.get(position).is_some_and(|&byte| byte != b'\n') {
                        position += 1;
                    }
                }
                _ => break,
            }
        }
        let start = position;
        while data.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        *field = std::str::from_utf8(&data[start..position])
            .ok()
            .and_then(|digits| digits.parse().ok())
            .ok_or("a broken PPM header")?;
    }
    let [width, height, maximum] = fields;
    if maximum != 255 {
        return Err(format!("a PPM with maximum value {maximum}"));
    }
    // A single whitespace character ends the header.
    position += 1;
    let size = width * height * 3;
    let pixels = data
        .get(position..position + size)
        .ok_or("a truncated PPM")?
        .to_vec();
    Ok(Image {
        width,
        height,
        pixels,
    })
}

/// Runs `pdftoppm` (or the program `PDFTOPPM` names) on PDFs, in a directory for its files.
pub struct Rasterizer {
    program: OsString,
    scratch: PathBuf,
    counter: AtomicU64,
}

impl Rasterizer {
    /// How long one PDF may take.
    const LIMIT: Duration = Duration::from_secs(120);

    /// A rasterizer that keeps its files in `scratch` while it works.
    pub fn new(scratch: impl Into<PathBuf>) -> std::io::Result<Self> {
        let scratch = scratch.into();
        std::fs::create_dir_all(&scratch)?;
        Ok(Self {
            program: std::env::var_os("PDFTOPPM").unwrap_or_else(|| "pdftoppm".into()),
            scratch,
            counter: AtomicU64::new(0),
        })
    }

    /// The version line of the rasterizer, or why it cannot be run.
    pub fn version(&self) -> Result<String, String> {
        let output = Command::new(&self.program)
            .arg("-v")
            .output()
            .map_err(|error| format!("cannot run {}: {error}", self.program.to_string_lossy()))?;
        // Poppler prints its version to the standard error.
        let text =
            String::from_utf8_lossy(&output.stderr).into_owned() + &String::from_utf8_lossy(&output.stdout);
        Ok(text.lines().next().unwrap_or_default().trim().to_owned())
    }

    /// The pages `1..=last` of a PDF (fewer if it has fewer) at `dpi` dots per inch.
    pub fn rasterize(&self, pdf: &[u8], dpi: u32, last: usize) -> Result<Vec<Image>, String> {
        let name = format!("r{}", self.counter.fetch_add(1, Ordering::Relaxed));
        let input = self.scratch.join(format!("{name}.pdf"));
        std::fs::write(&input, pdf).map_err(|error| error.to_string())?;
        let prefix = self.scratch.join(&name);
        let status = crate::process::run(
            Command::new(&self.program)
                .arg("-r")
                .arg(dpi.to_string())
                .arg("-l")
                .arg(last.max(1).to_string())
                .arg(&input)
                .arg(&prefix),
            Self::LIMIT,
        );
        let pages = self.collect(&name);
        let _ = std::fs::remove_file(&input);
        match status {
            Ok(Some(status)) if status.success() => pages,
            Ok(Some(status)) => Err(format!("pdftoppm failed ({status})")),
            Ok(None) => Err("pdftoppm took too long".to_owned()),
            Err(error) => Err(format!("cannot run {}: {error}", self.program.to_string_lossy())),
        }
    }

    /// Reads and removes the page images `pdftoppm` wrote for `name`: `name-1.ppm`, or with
    /// the numbers padded to the same width (`name-01.ppm`) when there are more pages.
    fn collect(&self, name: &str) -> Result<Vec<Image>, String> {
        let mut files: Vec<(usize, PathBuf)> = std::fs::read_dir(&self.scratch)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let file = entry.file_name().into_string().ok()?;
                let number = file.strip_prefix(name)?.strip_prefix('-')?.strip_suffix(".ppm")?;
                Some((number.parse().ok()?, entry.path()))
            })
            .collect();
        files.sort();
        let mut pages = Vec::with_capacity(files.len());
        let mut failure = None;
        for (_, path) in &files {
            match std::fs::read(path).map_err(|error| error.to_string()) {
                Ok(data) => match read_ppm(&data) {
                    Ok(image) => pages.push(image),
                    Err(error) => failure = Some(error),
                },
                Err(error) => failure = Some(error),
            }
            let _ = std::fs::remove_file(path);
        }
        failure.map_or(Ok(pages), Err)
    }

    /// The directory the rasterizer works in.
    pub fn scratch(&self) -> &Path {
        &self.scratch
    }
}

/// How two images differ, as web-platform-tests measures it for fuzzy reftests: the largest
/// difference of a color channel, and the number of pixels that differ at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Difference {
    pub max_channel: u8,
    pub pixels: usize,
}

/// How `a` and `b` differ; where one is larger, the other counts as white there.
pub fn difference(a: &Image, b: &Image) -> Difference {
    if a.width == b.width && a.height == b.height && a.pixels == b.pixels {
        return Difference::default();
    }
    let mut result = Difference::default();
    if a.width == b.width && a.height == b.height {
        for (x, y) in a
            .pixels
            .as_chunks::<3>()
            .0
            .iter()
            .zip(b.pixels.as_chunks::<3>().0)
        {
            let largest = (0..3)
                .map(|channel| x[channel].abs_diff(y[channel]))
                .max()
                .unwrap_or(0);
            if largest > 0 {
                result.pixels += 1;
                result.max_channel = result.max_channel.max(largest);
            }
        }
        return result;
    }
    for y in 0..a.height.max(b.height) {
        for x in 0..a.width.max(b.width) {
            let mut largest = 0;
            for channel in 0..3 {
                largest = largest.max(a.channel(x, y, channel).abs_diff(b.channel(x, y, channel)));
            }
            if largest > 0 {
                result.pixels += 1;
                result.max_channel = result.max_channel.max(largest);
            }
        }
    }
    result
}

/// The share of pixels (0 to 1) that differ between `a` and `b` beyond what antialiasing and
/// positions one pixel apart explain: a pixel counts if a channel of it lies more than
/// `threshold` outside the range of that channel in the 3×3 pixels around it in the other image,
/// in either direction.
pub fn tolerant_share(a: &Image, b: &Image, threshold: u8) -> f64 {
    let differ = tolerant_flags(a, b, threshold);
    if differ.is_empty() {
        return 0.0;
    }
    differ.iter().filter(|&&flag| flag).count() as f64 / differ.len() as f64
}

/// [`difference`] with the tolerance of [`tolerant_share`]: only pixels that lie outside the
/// range of the pixels around them in the other image count, and the largest difference is
/// that of those pixels.
pub fn tolerant_difference(a: &Image, b: &Image, threshold: u8) -> Difference {
    if a.width == b.width && a.height == b.height && a.pixels == b.pixels {
        return Difference::default();
    }
    let width = a.width.max(b.width);
    let mut result = Difference::default();
    for (index, flag) in tolerant_flags(a, b, threshold).into_iter().enumerate() {
        if flag {
            let (x, y) = (index % width, index / width);
            let largest = (0..3)
                .map(|channel| a.channel(x, y, channel).abs_diff(b.channel(x, y, channel)))
                .max()
                .unwrap_or(0);
            result.pixels += 1;
            result.max_channel = result.max_channel.max(largest);
        }
    }
    result
}

/// For each pixel of the larger extent of `a` and `b`, whether it differs beyond the tolerance.
fn tolerant_flags(a: &Image, b: &Image, threshold: u8) -> Vec<bool> {
    let width = a.width.max(b.width);
    let height = a.height.max(b.height);
    let mut differ = vec![false; width * height];
    if differ.is_empty() {
        return differ;
    }
    for channel in 0..3 {
        let plane_a = plane(a, width, height, channel);
        let plane_b = plane(b, width, height, channel);
        for (one, other) in [(&plane_a, &plane_b), (&plane_b, &plane_a)] {
            let (low, high) = neighborhood(other, width, height);
            for (index, flag) in differ.iter_mut().enumerate() {
                let value = i16::from(one[index]);
                *flag |= value < i16::from(low[index]) - i16::from(threshold)
                    || value > i16::from(high[index]) + i16::from(threshold);
            }
        }
    }
    differ
}

/// One channel of an image, `width` × `height`, white outside it.
fn plane(image: &Image, width: usize, height: usize, channel: usize) -> Vec<u8> {
    let mut plane = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            plane.push(image.channel(x, y, channel));
        }
    }
    plane
}

/// The minimum and the maximum of the 3×3 pixels around each pixel (edges repeated).
fn neighborhood(plane: &[u8], width: usize, height: usize) -> (Vec<u8>, Vec<u8>) {
    let mut low = vec![0; plane.len()];
    let mut high = vec![0; plane.len()];
    // Across, then down.
    for y in 0..height {
        let row = &plane[y * width..(y + 1) * width];
        for x in 0..width {
            let window = &row[x.saturating_sub(1)..(x + 2).min(width)];
            low[y * width + x] = *window.iter().min().expect("not empty");
            high[y * width + x] = *window.iter().max().expect("not empty");
        }
    }
    let (across_low, across_high) = (low.clone(), high.clone());
    for y in 0..height {
        for x in 0..width {
            let rows = y.saturating_sub(1)..(y + 2).min(height);
            low[y * width + x] = rows
                .clone()
                .map(|row| across_low[row * width + x])
                .min()
                .expect("not empty");
            high[y * width + x] = rows
                .map(|row| across_high[row * width + x])
                .max()
                .expect("not empty");
        }
    }
    (low, high)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: usize, height: usize, fill: [u8; 3]) -> Image {
        Image {
            width,
            height,
            pixels: fill.repeat(width * height),
        }
    }

    fn set(image: &mut Image, x: usize, y: usize, color: [u8; 3]) {
        let at = (y * image.width + x) * 3;
        image.pixels[at..at + 3].copy_from_slice(&color);
    }

    #[test]
    fn reads_a_ppm_with_a_comment() {
        let mut data = b"P6\n# pdftoppm\n2 1\n255\n".to_vec();
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let image = read_ppm(&data).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.pixels, [1, 2, 3, 4, 5, 6]);
        assert!(read_ppm(b"P6\n2 1\n255\n\x01").is_err());
        assert!(read_ppm(b"P3\n1 1\n255\n0 0 0").is_err());
    }

    #[test]
    fn counts_differing_pixels_and_the_largest_difference() {
        let a = image(4, 4, [255, 255, 255]);
        let mut b = a.clone();
        assert_eq!(difference(&a, &b), Difference::default());
        set(&mut b, 1, 1, [250, 255, 255]);
        set(&mut b, 2, 3, [255, 200, 255]);
        assert_eq!(
            difference(&a, &b),
            Difference {
                max_channel: 55,
                pixels: 2
            }
        );
        // A larger image counts as white beyond the other.
        let mut wide = image(5, 4, [255, 255, 255]);
        set(&mut wide, 4, 0, [0, 0, 0]);
        assert_eq!(difference(&a, &wide).pixels, 1);
    }

    #[test]
    fn tolerates_edges_one_pixel_apart_but_not_missing_ink() {
        let white = image(10, 10, [255, 255, 255]);
        let mut a = white.clone();
        let mut b = white.clone();
        for y in 2..8 {
            set(&mut a, 4, y, [0, 0, 0]);
            set(&mut b, 5, y, [0, 0, 0]);
        }
        assert!(tolerant_share(&a, &b, 48).abs() < f64::EPSILON);
        assert!(tolerant_share(&a, &white, 48) > 0.05);
        assert_eq!(tolerant_difference(&a, &b, 0), Difference::default());
        // A red pixel where the other image has green and white around it counts.
        let mut green = white.clone();
        for x in 0..10 {
            set(&mut green, x, 5, [0, 128, 0]);
        }
        let mut seam = green.clone();
        set(&mut seam, 3, 6, [255, 0, 0]);
        assert_eq!(
            tolerant_difference(&green, &seam, 0),
            Difference {
                max_channel: 255,
                pixels: 1
            }
        );
    }
}
