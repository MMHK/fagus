//! AVIF images: AV1 pictures in a HEIF container (ISO/IEC 23008-12, the AV1 Image File Format
//! and MIAF). This reads the container; fagus-pdf decodes the AV1 pictures. What Chrome does where
//! the format leaves room (Chrome 157, measured):
//!
//! - The primary item is shown: one AV1 picture or a grid of them. An image sequence (`avis`)
//!   shows its primary item, which encoders make its first frame, or, without one, the first
//!   frame of its first track.
//! - An auxiliary alpha item (`auxl`, `auxC`) is the alpha channel, premultiplied where the
//!   primary item refers to it with `prem`.
//! - `irot` and `imir` turn the image whatever `image-orientation` says: they are part of the
//!   image, not metadata ([`Avif::orientation`]).
//! - A clean aperture (`clap`) crops the image only where it starts at the top left corner;
//!   others are ignored.
//! - The colors are those of an ICC profile (`colr` of type `prof` or `rICC`), or else of the
//!   code points (`nclx`) of the `colr` box or, without one, of the AV1 sequence header.

use std::ops::Range;

/// What the container of an AVIF image says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Avif {
    /// The size of the decoded picture (or grid), before cropping and turning.
    pub width: u32,
    pub height: u32,
    /// The size of the part shown, from the top left corner: smaller than the picture where a
    /// clean aperture crops it.
    pub crop: (u32, u32),
    /// How the picture is turned to be seen, as an EXIF orientation (1 to 8).
    pub orientation: u8,
    pub color: Coded,
    pub alpha: Option<Coded>,
    /// Whether the colors are premultiplied by the alpha.
    pub premultiplied: bool,
    /// Where the ICC profile is in the file.
    pub icc: Option<Range<usize>>,
    pub nclx: Option<Nclx>,
}

/// An AV1 picture, or a grid of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Coded {
    /// Where the AV1 data of the picture is in the file, in parts.
    Picture(Vec<Range<usize>>),
    /// Pictures of one size in `rows` and `columns`, row by row, whose top left `width` by
    /// `height` pixels are the image.
    Grid {
        rows: u32,
        columns: u32,
        width: u32,
        height: u32,
        tiles: Vec<Vec<Range<usize>>>,
    },
}

/// The color code points of a `colr` box (ITU-T H.273).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nclx {
    pub primaries: u16,
    pub transfer: u16,
    pub matrix: u16,
    pub full_range: bool,
}

/// The types of auxiliary items that are an alpha channel.
const ALPHA: [&[u8]; 2] = [
    b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha",
    b"urn:mpeg:hevc:2015:auxid:1",
];

/// The most items, references and properties read: far more than an image has.
const MAX_ENTRIES: usize = 4096;

/// Whether data is an AVIF file: a `ftyp` box first that names the brand `avif` or `avis`.
pub fn is_avif(data: &[u8]) -> bool {
    let Some((kind, content)) = boxes(data, 0..data.len()).next() else {
        return false;
    };
    if &kind != b"ftyp" {
        return false;
    }
    let brands = &data[content];
    // The major brand, a minor version, then the compatible brands.
    brands
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .any(|(index, brand)| index != 1 && (brand == b"avif" || brand == b"avis"))
}

/// The size of an AVIF image as it is seen, before turning: `None` for data that is not one.
pub fn size(data: &[u8]) -> Option<(u32, u32)> {
    parse(data).map(|avif| avif.crop)
}

/// Reads the container of an AVIF image; `None` for data that is not one Chrome shows.
pub fn parse(data: &[u8]) -> Option<Avif> {
    if !is_avif(data) {
        return None;
    }
    let whole = 0..data.len();
    let meta = boxes(data, whole.clone()).find(|(kind, _)| kind == b"meta");
    if let Some(avif) = meta.and_then(|(_, meta)| Meta::read(data, meta)?.avif(data)) {
        return Some(avif);
    }
    let (_, moov) = boxes(data, whole).find(|(kind, _)| kind == b"moov")?;
    sequence(data, moov)
}

/// The data of a picture in parts, as one; `None` where it is larger than the file.
pub fn bytes(data: &[u8], parts: &[Range<usize>]) -> Option<Vec<u8>> {
    let total = parts.iter().map(ExactSizeIterator::len).sum::<usize>();
    if total > data.len() {
        return None;
    }
    let mut bytes = Vec::with_capacity(total);
    for part in parts {
        bytes.extend_from_slice(data.get(part.clone())?);
    }
    Some(bytes)
}

/// The boxes within `range` of `data`: their types and where their contents are. A box that
/// runs past the end of the range ends the boxes.
fn boxes(data: &[u8], range: Range<usize>) -> impl Iterator<Item = ([u8; 4], Range<usize>)> + '_ {
    let mut at = range.start;
    let end = range.end.min(data.len());
    std::iter::from_fn(move || {
        let mut reader = Reader { data, at, end };
        let size = u64::from(reader.u32()?);
        let kind = reader.fourcc()?;
        let size = match size {
            0 => (end - at) as u64,
            1 => reader.u64()?,
            size => size,
        };
        let header = (reader.at - at) as u64;
        if size < header || size > (end - at) as u64 {
            at = end;
            return None;
        }
        let content = reader.at..at + size as usize;
        at = content.end;
        Some((kind, content))
    })
}

/// Reads big-endian numbers from data up to `end`.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    end: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8], range: Range<usize>) -> Self {
        Self {
            data,
            at: range.start,
            end: range.end.min(data.len()),
        }
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count).filter(|end| *end <= self.end)?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Some(bytes)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }

    fn fourcc(&mut self) -> Option<[u8; 4]> {
        self.take(4)?.try_into().ok()
    }

    /// A number of `size` bytes: 0, 4 or 8.
    fn sized(&mut self, size: u8) -> Option<u64> {
        match size {
            0 => Some(0),
            4 => self.u32().map(u64::from),
            8 => self.u64(),
            _ => None,
        }
    }

    /// The version and flags of a full box.
    fn full(&mut self) -> Option<(u8, u32)> {
        let word = self.u32()?;
        Some(((word >> 24) as u8, word & 0x00FF_FFFF))
    }

    /// An item ID, of 16 bits or (`wide`) 32.
    fn id(&mut self, wide: bool) -> Option<u32> {
        if wide {
            self.u32()
        } else {
            self.u16().map(u32::from)
        }
    }

    fn rest(&self) -> Range<usize> {
        self.at..self.end
    }
}

/// Where an item's data is: in the file or in the `idat` box, in parts of offset and length (0
/// for the rest).
#[derive(Debug)]
struct Location {
    in_idat: bool,
    extents: Vec<(u64, u64)>,
}

/// The `meta` box of an AVIF file, as far as it matters.
#[derive(Debug, Default)]
struct Meta {
    primary: Option<u32>,
    /// Items by ID: their types.
    items: Vec<(u32, [u8; 4])>,
    locations: Vec<(u32, Location)>,
    /// References: type, from, to.
    references: Vec<([u8; 4], u32, Vec<u32>)>,
    /// The properties (type and content) in the order of `ipco`.
    properties: Vec<([u8; 4], Range<usize>)>,
    /// The properties of each item, as indices into `properties` from 1.
    associations: Vec<(u32, Vec<u16>)>,
    idat: Option<Range<usize>>,
}

impl Meta {
    fn read(data: &[u8], meta: Range<usize>) -> Option<Self> {
        let mut reader = Reader::new(data, meta);
        reader.full()?;
        let mut out = Self::default();
        let mut picture = false;
        for (kind, content) in boxes(data, reader.rest()) {
            let mut reader = Reader::new(data, content.clone());
            match &kind {
                b"hdlr" => {
                    reader.full()?;
                    reader.u32()?;
                    picture = &reader.fourcc()? == b"pict";
                }
                b"pitm" => {
                    let (version, _) = reader.full()?;
                    out.primary = Some(reader.id(version != 0)?);
                }
                b"iloc" => out.locations = locations(&mut reader)?,
                b"iinf" => {
                    let (version, _) = reader.full()?;
                    reader.id(version != 0)?;
                    for (kind, content) in boxes(data, reader.rest()).take(MAX_ENTRIES) {
                        if &kind != b"infe" {
                            continue;
                        }
                        let mut reader = Reader::new(data, content);
                        let (version, _) = reader.full()?;
                        if version < 2 {
                            continue;
                        }
                        let id = reader.id(version > 2)?;
                        let protected = reader.u16()? != 0;
                        let kind = reader.fourcc()?;
                        if !protected {
                            out.items.push((id, kind));
                        }
                    }
                }
                b"iref" => {
                    let (version, _) = reader.full()?;
                    for (kind, content) in boxes(data, reader.rest()).take(MAX_ENTRIES) {
                        let mut reader = Reader::new(data, content);
                        let from = reader.id(version != 0)?;
                        let count = usize::from(reader.u16()?);
                        let to = (0..count)
                            .map(|_| reader.id(version != 0))
                            .collect::<Option<Vec<_>>>()?;
                        out.references.push((kind, from, to));
                    }
                }
                b"iprp" => {
                    for (kind, content) in boxes(data, content) {
                        if &kind == b"ipco" {
                            out.properties = boxes(data, content).take(MAX_ENTRIES).collect();
                        } else if &kind == b"ipma" {
                            out.associations
                                .extend(associations(&mut Reader::new(data, content))?);
                        }
                    }
                }
                b"idat" => out.idat = Some(content),
                _ => {}
            }
        }
        picture.then_some(out)
    }

    fn kind(&self, id: u32) -> Option<[u8; 4]> {
        self.items
            .iter()
            .find(|(item, _)| *item == id)
            .map(|(_, kind)| *kind)
    }

    /// The properties of an item, in order.
    fn properties_of(&self, id: u32) -> impl Iterator<Item = &([u8; 4], Range<usize>)> {
        self.associations
            .iter()
            .filter(move |(item, _)| *item == id)
            .flat_map(|(_, indices)| indices)
            .filter_map(|index| self.properties.get(usize::from(*index).checked_sub(1)?))
    }

    fn property(&self, id: u32, kind: &[u8; 4]) -> Option<Range<usize>> {
        self.properties_of(id)
            .find(|(property, _)| property == kind)
            .map(|(_, content)| content.clone())
    }

    /// The items a reference of `kind` from `from` names.
    fn referenced(&self, kind: &[u8; 4], from: u32) -> Option<&[u32]> {
        self.references
            .iter()
            .find(|(reference, item, _)| reference == kind && *item == from)
            .map(|(_, _, to)| to.as_slice())
    }

    /// Where an item's data is in the file.
    fn extents(&self, data: &[u8], id: u32) -> Option<Vec<Range<usize>>> {
        let (_, location) = self.locations.iter().find(|(item, _)| *item == id)?;
        let within = if location.in_idat {
            self.idat.clone()?
        } else {
            0..data.len()
        };
        let mut extents = Vec::with_capacity(location.extents.len());
        for (offset, length) in &location.extents {
            let start = within.start.checked_add(usize::try_from(*offset).ok()?)?;
            let end = if *length == 0 {
                within.end
            } else {
                start.checked_add(usize::try_from(*length).ok()?)?
            };
            if start > end || end > within.end {
                return None;
            }
            extents.push(start..end);
        }
        (!extents.is_empty()).then_some(extents)
    }

    /// An AV1 picture or a grid of them, and its size; `None` for other items.
    fn coded(&self, data: &[u8], id: u32) -> Option<(Coded, (u32, u32))> {
        match &self.kind(id)? {
            b"av01" => {
                let mut reader = Reader::new(data, self.property(id, b"ispe")?);
                reader.full()?;
                let size = (reader.u32()?, reader.u32()?);
                Some((Coded::Picture(self.extents(data, id)?), size))
            }
            b"grid" => {
                let grid = bytes(data, &self.extents(data, id)?)?;
                let mut reader = Reader::new(&grid, 0..grid.len());
                let _version = reader.u8()?;
                let wide = reader.u8()? & 1 != 0;
                let rows = u32::from(reader.u8()?) + 1;
                let columns = u32::from(reader.u8()?) + 1;
                let width = reader.id(wide)?;
                let height = reader.id(wide)?;
                let tiles = self.referenced(b"dimg", id)?;
                if tiles.len() != (rows * columns) as usize {
                    return None;
                }
                let tiles = tiles
                    .iter()
                    .map(|tile| {
                        (self.kind(*tile)? == *b"av01")
                            .then(|| self.extents(data, *tile))
                            .flatten()
                    })
                    .collect::<Option<Vec<_>>>()?;
                let grid = Coded::Grid {
                    rows,
                    columns,
                    width,
                    height,
                    tiles,
                };
                Some((grid, (width, height)))
            }
            _ => None,
        }
    }

    /// The image of the primary item.
    fn avif(&self, data: &[u8]) -> Option<Avif> {
        let primary = self.primary?;
        let (color, (width, height)) = self.coded(data, primary)?;
        // The alpha item refers to the primary item.
        let alpha = self
            .references
            .iter()
            .filter(|(kind, _, to)| kind == b"auxl" && to.contains(&primary))
            .map(|(_, from, _)| *from)
            .find(|item| {
                self.property(*item, b"auxC").is_some_and(|content| {
                    let kind = data.get(content.start + 4..content.end).unwrap_or_default();
                    let kind = kind.split(|byte| *byte == 0).next().unwrap_or_default();
                    ALPHA.contains(&kind)
                })
            });
        let premultiplied = alpha.is_some_and(|alpha| {
            self.referenced(b"prem", primary)
                .is_some_and(|to| to.contains(&alpha))
        });
        let alpha = alpha
            .and_then(|alpha| self.coded(data, alpha))
            .map(|(alpha, _)| alpha);
        // The colors of a grid can be those of its first tile.
        let mut sources = vec![primary];
        if let Some(first) = self.referenced(b"dimg", primary).and_then(|tiles| tiles.first()) {
            sources.push(*first);
        }
        let (mut icc, mut nclx) = (None, None);
        for source in sources {
            for (kind, content) in self.properties_of(source) {
                if kind == b"colr" {
                    colour(data, content.clone(), &mut icc, &mut nclx);
                }
            }
        }
        let first = |kind: &[u8; 4]| {
            let content = self.property(primary, kind)?;
            data.get(content)?.first().copied()
        };
        let rotation = first(b"irot").map(|angle| angle & 3);
        let mirror = first(b"imir").map(|axis| axis & 1);
        let crop = self
            .property(primary, b"clap")
            .and_then(|content| clean_aperture(data, content, (width, height)))
            .unwrap_or((width, height));
        Some(Avif {
            width,
            height,
            crop,
            orientation: orientation(rotation.unwrap_or(0), mirror),
            color,
            alpha,
            premultiplied,
            icc,
            nclx,
        })
    }
}

/// The locations of the items of an `iloc` box.
fn locations(reader: &mut Reader<'_>) -> Option<Vec<(u32, Location)>> {
    let (version, _) = reader.full()?;
    if version > 2 {
        return None;
    }
    let sizes = reader.u8()?;
    let (offset_size, length_size) = (sizes >> 4, sizes & 15);
    let sizes = reader.u8()?;
    let base_size = sizes >> 4;
    let index_size = if version == 0 { 0 } else { sizes & 15 };
    let count = if version < 2 {
        u32::from(reader.u16()?)
    } else {
        reader.u32()?
    };
    let mut locations = Vec::new();
    for _ in 0..count.min(MAX_ENTRIES as u32) {
        let id = reader.id(version == 2)?;
        let method = if version == 0 { 0 } else { reader.u16()? & 15 };
        let reference = reader.u16()?;
        let base = reader.sized(base_size)?;
        let count = reader.u16()?;
        // Extents of no bytes each could make millions.
        if count > 256 {
            return None;
        }
        let extents = (0..count)
            .map(|_| {
                reader.sized(index_size)?;
                let offset = base.checked_add(reader.sized(offset_size)?)?;
                Some((offset, reader.sized(length_size)?))
            })
            .collect::<Option<Vec<_>>>()?;
        // Data in the file or in `idat`, not in other files or items.
        if reference == 0 && method <= 1 {
            locations.push((
                id,
                Location {
                    in_idat: method == 1,
                    extents,
                },
            ));
        }
    }
    Some(locations)
}

/// The associations of an `ipma` box: each item's property indices.
fn associations(reader: &mut Reader<'_>) -> Option<Vec<(u32, Vec<u16>)>> {
    let (version, flags) = reader.full()?;
    let count = reader.u32()?;
    let mut associations = Vec::new();
    for _ in 0..count.min(MAX_ENTRIES as u32) {
        let id = reader.id(version != 0)?;
        let indices = (0..reader.u8()?)
            .map(|_| {
                if flags & 1 != 0 {
                    reader.u16().map(|index| index & 0x7FFF)
                } else {
                    reader.u8().map(|index| u16::from(index & 0x7F))
                }
            })
            .collect::<Option<Vec<_>>>()?;
        associations.push((id, indices));
    }
    Some(associations)
}

/// Reads a `colr` box: the first ICC profile and the first code points win.
fn colour(data: &[u8], content: Range<usize>, icc: &mut Option<Range<usize>>, nclx: &mut Option<Nclx>) {
    let mut reader = Reader::new(data, content);
    match reader.fourcc() {
        Some(kind) if (&kind == b"prof" || &kind == b"rICC") && icc.is_none() => *icc = Some(reader.rest()),
        Some(kind) if &kind == b"nclx" && nclx.is_none() => {
            *nclx = read_nclx(&mut reader);
        }
        _ => {}
    }
}

fn read_nclx(reader: &mut Reader<'_>) -> Option<Nclx> {
    Some(Nclx {
        primaries: reader.u16()?,
        transfer: reader.u16()?,
        matrix: reader.u16()?,
        full_range: reader.u8()? & 0x80 != 0,
    })
}

/// The size a clean aperture crops a picture of `size` to, where it starts at the top left
/// corner, as the only ones Chrome applies; `None` for others.
fn clean_aperture(data: &[u8], content: Range<usize>, (width, height): (u32, u32)) -> Option<(u32, u32)> {
    let mut reader = Reader::new(data, content);
    let mut fields = [0_i64; 8];
    for (index, field) in fields.iter_mut().enumerate() {
        let value = reader.u32()?;
        // The offsets are signed.
        *field = if index == 4 || index == 6 {
            i64::from(value as i32)
        } else {
            i64::from(value)
        };
    }
    let [
        width_n,
        width_d,
        height_n,
        height_d,
        across_n,
        across_d,
        down_n,
        down_d,
    ] = fields;
    // A whole number of pixels, centered `offset` from the picture's center: it starts at the
    // left where offset = -(size - crop) / 2.
    let side = |n: i64, d: i64, offset_n: i64, offset_d: i64, size: u32| {
        if d <= 0 || offset_d <= 0 || n % d != 0 {
            return None;
        }
        let crop = n / d;
        let size = i64::from(size);
        (crop > 0 && crop <= size && 2 * offset_n + (size - crop) * offset_d == 0).then_some(crop as u32)
    };
    Some((
        side(width_n, width_d, across_n, across_d, width)?,
        side(height_n, height_d, down_n, down_d, height)?,
    ))
}

/// The EXIF orientation that turns a picture as `irot` (`rotation` quarter turns
/// anticlockwise) and then `imir` (`mirror` 0 top to bottom, 1 left to right) do.
fn orientation(rotation: u8, mirror: Option<u8>) -> u8 {
    match (rotation, mirror) {
        (0, None) => 1,
        (0, Some(0)) | (2, Some(1)) => 4,
        (0 | 2, Some(_)) => 2,
        (1, None) => 8,
        (1, Some(0)) | (3, Some(1)) => 5,
        (1 | 3, Some(_)) => 7,
        (2, None) => 3,
        _ => 6,
    }
}

/// The first frame of the first track of an image sequence without a primary item, and the
/// first frame of the track of its alpha.
fn sequence(data: &[u8], moov: Range<usize>) -> Option<Avif> {
    /// A track: its ID, the track it is the alpha of, its first frame and its sample entry.
    struct Track {
        id: u32,
        alpha_of: Option<u32>,
        frame: Range<usize>,
        entry: Range<usize>,
    }
    let child = |range: Range<usize>, kind: &[u8; 4]| {
        boxes(data, range)
            .find(|(found, _)| found == kind)
            .map(|(_, content)| content)
    };
    let track = |trak: Range<usize>| -> Option<Track> {
        let mut tkhd = Reader::new(data, child(trak.clone(), b"tkhd")?);
        let (version, _) = tkhd.full()?;
        tkhd.take(if version == 1 { 16 } else { 8 })?;
        let id = tkhd.u32()?;
        let alpha_of = child(trak.clone(), b"tref")
            .and_then(|tref| child(tref, b"auxl"))
            .and_then(|auxl| Reader::new(data, auxl).u32());
        let stbl = child(child(child(trak, b"mdia")?, b"minf")?, b"stbl")?;
        let mut stsd = Reader::new(data, child(stbl.clone(), b"stsd")?);
        stsd.full()?;
        stsd.u32()?;
        let (kind, entry) = boxes(data, stsd.rest()).next()?;
        if &kind != b"av01" {
            return None;
        }
        let mut stsz = Reader::new(data, child(stbl.clone(), b"stsz")?);
        stsz.full()?;
        let mut size = stsz.u32()?;
        if size == 0 {
            stsz.u32()?;
            size = stsz.u32()?;
        }
        let start = if let Some(stco) = child(stbl.clone(), b"stco") {
            let mut stco = Reader::new(data, stco);
            stco.full()?;
            stco.u32()?;
            u64::from(stco.u32()?)
        } else {
            let mut co64 = Reader::new(data, child(stbl, b"co64")?);
            co64.full()?;
            co64.u32()?;
            co64.u64()?
        };
        let start = usize::try_from(start).ok()?;
        let end = start.checked_add(size as usize)?;
        (end <= data.len()).then_some(Track {
            id,
            alpha_of,
            frame: start..end,
            entry,
        })
    };
    let tracks: Vec<Track> = boxes(data, moov)
        .filter(|(kind, _)| kind == b"trak")
        .take(64)
        .filter_map(|(_, trak)| track(trak))
        .collect();
    let color = tracks.iter().find(|track| track.alpha_of.is_none())?;
    let alpha = tracks.iter().find(|track| track.alpha_of == Some(color.id));
    // A visual sample entry: its width and height after 24 bytes, its boxes after 78.
    let mut entry = Reader::new(data, color.entry.clone());
    entry.take(24)?;
    let (width, height) = (u32::from(entry.u16()?), u32::from(entry.u16()?));
    let (mut icc, mut nclx) = (None, None);
    let boxes_at = color.entry.start.checked_add(78)?;
    for (kind, content) in boxes(data, boxes_at..color.entry.end) {
        if &kind == b"colr" {
            colour(data, content, &mut icc, &mut nclx);
        }
    }
    Some(Avif {
        width,
        height,
        crop: (width, height),
        orientation: 1,
        color: Coded::Picture(vec![color.frame.clone()]),
        alpha: alpha.map(|alpha| Coded::Picture(vec![alpha.frame.clone()])),
        premultiplied: false,
        icc,
        nclx,
    })
}

/// The color code points of the sequence header of AV1 data (AV1, §5.5.2), for an image
/// without a `colr` box of them.
pub fn sequence_colors(av1: &[u8]) -> Option<Nclx> {
    let mut at = 0;
    // The OBUs: a header, an optional extension, a size and the payload.
    while at < av1.len() {
        let header = av1[at];
        let kind = (header >> 3) & 15;
        at += 1 + usize::from(header & 4 != 0);
        let size = if header & 2 != 0 {
            let mut size = 0_u64;
            let mut shift = 0;
            loop {
                let byte = *av1.get(at)?;
                at += 1;
                size |= u64::from(byte & 0x7F) << shift;
                if byte & 0x80 == 0 {
                    break;
                }
                shift += 7;
                if shift > 56 {
                    return None;
                }
            }
            usize::try_from(size).ok()?
        } else {
            av1.len().checked_sub(at)?
        };
        let payload = av1.get(at..at.checked_add(size)?)?;
        if kind == 1 {
            return sequence_header(&mut Bits { data: payload, at: 0 });
        }
        at += size;
    }
    None
}

/// Reads bits from the most significant on.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Option<bool> {
        let byte = self.data.get(self.at / 8)?;
        let bit = byte >> (7 - self.at % 8) & 1;
        self.at += 1;
        Some(bit != 0)
    }

    fn bits(&mut self, count: u32) -> Option<u32> {
        (0..count).try_fold(0, |value, _| Some(value << 1 | u32::from(self.bit()?)))
    }

    /// A variable-length unsigned number (AV1, §4.10.3), skipped.
    fn skip_uvlc(&mut self) -> Option<()> {
        let mut zeros = 0;
        while !self.bit()? {
            zeros += 1;
            if zeros >= 32 {
                return Some(());
            }
        }
        self.bits(zeros).map(|_| ())
    }
}

/// The color configuration of a sequence header OBU (AV1, §5.5.1 and §5.5.2).
fn sequence_header(bits: &mut Bits<'_>) -> Option<Nclx> {
    let profile = bits.bits(3)?;
    bits.bit()?;
    let reduced = bits.bit()?;
    if reduced {
        bits.bits(5)?;
    } else {
        let mut decoder_model = false;
        let mut delay_length = 0;
        if bits.bit()? {
            // Timing information.
            bits.bits(32)?;
            bits.bits(32)?;
            if bits.bit()? {
                bits.skip_uvlc()?;
            }
            decoder_model = bits.bit()?;
            if decoder_model {
                delay_length = bits.bits(5)? + 1;
                bits.bits(32)?;
                bits.bits(10)?;
            }
        }
        let initial_delay = bits.bit()?;
        for _ in 0..=bits.bits(5)? {
            bits.bits(12)?;
            if bits.bits(5)? > 7 {
                bits.bit()?;
            }
            if decoder_model && bits.bit()? {
                bits.bits(delay_length)?;
                bits.bits(delay_length)?;
                bits.bit()?;
            }
            if initial_delay && bits.bit()? {
                bits.bits(4)?;
            }
        }
    }
    let width_bits = bits.bits(4)? + 1;
    let height_bits = bits.bits(4)? + 1;
    bits.bits(width_bits)?;
    bits.bits(height_bits)?;
    if !reduced && bits.bit()? {
        bits.bits(7)?;
    }
    // 128 × 128 superblocks, filter intra, intra edge filter.
    bits.bits(3)?;
    if !reduced {
        // Inter-intra and masked compounds, warped motion, dual filter.
        bits.bits(4)?;
        let order_hint = bits.bit()?;
        if order_hint {
            bits.bits(2)?;
        }
        let screen_content = if bits.bit()? { 2 } else { bits.bits(1)? };
        if screen_content > 0 && !bits.bit()? {
            bits.bit()?;
        }
        if order_hint {
            bits.bits(3)?;
        }
    }
    // Super-resolution, CDEF, loop restoration.
    bits.bits(3)?;
    // The color configuration.
    let high = bits.bit()?;
    if profile == 2 && high {
        bits.bit()?;
    }
    let monochrome = profile != 1 && bits.bit()?;
    let (primaries, transfer, matrix) = if bits.bit()? {
        (bits.bits(8)?, bits.bits(8)?, bits.bits(8)?)
    } else {
        (2, 2, 2)
    };
    let full_range = if !monochrome && primaries == 1 && transfer == 13 && matrix == 0 {
        true
    } else {
        bits.bit()?
    };
    Some(Nclx {
        primaries: primaries as u16,
        transfer: transfer as u16,
        matrix: matrix as u16,
        full_range,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!(
            "{}/../../tests/corpus/images/media/avif/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(&path).unwrap_or_else(|_| panic!("{path}"))
    }

    #[test]
    fn recognizes_avif_by_its_brands() {
        assert!(is_avif(&fixture("opaque.avif")));
        assert!(is_avif(&fixture("animated.avif")));
        assert!(!is_avif(b"\0\0\0\x14ftypheic\0\0\0\0mif1"));
        assert!(!is_avif(b"\x89PNG\r\n\x1a\n"));
        // The minor version is not a brand.
        assert!(!is_avif(b"\0\0\0\x14ftypmif1avif\0\0\0\0"));
        assert!(is_avif(b"\0\0\0\x14ftypmif1\0\0\0\0avis"));
    }

    #[test]
    fn reads_the_primary_item_and_its_alpha() {
        let data = fixture("alpha.avif");
        let avif = parse(&data).expect("parses");
        assert_eq!(
            (avif.width, avif.height, avif.crop, avif.orientation),
            (24, 16, (24, 16), 1)
        );
        assert!(matches!(avif.color, Coded::Picture(_)));
        assert!(matches!(avif.alpha, Some(Coded::Picture(_))));
        assert!(!avif.premultiplied);
        assert_eq!(
            avif.nclx,
            Some(Nclx {
                primaries: 1,
                transfer: 13,
                matrix: 6,
                full_range: true
            })
        );
        assert!(
            parse(&fixture("premultiplied.avif"))
                .expect("parses")
                .premultiplied
        );
        assert!(parse(&fixture("opaque.avif")).expect("parses").alpha.is_none());
    }

    #[test]
    fn turns_as_irot_and_imir_say() {
        // Measured in Chrome 157: a quarter turn anticlockwise, then top to bottom.
        let avif = parse(&fixture("turned.avif")).expect("parses");
        assert_eq!(avif.orientation, 5);
        let expected = [[1, 4, 2], [8, 5, 7], [3, 2, 4], [6, 7, 5]];
        for (rotation, row) in expected.iter().enumerate() {
            for (mirror, expected) in [None, Some(0), Some(1)].into_iter().zip(row) {
                assert_eq!(
                    orientation(rotation as u8, mirror),
                    *expected,
                    "{rotation} {mirror:?}"
                );
            }
        }
    }

    #[test]
    fn crops_only_to_a_clean_aperture_at_the_top_left() {
        let avif = parse(&fixture("cropped.avif")).expect("parses");
        assert_eq!((avif.width, avif.height, avif.crop), (24, 16, (16, 10)));
        // 16 × 10 at (2, 2) of 24 × 16 is ignored, as in Chrome.
        let clap = |fields: [i32; 8]| {
            fields
                .iter()
                .flat_map(|field| field.to_be_bytes())
                .collect::<Vec<u8>>()
        };
        let centered = clap([16, 1, 10, 1, -2, 1, -1, 1]);
        assert_eq!(clean_aperture(&centered, 0..32, (24, 16)), None);
        let top_left = clap([16, 1, 10, 1, -4, 1, -3, 1]);
        assert_eq!(clean_aperture(&top_left, 0..32, (24, 16)), Some((16, 10)));
        let halves = clap([32, 2, 10, 1, -8, 2, -6, 2]);
        assert_eq!(clean_aperture(&halves, 0..32, (24, 16)), Some((16, 10)));
        let larger = clap([32, 1, 10, 1, 4, 1, -3, 1]);
        assert_eq!(clean_aperture(&larger, 0..32, (24, 16)), None);
        let fraction = clap([33, 2, 10, 1, -4, 1, -3, 1]);
        assert_eq!(clean_aperture(&fraction, 0..32, (24, 16)), None);
    }

    #[test]
    fn reads_grids_and_sequences() {
        let data = fixture("grid.avif");
        let avif = parse(&data).expect("parses");
        let Coded::Grid {
            rows,
            columns,
            width,
            height,
            tiles,
        } = &avif.color
        else {
            panic!("a grid");
        };
        assert_eq!(
            (*rows, *columns, *width, *height, tiles.len()),
            (2, 2, 128, 128, 4)
        );
        // The primary item of a sequence is its first frame.
        let animated = parse(&fixture("animated.avif")).expect("parses");
        assert_eq!(animated.crop, (24, 16));
        // Without the `meta` box, the first frame of the track.
        let mut data = fixture("animated.avif");
        let (_, meta) = boxes(&data, 0..data.len())
            .find(|(kind, _)| kind == b"meta")
            .expect("meta");
        data[meta.start - 4..meta.start].copy_from_slice(b"free");
        let track = parse(&data).expect("parses");
        assert_eq!(track.crop, (24, 16));
        let Coded::Picture(frame) = &track.color else {
            panic!("a picture");
        };
        let Coded::Picture(primary) = &animated.color else {
            panic!("a picture");
        };
        assert_eq!(bytes(&data, frame), bytes(&data, primary));
    }

    #[test]
    fn reads_the_colors_of_the_sequence_header() {
        for (name, expected) in [
            ("opaque.avif", (1, 13, 6, true)),
            ("twelve-bit.avif", (1, 13, 1, false)),
        ] {
            let data = fixture(name);
            let avif = parse(&data).expect("parses");
            let Coded::Picture(parts) = &avif.color else {
                panic!("a picture");
            };
            let colors = sequence_colors(&bytes(&data, parts).expect("data")).expect("colors");
            let (primaries, transfer, matrix, full_range) = expected;
            assert_eq!(
                colors,
                Nclx {
                    primaries,
                    transfer,
                    matrix,
                    full_range
                },
                "{name}"
            );
        }
        assert_eq!(sequence_colors(&[0x0A, 5, 0, 0, 0]), None);
    }

    #[test]
    fn refuses_broken_containers() {
        let data = fixture("opaque.avif");
        for end in [0, 8, 20, 40, 100, data.len() - 1] {
            assert!(parse(&data[..end]).is_none(), "{end}");
        }
        // A box larger than the file ends the boxes.
        assert_eq!(boxes(b"\0\0\x01\0free", 0..8).count(), 0);
        assert_eq!(
            boxes(b"\0\0\0\x01free\xff\xff\xff\xff\xff\xff\xff\xff", 0..16).count(),
            0
        );
    }
}
