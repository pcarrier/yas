use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn font_dirs() -> Vec<String> {
    let mut dirs = Vec::new();
    if let Ok(extra) = std::env::var("YAS_FONT_DIRS") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for d in extra.split(sep) {
            let d = d.trim();
            if !d.is_empty() {
                dirs.push(d.to_owned());
            }
        }
    }
    #[cfg(unix)]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let home = home.to_string_lossy();
            dirs.push(format!("{home}/Library/Fonts"));
            dirs.push(format!("{home}/.local/share/fonts"));
            dirs.push(format!("{home}/.fonts"));
        }
        dirs.push("/Library/Fonts".into());
        dirs.push("/System/Library/Fonts".into());
        dirs.push("/usr/share/fonts".into());
        dirs.push("/usr/local/share/fonts".into());
    }
    #[cfg(windows)]
    {
        if let Ok(windir) = std::env::var("SYSTEMROOT") {
            dirs.push(format!("{windir}\\Fonts"));
        } else {
            dirs.push(r"C:\Windows\Fonts".into());
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(format!(r"{local}\Microsoft\Windows\Fonts"));
        }
    }
    dirs
}

#[derive(Debug, Clone)]
pub struct FontInfo {
    pub family: String,
    pub subfamily: String,
    pub is_monospace: bool,
}

#[derive(Debug, Clone)]
pub struct FontVariant {
    pub path: String,
    /// Which face of the file this is. Always 0 outside `ttcf` collections.
    pub face_index: u32,
    pub weight: String,
    pub style: String,
}

/// Family flag: every usable face has a fixed advance width.
pub const FONT_FAMILY_MONOSPACE: u16 = 1 << 0;
/// Family flag: at least one face has an OpenType variation table.
pub const FONT_FAMILY_VARIABLE: u16 = 1 << 1;
/// Family flag: at least one face has color-glyph tables.
pub const FONT_FAMILY_COLOR: u16 = 1 << 2;
/// Family flag: at least one face may be exported under the selected policy.
pub const FONT_FAMILY_FETCHABLE: u16 = 1 << 3;

/// Face flag: the face has an OpenType variation table.
pub const FONT_FACE_VARIABLE: u16 = 1 << 0;
/// Face flag: the face has color-glyph tables.
pub const FONT_FACE_COLOR: u16 = 1 << 1;
/// Face flag: the face may be exported under the selected policy.
pub const FONT_FACE_FETCHABLE: u16 = 1 << 2;

/// Maximum size of a source font or collection accepted by the catalogue.
///
/// The bounded reader also enforces this limit if a file grows after its
/// metadata is inspected, so neither scanning nor fetching can allocate from
/// an arbitrarily large local file.
pub const MAX_FONT_SOURCE_BYTES: usize = 64 * 1024 * 1024;

/// Container/outlines format delivered to a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum FontFormat {
    TrueType = 0,
    Cff = 1,
    Woff = 2,
    Woff2 = 3,
}

/// Upright/slanted style of one face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum FontStyle {
    Normal = 0,
    Italic = 1,
    Oblique = 2,
}

/// Export is deliberately not implicit. Callers must choose a policy when
/// constructing a catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontExportPolicy {
    Deny,
    Allow,
}

/// Why a face is or is not available for export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontExportStatus {
    Allowed,
    DisabledByPolicy,
    RestrictedLicense,
    BitmapOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub cell_advance: i32,
    pub ascent: i32,
    pub descent: i32,
    pub line_gap: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFaceDescription {
    /// BLAKE3 of the standalone bytes returned by [`FontCatalog::fetch_face`].
    pub content_hash: [u8; 32],
    pub byte_len: u64,
    pub format: FontFormat,
    pub style: FontStyle,
    pub flags: u16,
    /// OpenType `usWeightClass`, normalized to 1..=1000.
    pub weight: u16,
    pub weight_min: u16,
    pub weight_default: u16,
    pub weight_max: u16,
    /// CSS width percentages in tenths (1000 is normal width).
    pub stretch_min: u16,
    pub stretch_default: u16,
    pub stretch_max: u16,
    pub slant_tenths_degrees: i16,
    pub metrics: FontMetrics,
    pub subfamily: String,
    pub postscript_name: String,
    pub export_status: FontExportStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFamilySummary {
    /// Stable catalogue identifier. It is currently the canonical name-table
    /// family name and is deliberately opaque to protocol clients.
    pub family: String,
    pub display_name: String,
    pub flags: u16,
    pub face_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFamilyDescription {
    pub family: String,
    pub display_name: String,
    pub flags: u16,
    pub faces: Vec<FontFaceDescription>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFetchError {
    NotFound,
    DisabledByPolicy,
    RestrictedEmbedding,
    Changed,
    TooLarge,
    Io,
    Invalid,
}

impl fmt::Display for FontFetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotFound => "font face not found",
            Self::DisabledByPolicy => "font export is disabled",
            Self::RestrictedEmbedding => "font embedding license forbids export",
            Self::Changed => "font changed since the catalogue was built",
            Self::TooLarge => "font source exceeds the size limit",
            Self::Io => "font file could not be read",
            Self::Invalid => "font face is invalid",
        })
    }
}

impl std::error::Error for FontFetchError {}

#[derive(Debug, Clone)]
struct FontFaceSource {
    path: PathBuf,
    face_index: u32,
    export_status: FontExportStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FontSourceReadError {
    TooLarge,
    Io,
}

fn read_font_source(path: &Path) -> Result<Vec<u8>, FontSourceReadError> {
    let file = File::open(path).map_err(|_| FontSourceReadError::Io)?;
    let byte_len = file.metadata().map_err(|_| FontSourceReadError::Io)?.len();
    if byte_len > MAX_FONT_SOURCE_BYTES as u64 {
        return Err(FontSourceReadError::TooLarge);
    }

    let mut data = Vec::with_capacity(byte_len as usize);
    file.take(MAX_FONT_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|_| FontSourceReadError::Io)?;
    if data.len() > MAX_FONT_SOURCE_BYTES {
        Err(FontSourceReadError::TooLarge)
    } else {
        Ok(data)
    }
}

/// A deterministic, path-free view of the server's installed fonts.
///
/// Source paths remain private and are keyed by the hash of the exact bytes a
/// client asks for. Files are re-read and re-hashed at fetch time, preventing
/// a changed path from silently serving different content under an old id.
#[derive(Debug, Clone)]
pub struct FontCatalog {
    export_policy: FontExportPolicy,
    families: Vec<FontFamilyDescription>,
    sources: BTreeMap<[u8; 32], Vec<FontFaceSource>>,
}

impl FontCatalog {
    /// Discover the host's fonts. The result is independent of filesystem and
    /// fontconfig enumeration order.
    pub fn scan(export_policy: FontExportPolicy) -> Self {
        let mut paths = BTreeSet::new();
        #[cfg(unix)]
        if let Some(fontconfig_paths) = paths_via_fc_list_all() {
            paths.extend(fontconfig_paths.into_iter().map(PathBuf::from));
        }
        for dir in font_dirs() {
            let mut discovered = Vec::new();
            collect_font_paths(&dir, &mut discovered);
            paths.extend(discovered.into_iter().map(PathBuf::from));
        }
        Self::from_paths(export_policy, paths)
    }

    /// Build a catalogue from a caller-supplied set of files. This is useful
    /// for sandboxed servers and makes deterministic behaviour testable.
    pub fn from_paths<I, P>(export_policy: FontExportPolicy, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let paths: BTreeSet<PathBuf> = paths
            .into_iter()
            .map(|path| path.as_ref().to_path_buf())
            .collect();
        let mut grouped: BTreeMap<String, Vec<FontFaceDescription>> = BTreeMap::new();
        let mut sources: BTreeMap<[u8; 32], Vec<FontFaceSource>> = BTreeMap::new();

        for path in paths {
            let Ok(data) = read_font_source(&path) else {
                continue;
            };
            for face_index in 0..valid_face_count(&data) {
                let Some(face) = describe_catalog_face(&data, face_index, export_policy) else {
                    continue;
                };
                let family = face.0;
                let description = face.1;
                sources
                    .entry(description.content_hash)
                    .or_default()
                    .push(FontFaceSource {
                        path: path.clone(),
                        face_index: face_index as u32,
                        export_status: description.export_status,
                    });
                let faces = grouped.entry(family).or_default();
                if !faces
                    .iter()
                    .any(|known| known.content_hash == description.content_hash)
                {
                    faces.push(description);
                }
            }
        }

        let mut families = Vec::with_capacity(grouped.len());
        for (family, mut faces) in grouped {
            faces.sort_by(|a, b| {
                (a.weight, a.style, &a.subfamily, a.content_hash).cmp(&(
                    b.weight,
                    b.style,
                    &b.subfamily,
                    b.content_hash,
                ))
            });
            let flags = family_flags(&faces);
            families.push(FontFamilyDescription {
                display_name: family.clone(),
                family,
                flags,
                faces,
            });
        }

        Self {
            export_policy,
            families,
            sources,
        }
    }

    pub fn families(&self) -> &[FontFamilyDescription] {
        &self.families
    }

    pub fn summaries(&self) -> Vec<FontFamilySummary> {
        self.families
            .iter()
            .map(|family| FontFamilySummary {
                family: family.family.clone(),
                display_name: family.display_name.clone(),
                flags: family.flags,
                face_count: u16::try_from(family.faces.len()).unwrap_or(u16::MAX),
            })
            .collect()
    }

    pub fn describe(&self, family: &str) -> Option<&FontFamilyDescription> {
        self.families
            .binary_search_by(|candidate| candidate.family.as_str().cmp(family))
            .ok()
            .map(|index| &self.families[index])
            .or_else(|| {
                self.families
                    .iter()
                    .find(|candidate| family_matches(&candidate.family, family))
            })
    }

    /// Fetch the exact standalone bytes identified by `content_hash`.
    pub fn fetch_face(&self, content_hash: &[u8; 32]) -> Result<Vec<u8>, FontFetchError> {
        if self.export_policy == FontExportPolicy::Deny {
            return Err(FontFetchError::DisabledByPolicy);
        }
        let sources = self
            .sources
            .get(content_hash)
            .ok_or(FontFetchError::NotFound)?;
        if let Some(status) = sources
            .iter()
            .map(|source| source.export_status)
            .find(|status| {
                matches!(
                    status,
                    FontExportStatus::RestrictedLicense | FontExportStatus::BitmapOnly
                )
            })
        {
            let _ = status;
            return Err(FontFetchError::RestrictedEmbedding);
        }

        let mut last_error = FontFetchError::Io;
        for source in sources {
            let data = match read_font_source(&source.path) {
                Ok(data) => data,
                Err(FontSourceReadError::TooLarge) => {
                    last_error = FontFetchError::TooLarge;
                    continue;
                }
                Err(FontSourceReadError::Io) => {
                    last_error = FontFetchError::Io;
                    continue;
                }
            };
            let Some(bytes) = standalone_face_bytes(&data, source.face_index) else {
                last_error = FontFetchError::Invalid;
                continue;
            };
            if blake3_hash(&bytes) != *content_hash {
                last_error = FontFetchError::Changed;
                continue;
            }
            return Ok(bytes);
        }
        Err(last_error)
    }
}

/// How many faces a font file holds. Plain sfnt files hold exactly one.
fn face_count(data: &[u8]) -> usize {
    if data.len() >= 12 && &data[0..4] == b"ttcf" {
        u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize
    } else {
        1
    }
}

/// Like [`face_count`], but rejects a collection whose claimed directory does
/// not fit in the file. This bounds catalogue work on untrusted font files.
fn valid_face_count(data: &[u8]) -> usize {
    if data.len() < 12 {
        return 0;
    }
    if &data[0..4] != b"ttcf" {
        return 1;
    }
    let count = face_count(data);
    match count
        .checked_mul(4)
        .and_then(|directory_len| 12usize.checked_add(directory_len))
    {
        Some(end) if end <= data.len() => count,
        _ => 0,
    }
}

fn blake3_hash(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}

fn standalone_face_bytes(data: &[u8], face_index: u32) -> Option<Vec<u8>> {
    if data.starts_with(b"ttcf") {
        extract_face(data, face_index)
    } else if face_index == 0 && face_offset(data, 0).is_some() {
        Some(data.to_vec())
    } else {
        None
    }
}

fn font_format(bytes: &[u8]) -> Option<FontFormat> {
    match bytes.get(..4)? {
        b"OTTO" => Some(FontFormat::Cff),
        b"wOFF" => Some(FontFormat::Woff),
        b"wOF2" => Some(FontFormat::Woff2),
        [0, 1, 0, 0] | b"true" | b"typ1" => Some(FontFormat::TrueType),
        _ => None,
    }
}

fn font_style_in(data: &[u8], face: usize, subfamily: &str) -> FontStyle {
    let normalized = subfamily.to_lowercase();
    if normalized.contains("oblique") {
        return FontStyle::Oblique;
    }
    if normalized.contains("italic") {
        return FontStyle::Italic;
    }
    if let Some(head) = table_slice_in(data, face, b"head")
        && head.len() >= 46
        && u16::from_be_bytes([head[44], head[45]]) & 2 != 0
    {
        return FontStyle::Italic;
    }
    FontStyle::Normal
}

fn fallback_weight(subfamily: &str) -> u16 {
    let name = subfamily.to_lowercase().replace([' ', '-', '_'], "");
    if name.contains("thin") {
        100
    } else if name.contains("extralight") || name.contains("ultralight") {
        200
    } else if name.contains("light") {
        300
    } else if name.contains("medium") {
        500
    } else if name.contains("semibold") || name.contains("demibold") {
        600
    } else if name.contains("extrabold") || name.contains("ultrabold") {
        800
    } else if name.contains("black") || name.contains("heavy") {
        900
    } else if name.contains("bold") {
        700
    } else {
        400
    }
}

fn font_weight_in(data: &[u8], face: usize, subfamily: &str) -> u16 {
    if let Some(os2) = table_slice_in(data, face, b"OS/2")
        && os2.len() >= 6
    {
        let weight = u16::from_be_bytes([os2[4], os2[5]]);
        if (1..=1000).contains(&weight) {
            return weight;
        }
    }
    fallback_weight(subfamily)
}

fn font_stretch_in(data: &[u8], face: usize) -> u16 {
    let width_class = table_slice_in(data, face, b"OS/2")
        .filter(|os2| os2.len() >= 8)
        .map(|os2| u16::from_be_bytes([os2[6], os2[7]]))
        .unwrap_or(5);
    match width_class {
        1 => 500,
        2 => 625,
        3 => 750,
        4 => 875,
        5 => 1000,
        6 => 1125,
        7 => 1250,
        8 => 1500,
        9 => 2000,
        _ => 1000,
    }
}

fn fixed_16_16_scaled(raw: i32, scale: i64) -> i32 {
    let scaled = i64::from(raw).saturating_mul(scale);
    let rounded = if scaled >= 0 {
        scaled.saturating_add(1 << 15)
    } else {
        scaled.saturating_sub(1 << 15)
    } / (1 << 16);
    i32::try_from(rounded).unwrap_or(if rounded < 0 { i32::MIN } else { i32::MAX })
}

fn font_slant_in(data: &[u8], face: usize) -> i16 {
    table_slice_in(data, face, b"post")
        .filter(|post| post.len() >= 8)
        .map(|post| i32::from_be_bytes(post[4..8].try_into().expect("checked post angle")))
        .map(|raw| fixed_16_16_scaled(raw, 10))
        .and_then(|value| i16::try_from(value).ok())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VariationRanges {
    weight: (u16, u16, u16),
    stretch: (u16, u16, u16),
    slant_tenths_degrees: i16,
}

fn variation_ranges_in(
    data: &[u8],
    face: usize,
    weight: u16,
    stretch: u16,
    slant_tenths_degrees: i16,
) -> VariationRanges {
    let mut ranges = VariationRanges {
        weight: (weight, weight, weight),
        stretch: (stretch, stretch, stretch),
        slant_tenths_degrees,
    };
    let Some(fvar) = table_slice_in(data, face, b"fvar").filter(|table| table.len() >= 16) else {
        return ranges;
    };
    let axes_offset = u16::from_be_bytes([fvar[4], fvar[5]]) as usize;
    let axis_count = u16::from_be_bytes([fvar[8], fvar[9]]) as usize;
    let axis_size = u16::from_be_bytes([fvar[10], fvar[11]]) as usize;
    if axis_size < 20 {
        return ranges;
    }
    for index in 0..axis_count {
        let Some(start) = index
            .checked_mul(axis_size)
            .and_then(|offset| axes_offset.checked_add(offset))
        else {
            return ranges;
        };
        let Some(axis) = fvar.get(start..start + 20) else {
            return ranges;
        };
        let raw = |offset: usize| {
            i32::from_be_bytes(
                axis[offset..offset + 4]
                    .try_into()
                    .expect("fixed axis field"),
            )
        };
        match &axis[..4] {
            b"wght" => {
                let values = [raw(4), raw(8), raw(12)]
                    .map(|value| fixed_16_16_scaled(value, 1).clamp(1, 1000) as u16);
                if values[0] <= values[1] && values[1] <= values[2] {
                    ranges.weight = (values[0], values[1], values[2]);
                }
            }
            b"wdth" => {
                let values = [raw(4), raw(8), raw(12)]
                    .map(|value| fixed_16_16_scaled(value, 10).clamp(1, u16::MAX as i32) as u16);
                if values[0] <= values[1] && values[1] <= values[2] {
                    ranges.stretch = (values[0], values[1], values[2]);
                }
            }
            b"slnt" => {
                let value = fixed_16_16_scaled(raw(8), 10);
                if let Ok(value) = i16::try_from(value) {
                    ranges.slant_tenths_degrees = value;
                }
            }
            _ => {}
        }
    }
    ranges
}

fn read_font_metrics_in(data: &[u8], face: usize, monospace: bool) -> FontMetrics {
    let units_per_em = table_slice_in(data, face, b"head")
        .filter(|head| head.len() >= 20)
        .map(|head| u16::from_be_bytes([head[18], head[19]]))
        .unwrap_or(0);
    let (ascent, descent, line_gap) = table_slice_in(data, face, b"hhea")
        .filter(|hhea| hhea.len() >= 10)
        .map(|hhea| {
            (
                i16::from_be_bytes([hhea[4], hhea[5]]) as i32,
                i16::from_be_bytes([hhea[6], hhea[7]]) as i32,
                i16::from_be_bytes([hhea[8], hhea[9]]) as i32,
            )
        })
        .unwrap_or((0, 0, 0));
    let cell_advance = if monospace {
        first_advance_in(data, face).unwrap_or(0) as i32
    } else {
        0
    };
    FontMetrics {
        units_per_em,
        cell_advance,
        ascent,
        descent,
        line_gap,
    }
}

fn first_advance_in(data: &[u8], face: usize) -> Option<u16> {
    let hhea = table_slice_in(data, face, b"hhea")?;
    let hmtx = table_slice_in(data, face, b"hmtx")?;
    if hhea.len() < 36 {
        return None;
    }
    let count = u16::from_be_bytes([hhea[34], hhea[35]]) as usize;
    let metrics_len = count.checked_mul(4)?;
    if count == 0 || metrics_len > hmtx.len() {
        return None;
    }
    (0..count).find_map(|index| {
        let offset = index * 4;
        let advance = u16::from_be_bytes([hmtx[offset], hmtx[offset + 1]]);
        (advance != 0).then_some(advance)
    })
}

fn read_name_id_in(data: &[u8], face: usize, wanted_id: u16) -> Option<String> {
    let table = table_slice_in(data, face, b"name")?;
    if table.len() < 6 {
        return None;
    }
    let count = u16::from_be_bytes([table[2], table[3]]) as usize;
    let records_end = count.checked_mul(12)?.checked_add(6)?;
    if records_end > table.len() {
        return None;
    }
    let strings = u16::from_be_bytes([table[4], table[5]]) as usize;
    let mut best: Option<(u8, String)> = None;
    for index in 0..count {
        let record = 6 + index * 12;
        let platform = u16::from_be_bytes([table[record], table[record + 1]]);
        let language = u16::from_be_bytes([table[record + 4], table[record + 5]]);
        let name_id = u16::from_be_bytes([table[record + 6], table[record + 7]]);
        if name_id != wanted_id {
            continue;
        }
        let platform_priority = match platform {
            3 => 2,
            1 => 1,
            _ => continue,
        };
        let english_priority =
            u8::from((platform == 3 && language == 0x0409) || (platform == 1 && language == 0));
        let priority = platform_priority + english_priority * 4;
        let len = u16::from_be_bytes([table[record + 8], table[record + 9]]) as usize;
        let relative = u16::from_be_bytes([table[record + 10], table[record + 11]]) as usize;
        let Some(start) = strings.checked_add(relative) else {
            continue;
        };
        let Some(end) = start.checked_add(len) else {
            continue;
        };
        let Some(raw) = table.get(start..end) else {
            continue;
        };
        let decoded = if platform == 3 {
            if raw.len() % 2 != 0 {
                continue;
            }
            let utf16: Vec<u16> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_be_bytes(*pair))
                .collect();
            String::from_utf16_lossy(&utf16)
        } else {
            String::from_utf8_lossy(raw).into_owned()
        };
        let decoded = decoded.trim_matches(['\0', ' ']).to_owned();
        if !decoded.is_empty() && best.as_ref().is_none_or(|known| priority > known.0) {
            best = Some((priority, decoded));
        }
    }
    best.map(|(_, name)| name)
}

fn embedding_status_in(
    data: &[u8],
    face: usize,
    export_policy: FontExportPolicy,
) -> FontExportStatus {
    if export_policy == FontExportPolicy::Deny {
        return FontExportStatus::DisabledByPolicy;
    }
    let Some(os2) = table_slice_in(data, face, b"OS/2").filter(|os2| os2.len() >= 10) else {
        return FontExportStatus::Allowed;
    };
    let fs_type = u16::from_be_bytes([os2[8], os2[9]]);
    if fs_type & 0x0002 != 0 {
        FontExportStatus::RestrictedLicense
    } else if fs_type & 0x0200 != 0 {
        FontExportStatus::BitmapOnly
    } else {
        // Preview/print, editable, and no-subsetting licenses permit embedding.
        // The catalogue always exports the whole standalone face.
        FontExportStatus::Allowed
    }
}

fn describe_catalog_face(
    data: &[u8],
    face_index: usize,
    export_policy: FontExportPolicy,
) -> Option<(String, FontFaceDescription)> {
    let face = face_offset(data, face_index)?;
    let info = read_font_info_in(data, face)?;
    let standalone = standalone_face_bytes(data, face_index as u32)?;
    let format = font_format(&standalone)?;
    let variable = table_slice_in(data, face, b"fvar").is_some();
    let color = [b"COLR", b"CBDT", b"sbix", b"SVG "]
        .iter()
        .any(|tag| table_slice_in(data, face, tag).is_some());
    let export_status = embedding_status_in(data, face, export_policy);
    let mut flags = 0;
    if variable {
        flags |= FONT_FACE_VARIABLE;
    }
    if color {
        flags |= FONT_FACE_COLOR;
    }
    if export_status == FontExportStatus::Allowed {
        flags |= FONT_FACE_FETCHABLE;
    }
    let weight = font_weight_in(data, face, &info.subfamily);
    let stretch = font_stretch_in(data, face);
    let variation = variation_ranges_in(data, face, weight, stretch, font_slant_in(data, face));
    let description = FontFaceDescription {
        content_hash: blake3_hash(&standalone),
        byte_len: standalone.len() as u64,
        format,
        style: font_style_in(data, face, &info.subfamily),
        flags,
        weight: variation.weight.1,
        weight_min: variation.weight.0,
        weight_default: variation.weight.1,
        weight_max: variation.weight.2,
        stretch_min: variation.stretch.0,
        stretch_default: variation.stretch.1,
        stretch_max: variation.stretch.2,
        slant_tenths_degrees: variation.slant_tenths_degrees,
        metrics: read_font_metrics_in(data, face, info.is_monospace),
        subfamily: info.subfamily,
        postscript_name: read_name_id_in(data, face, 6).unwrap_or_default(),
        export_status,
    };
    Some((info.family, description))
}

fn family_flags(faces: &[FontFaceDescription]) -> u16 {
    let mut flags = 0;
    if !faces.is_empty() && faces.iter().all(|face| face.metrics.cell_advance > 0) {
        flags |= FONT_FAMILY_MONOSPACE;
    }
    if faces
        .iter()
        .any(|face| face.flags & FONT_FACE_VARIABLE != 0)
    {
        flags |= FONT_FAMILY_VARIABLE;
    }
    if faces.iter().any(|face| face.flags & FONT_FACE_COLOR != 0) {
        flags |= FONT_FAMILY_COLOR;
    }
    if faces
        .iter()
        .any(|face| face.flags & FONT_FACE_FETCHABLE != 0)
    {
        flags |= FONT_FAMILY_FETCHABLE;
    }
    flags
}

/// Byte offset of a face's table directory, or None when the file has no
/// such face.
fn face_offset(data: &[u8], index: usize) -> Option<usize> {
    if data.len() < 12 {
        return None;
    }
    if &data[0..4] != b"ttcf" {
        return if index == 0 { Some(0) } else { None };
    }
    let rec = index.checked_mul(4)?.checked_add(12)?;
    let record = data.get(rec..rec.checked_add(4)?)?;
    Some(u32::from_be_bytes(record.try_into().ok()?) as usize)
}

fn sfnt_offset(data: &[u8]) -> Option<usize> {
    face_offset(data, 0)
}

/// Locate a table within one specific face's directory.
fn table_slice_in<'a>(data: &'a [u8], offset: usize, tag: &[u8; 4]) -> Option<&'a [u8]> {
    let header_end = offset.checked_add(12)?;
    data.get(offset..header_end)?;
    let num_tables = u16::from_be_bytes([data[offset + 4], data[offset + 5]]) as usize;
    let records_end = num_tables.checked_mul(16)?.checked_add(header_end)?;
    data.get(header_end..records_end)?;
    for i in 0..num_tables {
        let rec = header_end + i * 16;
        if &data[rec..rec + 4] == tag {
            let table_offset =
                u32::from_be_bytes([data[rec + 8], data[rec + 9], data[rec + 10], data[rec + 11]])
                    as usize;
            let table_length = u32::from_be_bytes([
                data[rec + 12],
                data[rec + 13],
                data[rec + 14],
                data[rec + 15],
            ]) as usize;
            let table_end = table_offset.checked_add(table_length)?;
            if table_end > data.len() {
                return None;
            }
            return Some(&data[table_offset..table_end]);
        }
    }
    None
}

fn read_is_monospace_in(data: &[u8], face: usize) -> bool {
    let table_slice = |tag: &[u8; 4]| table_slice_in(data, face, tag);
    if let Some(post) = table_slice(b"post")
        && post.len() >= 16
    {
        let is_fixed_pitch = u32::from_be_bytes([post[12], post[13], post[14], post[15]]);
        if is_fixed_pitch != 0 {
            return true;
        }
    }

    let Some(hhea) = table_slice(b"hhea") else {
        return false;
    };
    let Some(hmtx) = table_slice(b"hmtx") else {
        return false;
    };
    if hhea.len() < 36 {
        return false;
    }
    let num_long_metrics = u16::from_be_bytes([hhea[34], hhea[35]]) as usize;
    if num_long_metrics == 0 {
        return false;
    }
    let Some(metrics_len) = num_long_metrics.checked_mul(4) else {
        return false;
    };
    if hmtx.len() < metrics_len {
        return false;
    }

    let mut reference_width: Option<u16> = None;
    for i in 0..num_long_metrics {
        let idx = i * 4;
        let advance = u16::from_be_bytes([hmtx[idx], hmtx[idx + 1]]);
        if advance == 0 {
            continue;
        }
        match reference_width {
            Some(width) if width != advance => return false,
            Some(_) => {}
            None => reference_width = Some(advance),
        }
    }

    reference_width.is_some()
}

/// Read the monospace advance width as a fraction of the em square.
/// Returns `advance_width / units_per_em` for the first non-zero advance in hmtx,
/// matching how native terminals (Ghostty, kitty) compute cell width.
fn read_advance_ratio_in(data: &[u8], face: usize) -> Option<f64> {
    let head = table_slice_in(data, face, b"head")?;
    if head.len() < 20 {
        return None;
    }
    let units_per_em = u16::from_be_bytes([head[18], head[19]]) as f64;
    if units_per_em == 0.0 {
        return None;
    }

    let hhea = table_slice_in(data, face, b"hhea")?;
    let hmtx = table_slice_in(data, face, b"hmtx")?;
    if hhea.len() < 36 {
        return None;
    }
    let num_long_metrics = u16::from_be_bytes([hhea[34], hhea[35]]) as usize;
    if num_long_metrics == 0 || hmtx.len() < num_long_metrics * 4 {
        return None;
    }

    for i in 0..num_long_metrics {
        let idx = i * 4;
        let advance = u16::from_be_bytes([hmtx[idx], hmtx[idx + 1]]);
        if advance > 0 {
            return Some(advance as f64 / units_per_em);
        }
    }
    None
}

/// Read font family and subfamily from a TTF/OTF/TTC file's `name` table.
fn read_font_info(data: &[u8]) -> Option<FontInfo> {
    read_font_info_in(data, sfnt_offset(data)?)
}

fn read_font_info_in(data: &[u8], face: usize) -> Option<FontInfo> {
    let tbl = table_slice_in(data, face, b"name")?;
    if tbl.len() < 6 {
        return None;
    }
    let count = u16::from_be_bytes([tbl[2], tbl[3]]) as usize;
    let string_offset = u16::from_be_bytes([tbl[4], tbl[5]]) as usize;
    if tbl.len() < 6 + count * 12 {
        return None;
    }

    // Collect candidates for name IDs 1 (family), 2 (subfamily), 16 (typo family), 17 (typo subfamily).
    // Prefer platform 3 (Windows UTF-16) over 1 (Mac).
    // Prefer typo (16/17) over legacy (1/2).
    let mut family: Option<String> = None;
    let mut family_pri = 0u8;
    let mut subfamily: Option<String> = None;
    let mut subfamily_pri = 0u8;

    for i in 0..count {
        let rec = 6 + i * 12;
        let platform = u16::from_be_bytes([tbl[rec], tbl[rec + 1]]);
        let language = u16::from_be_bytes([tbl[rec + 4], tbl[rec + 5]]);
        let name_id = u16::from_be_bytes([tbl[rec + 6], tbl[rec + 7]]);
        let length = u16::from_be_bytes([tbl[rec + 8], tbl[rec + 9]]) as usize;
        let str_off = u16::from_be_bytes([tbl[rec + 10], tbl[rec + 11]]) as usize;

        let is_family = name_id == 1 || name_id == 16;
        let is_subfamily = name_id == 2 || name_id == 17;
        if !is_family && !is_subfamily {
            continue;
        }

        let plat_bonus: u8 = if platform == 3 {
            2
        } else if platform == 1 {
            1
        } else {
            0
        };
        if plat_bonus == 0 {
            continue;
        }
        let typo_bonus: u8 = if name_id >= 16 { 4 } else { 0 };
        // Name records repeat per language, and the localized ones are useless
        // to us: the macOS copies of Courier New call their bold face
        // "Negreta", which matches nothing downstream. Rank English first —
        // 0x0409 (en-US) on Windows records, 0 (English) on Mac ones.
        let lang_bonus: u8 =
            if (platform == 3 && language == 0x0409) || (platform == 1 && language == 0) {
                8
            } else {
                0
            };
        let priority = plat_bonus + typo_bonus + lang_bonus;

        let start = string_offset + str_off;
        if start + length > tbl.len() {
            continue;
        }
        let raw = &tbl[start..start + length];

        let decoded = if platform == 3 {
            let chars: Vec<u16> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes(*c))
                .collect();
            String::from_utf16_lossy(&chars)
        } else {
            String::from_utf8_lossy(raw).into_owned()
        };
        let decoded = decoded.trim().to_owned();
        if decoded.is_empty() {
            continue;
        }

        if is_family && priority > family_pri {
            family = Some(decoded);
            family_pri = priority;
        } else if is_subfamily && priority > subfamily_pri {
            subfamily = Some(decoded);
            subfamily_pri = priority;
        }
    }

    Some(FontInfo {
        family: family?,
        subfamily: subfamily.unwrap_or_else(|| "Regular".to_owned()),
        is_monospace: read_is_monospace_in(data, face),
    })
}

fn subfamily_to_weight_style(subfamily: &str) -> (&'static str, &'static str) {
    let s = subfamily.to_lowercase();
    let bold = s.contains("bold") || s.contains("heavy") || s.contains("black");
    let italic = s.contains("italic") || s.contains("oblique");
    match (bold, italic) {
        (true, true) => ("bold", "italic"),
        (true, false) => ("bold", "normal"),
        (false, true) => ("normal", "italic"),
        (false, false) => ("normal", "normal"),
    }
}

/// CSS weight/style for one face.
///
/// `head.macStyle` is the authority: two bits at a fixed offset that mean the
/// same thing in every language, where the subfamily string may be localized
/// (and so unmatchable) or missing. The string still gets a say, because it
/// distinguishes weights macStyle cannot and some fonts leave macStyle clear.
fn weight_style_in(data: &[u8], face: usize, subfamily: &str) -> (&'static str, &'static str) {
    let (mut bold, mut italic) = (false, false);
    if let Some(head) = table_slice_in(data, face, b"head")
        && head.len() >= 46
    {
        let mac_style = u16::from_be_bytes([head[44], head[45]]);
        bold = mac_style & 1 != 0;
        italic = mac_style & 2 != 0;
    }
    let (str_weight, str_style) = subfamily_to_weight_style(subfamily);
    match (
        bold || str_weight == "bold",
        italic || str_style == "italic",
    ) {
        (true, true) => ("bold", "italic"),
        (true, false) => ("bold", "normal"),
        (false, true) => ("normal", "italic"),
        (false, false) => ("normal", "normal"),
    }
}

/// Whether a font's own family name refers to the family being asked for.
/// Space-insensitive because file-scanned names and requested names disagree
/// on them ("PragmataPro Mono" vs "PragmataProMono").
fn family_matches(parsed: &str, requested: &str) -> bool {
    let a = parsed.to_lowercase();
    let b = requested.to_lowercase();
    a == b || a.replace(' ', "") == b.replace(' ', "")
}

/// Every face in one font file that belongs to `family`.
///
/// A `.ttc` collection holds several faces — on macOS that is how Menlo,
/// Courier and the SF families ship their bold and italic — so a file is one
/// candidate per face, not one candidate outright.
fn variants_in_file(path: &str, data: &[u8], family: &str) -> Vec<FontVariant> {
    let mut variants = Vec::new();
    let mut seen = BTreeSet::new();
    for face in 0..face_count(data).max(1) {
        let Some(offset) = face_offset(data, face) else {
            continue;
        };
        let Some(info) = read_font_info_in(data, offset) else {
            continue;
        };
        if !family_matches(&info.family, family) {
            continue;
        }
        let (weight, style) = weight_style_in(data, offset, &info.subfamily);
        // One face per CSS (weight, style) pair; a second claimant would only
        // shadow the first in the stylesheet anyway.
        if !seen.insert((weight, style)) {
            continue;
        }
        variants.push(FontVariant {
            path: path.to_owned(),
            face_index: face as u32,
            weight: weight.to_owned(),
            style: style.to_owned(),
        });
    }
    // Regular, bold, italic, bold-italic: the order a terminal needs them, and
    // so the order the payload budget in font_face_css spends itself in.
    variants.sort_by_key(|v| (v.weight == "bold", v.style == "italic"));
    variants
}

pub fn find_font_files(family: &str) -> Vec<FontVariant> {
    font_files_for_family(family)
        .into_iter()
        .flat_map(|(_, variants)| variants)
        .collect()
}

/// Candidate files for a family, each with its bytes and the faces inside it
/// that match. Reading happens once per file however many faces it yields.
fn font_files_for_family(family: &str) -> Vec<(Vec<u8>, Vec<FontVariant>)> {
    #[cfg(unix)]
    if let Some(paths) = paths_via_fc_match(family) {
        let results = read_matching_faces(paths, family);
        // Fontconfig answers every query with *something*, so an empty result
        // here means it had nothing of this family — fall through and scan.
        if !results.is_empty() {
            return results;
        }
    }
    let mut paths = Vec::new();
    for dir in &font_dirs() {
        collect_font_paths(dir, &mut paths);
    }
    read_matching_faces(paths, family)
}

fn read_matching_faces(paths: Vec<String>, family: &str) -> Vec<(Vec<u8>, Vec<FontVariant>)> {
    let mut results = Vec::new();
    let mut seen_paths = BTreeSet::new();
    for path in paths {
        if !seen_paths.insert(path.clone()) {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let variants = variants_in_file(&path, &data, family);
        if !variants.is_empty() {
            results.push((data, variants));
        }
    }
    results
}

/// Font files fontconfig considers relevant to `family`, best match first.
/// The reported style is ignored: a collection reports one style per listing
/// but contains all of them, so the faces are enumerated from the file itself.
#[cfg(unix)]
fn paths_via_fc_match(family: &str) -> Option<Vec<String>> {
    let output = std::process::Command::new("fc-match")
        .args(["--format", "%{file}\n", "-a", family])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<String> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_owned())
        .collect();
    if paths.is_empty() { None } else { Some(paths) }
}

/// All files known to fontconfig. Names are deliberately ignored: the name
/// table is the catalogue authority and collections are enumerated per face.
#[cfg(unix)]
fn paths_via_fc_list_all() -> Option<Vec<String>> {
    let output = std::process::Command::new("fc-list")
        .args(["--format", "%{file}\n"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let paths: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    (!paths.is_empty()).then_some(paths)
}

fn collect_font_paths(dir: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_font_paths(&path.to_string_lossy(), out);
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "ttf" | "otf" | "woff" | "woff2" | "ttc") {
            out.push(path.to_string_lossy().into_owned());
        }
    }
}

pub fn list_font_families() -> Vec<String> {
    #[cfg(unix)]
    if let Some(families) = list_via_fc_list() {
        return families;
    }
    list_via_name_tables()
}

pub fn list_monospace_font_families() -> Vec<String> {
    #[cfg(unix)]
    if let Some(families) = list_monospace_via_fc_list() {
        return families;
    }
    list_monospace_via_name_tables()
}

#[cfg(unix)]
fn list_via_fc_list() -> Option<Vec<String>> {
    let output = std::process::Command::new("fc-list")
        .args(["--format", "%{family}\n"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut families = BTreeSet::new();
    for line in text.lines() {
        for name in line.split(',') {
            let name = name.trim();
            if !name.is_empty() {
                families.insert(name.to_owned());
            }
        }
    }
    if families.is_empty() {
        return None;
    }
    Some(families.into_iter().collect())
}

fn list_via_name_tables() -> Vec<String> {
    let dirs = font_dirs();
    let mut families = BTreeSet::new();
    for dir in &dirs {
        scan_dir_recursive(dir, &mut families);
    }
    families.into_iter().collect()
}

#[cfg(unix)]
fn list_monospace_via_fc_list() -> Option<Vec<String>> {
    let output = std::process::Command::new("fc-list")
        .args(["--format", "%{file}\n"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut families = BTreeSet::new();
    let mut seen_paths = BTreeSet::new();
    for line in text.lines() {
        let path = line.trim();
        if path.is_empty() || !seen_paths.insert(path.to_owned()) {
            continue;
        }
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        let Some(info) = read_font_info(&data) else {
            continue;
        };
        if !info.is_monospace {
            continue;
        }
        // Use the name table family so the name matches what find_font_files expects.
        families.insert(info.family);
    }
    if families.is_empty() {
        return None;
    }
    Some(families.into_iter().collect())
}

fn list_monospace_via_name_tables() -> Vec<String> {
    let dirs = font_dirs();
    let mut families = BTreeSet::new();
    for dir in &dirs {
        scan_monospace_dir_recursive(dir, &mut families);
    }
    families.into_iter().collect()
}

fn scan_dir_recursive(dir: &str, families: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(&path.to_string_lossy(), families);
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "ttf" | "otf" | "woff" | "woff2" | "ttc") {
            continue;
        }
        if let Ok(data) = std::fs::read(&path)
            && let Some(info) = read_font_info(&data)
        {
            families.insert(info.family);
        }
    }
}

fn scan_monospace_dir_recursive(dir: &str, families: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_monospace_dir_recursive(&path.to_string_lossy(), families);
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "ttf" | "otf" | "woff" | "woff2" | "ttc") {
            continue;
        }
        if let Ok(data) = std::fs::read(&path)
            && let Some(info) = read_font_info(&data)
            && info.is_monospace
        {
            families.insert(info.family);
        }
    }
}

pub fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[(n >> 18 & 63) as usize] as char);
        out.push(CHARS[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[(n >> 6 & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Ceiling on the raw font bytes one stylesheet will inline. Collections that
/// share outlines across faces (CJK families, above all) turn into one full
/// copy per extracted face, and a stylesheet is not the place to discover that
/// you have asked for 80 MB. Faces are ordered regular-first, so what a budget
/// this size drops is the italics of an enormous family, never the text face.
///
/// The first face is exempt, which is what makes that last sentence true: a
/// family whose regular face is *itself* over the cap would otherwise emit no
/// `@font-face` at all and fall back to a default font — a worse outcome than
/// inlining one large face, and one that large CJK text faces (15–30 MB
/// alone) reach routinely.
const MAX_CSS_FONT_BYTES: usize = 24 * 1024 * 1024;

/// Whether a face of `len` bytes gets inlined, given the remaining budget
/// and how many faces are already in the stylesheet. The first one always
/// does — see [`MAX_CSS_FONT_BYTES`].
fn face_fits(len: usize, budget: usize, emitted: usize) -> bool {
    emitted == 0 || len <= budget
}

pub fn font_face_css(family: &str) -> Option<String> {
    let files = font_files_for_family(family);
    if files.is_empty() {
        return None;
    }
    // Escape single quotes in the family name to prevent CSS injection.
    let safe_family = family.replace('\\', "\\\\").replace('\'', "\\'");
    let mut css = String::new();
    let mut budget = MAX_CSS_FONT_BYTES;
    let mut emitted = 0usize;
    for (data, variants) in &files {
        for variant in variants {
            let Some((bytes, mime)) = face_payload(data, variant) else {
                continue;
            };
            if !face_fits(bytes.len(), budget, emitted) {
                continue;
            }
            budget = budget.saturating_sub(bytes.len());
            emitted += 1;
            css.push_str(&format!(
                "@font-face {{ font-family: '{}'; font-weight: {}; font-style: {}; src: url('data:{};base64,{}'); }}\n",
                safe_family,
                variant.weight,
                variant.style,
                mime,
                base64_encode(&bytes),
            ));
        }
    }
    if css.is_empty() { None } else { Some(css) }
}

/// The bytes to serve for one variant, plus their MIME type.
///
/// Single-face files are served verbatim. A face out of a collection has to be
/// rebuilt as a standalone font first: browsers load only the first face of a
/// `ttcf`, so the bold and italic faces of a collection are unreachable
/// otherwise — and unreachable means the browser fakes them by smearing the
/// regular outlines, which is exactly the fat, blurry bold this avoids.
fn face_payload(data: &[u8], variant: &FontVariant) -> Option<(Vec<u8>, &'static str)> {
    if face_count(data) <= 1 {
        let ext = variant.path.rsplit('.').next().unwrap_or("ttf");
        let mime = match ext {
            "otf" => "font/otf",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            _ => "font/ttf",
        };
        return Some((data.to_vec(), mime));
    }
    let bytes = extract_face(data, variant.face_index)?;
    let mime = if bytes.starts_with(b"OTTO") {
        "font/otf"
    } else {
        "font/ttf"
    };
    Some((bytes, mime))
}

/// Sum of a table's bytes read as big-endian u32s, tail zero-padded — the
/// checksum every sfnt table directory record carries.
fn table_checksum(bytes: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in bytes.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

/// Rebuild one face of a font collection as a standalone sfnt file.
///
/// Collection faces share tables by reference, so a face is just a table
/// directory: copy the tables it points at into a fresh file and it stands on
/// its own. Offsets, checksums and the head checksum adjustment are recomputed
/// because they all describe positions that have just changed.
fn extract_face(data: &[u8], face_index: u32) -> Option<Vec<u8>> {
    let base = face_offset(data, face_index as usize)?;
    if base + 12 > data.len() {
        return None;
    }
    let num_tables = u16::from_be_bytes([data[base + 4], data[base + 5]]) as usize;
    if num_tables == 0 || base + 12 + num_tables * 16 > data.len() {
        return None;
    }

    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let rec = base + 12 + i * 16;
        let tag = [data[rec], data[rec + 1], data[rec + 2], data[rec + 3]];
        let offset =
            u32::from_be_bytes([data[rec + 8], data[rec + 9], data[rec + 10], data[rec + 11]])
                as usize;
        let length = u32::from_be_bytes([
            data[rec + 12],
            data[rec + 13],
            data[rec + 14],
            data[rec + 15],
        ]) as usize;
        let end = offset.checked_add(length)?;
        if end > data.len() {
            return None;
        }
        let mut bytes = data[offset..end].to_vec();
        // head.checkSumAdjustment is defined to be zero while checksums are
        // taken, and gets its real value once the file is whole.
        if &tag == b"head" && bytes.len() >= 12 {
            bytes[8..12].fill(0);
        }
        tables.push((tag, bytes));
    }
    // Directory records are in tag order.
    tables.sort_by_key(|(tag, _)| *tag);

    let entry_selector = (usize::BITS - 1 - num_tables.leading_zeros()) as u16;
    let search_range = 16u32 << entry_selector;
    let mut out = Vec::new();
    out.extend_from_slice(&data[base..base + 4]); // sfnt version
    out.extend_from_slice(&(num_tables as u16).to_be_bytes());
    out.extend_from_slice(&(search_range as u16).to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(
        &((num_tables as u32 * 16).wrapping_sub(search_range) as u16).to_be_bytes(),
    );

    let body_start = 12 + num_tables * 16;
    let mut body = Vec::new();
    let mut head_offset = None;
    for (tag, bytes) in &tables {
        let offset = body_start + body.len();
        if tag == b"head" {
            head_offset = Some(offset);
        }
        out.extend_from_slice(tag);
        out.extend_from_slice(&table_checksum(bytes).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        body.extend_from_slice(bytes);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    out.extend_from_slice(&body);

    if let Some(head) = head_offset
        && head + 12 <= out.len()
    {
        let adjustment = 0xB1B0AFBAu32.wrapping_sub(table_checksum(&out));
        out[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    }
    Some(out)
}

/// Return the advance-width / units-per-em ratio for a font family's regular variant.
/// This is how native terminals compute cell width: `ratio * font_size_px`.
pub fn font_advance_ratio(family: &str) -> Option<f64> {
    let files = font_files_for_family(family);
    // Prefer the upright regular face; fall back to whatever reads.
    for want_regular in [true, false] {
        for (data, variants) in &files {
            for variant in variants {
                let is_regular = variant.style == "normal" && variant.weight == "normal";
                if want_regular && !is_regular {
                    continue;
                }
                if let Some(offset) = face_offset(data, variant.face_index as usize)
                    && let Some(ratio) = read_advance_ratio_in(data, offset)
                {
                    return Some(ratio);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    /// The budget sheds a huge family's italics; it must never shed the only
    /// face there is. A regular face over the cap used to be skipped, which
    /// left the stylesheet empty, `font_face_css` returning `None`, and the
    /// terminal on a fallback font — for families (large CJK text faces) that
    /// rendered fine before the cap existed.
    #[test]
    fn the_first_face_is_never_dropped() {
        let huge = MAX_CSS_FONT_BYTES + 1;
        assert!(
            face_fits(huge, MAX_CSS_FONT_BYTES, 0),
            "an oversized regular face is still better than no face"
        );
        // Once something is in the stylesheet, the budget rules again.
        assert!(!face_fits(huge, MAX_CSS_FONT_BYTES, 1));
        assert!(!face_fits(2, 1, 1));
        assert!(face_fits(1, 1, 1), "exactly filling the budget fits");
    }

    use super::*;

    fn build_test_font(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let header_len = 12 + tables.len() * 16;
        let mut data = vec![0u8; header_len];
        data[0..4].copy_from_slice(&[0, 1, 0, 0]);
        data[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());

        let mut offset = header_len;
        for (i, (tag, table)) in tables.iter().enumerate() {
            let rec = 12 + i * 16;
            data[rec..rec + 4].copy_from_slice(*tag);
            data[rec + 8..rec + 12].copy_from_slice(&(offset as u32).to_be_bytes());
            data[rec + 12..rec + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
            data.extend_from_slice(table);
            offset += table.len();
        }

        data
    }

    fn build_fvar(axes: &[(&[u8; 4], i32, i32, i32)]) -> Vec<u8> {
        let mut table = vec![0u8; 16];
        table[0..2].copy_from_slice(&1u16.to_be_bytes());
        table[4..6].copy_from_slice(&16u16.to_be_bytes());
        table[6..8].copy_from_slice(&2u16.to_be_bytes());
        table[8..10].copy_from_slice(&(axes.len() as u16).to_be_bytes());
        table[10..12].copy_from_slice(&20u16.to_be_bytes());
        for (tag, min, default, max) in axes {
            table.extend_from_slice(*tag);
            for value in [min, default, max] {
                table.extend_from_slice(&value.saturating_mul(1 << 16).to_be_bytes());
            }
            table.extend_from_slice(&[0; 4]);
        }
        table
    }

    #[test]
    fn variable_axes_produce_css_weight_stretch_and_slant_ranges() {
        let font = build_test_font(&[(
            b"fvar",
            build_fvar(&[
                (b"wght", 100, 450, 900),
                (b"wdth", 75, 100, 125),
                (b"slnt", -20, -12, 0),
            ]),
        )]);
        assert_eq!(
            variation_ranges_in(&font, 0, 400, 1000, 0),
            VariationRanges {
                weight: (100, 450, 900),
                stretch: (750, 1000, 1250),
                slant_tenths_degrees: -120,
            }
        );
    }

    #[test]
    fn static_width_and_post_angle_are_described() {
        let mut os2 = vec![0u8; 10];
        os2[6..8].copy_from_slice(&3u16.to_be_bytes());
        let mut post = vec![0u8; 8];
        post[4..8].copy_from_slice(&(-12i32).saturating_mul(1 << 16).to_be_bytes());
        let font = build_test_font(&[(b"OS/2", os2), (b"post", post)]);
        assert_eq!(font_stretch_in(&font, 0), 750);
        assert_eq!(font_slant_in(&font, 0), -120);
    }

    #[test]
    fn parse_font_info_from_system_fonts() {
        let families = list_font_families();
        assert!(!families.is_empty(), "no fonts found on system");
        for f in &families {
            assert!(!f.is_empty());
            assert!(!f.contains('\0'));
        }
    }

    /// Wrap several single-face fonts into a `ttcf` collection, rewriting each
    /// face's table offsets to point at the shared body.
    fn build_test_ttc(faces: &[Vec<u8>]) -> Vec<u8> {
        let header_len = 12 + faces.len() * 4;
        let mut out = vec![0u8; header_len];
        out[0..4].copy_from_slice(b"ttcf");
        out[4..8].copy_from_slice(&[0, 1, 0, 0]);
        out[8..12].copy_from_slice(&(faces.len() as u32).to_be_bytes());
        for (i, face) in faces.iter().enumerate() {
            let base = out.len();
            out[12 + i * 4..16 + i * 4].copy_from_slice(&(base as u32).to_be_bytes());
            // Copy the face verbatim, then shift its table offsets by where it
            // landed — offsets in an sfnt are file-absolute.
            out.extend_from_slice(face);
            let num_tables = u16::from_be_bytes([face[4], face[5]]) as usize;
            for t in 0..num_tables {
                let rec = base + 12 + t * 16;
                let off =
                    u32::from_be_bytes([out[rec + 8], out[rec + 9], out[rec + 10], out[rec + 11]]);
                out[rec + 8..rec + 12].copy_from_slice(&(off + base as u32).to_be_bytes());
            }
        }
        out
    }

    /// Minimal `name` table carrying one family, subfamily and PostScript name.
    fn build_name_table(family: &str, subfamily: &str) -> Vec<u8> {
        let postscript = format!(
            "{}-{}",
            family.replace([' ', '-'], ""),
            subfamily.replace([' ', '-'], "")
        );
        let names = [(1u16, family), (2, subfamily), (6, postscript.as_str())];
        let strings: Vec<Vec<u8>> = names
            .iter()
            .map(|(_, value)| {
                value
                    .encode_utf16()
                    .flat_map(|u| u.to_be_bytes())
                    .collect::<Vec<u8>>()
            })
            .collect();
        let count = strings.len();
        let storage = 6 + count * 12;
        let mut tbl = vec![0u8; storage];
        tbl[2..4].copy_from_slice(&(count as u16).to_be_bytes());
        tbl[4..6].copy_from_slice(&(storage as u16).to_be_bytes());
        let mut offset = 0usize;
        for (i, ((name_id, _), bytes)) in names.iter().zip(strings.iter()).enumerate() {
            let rec = 6 + i * 12;
            tbl[rec..rec + 2].copy_from_slice(&3u16.to_be_bytes()); // Windows
            tbl[rec + 2..rec + 4].copy_from_slice(&1u16.to_be_bytes()); // UCS-2
            tbl[rec + 6..rec + 8].copy_from_slice(&name_id.to_be_bytes());
            tbl[rec + 8..rec + 10].copy_from_slice(&(bytes.len() as u16).to_be_bytes());
            tbl[rec + 10..rec + 12].copy_from_slice(&(offset as u16).to_be_bytes());
            offset += bytes.len();
        }
        for bytes in &strings {
            tbl.extend_from_slice(bytes);
        }
        tbl
    }

    fn face_with_names(family: &str, subfamily: &str) -> Vec<u8> {
        face_with_metadata(family, subfamily, fallback_weight(subfamily), 0)
    }

    fn face_with_metadata(family: &str, subfamily: &str, weight: u16, fs_type: u16) -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
        head[8..12].copy_from_slice(&0xDEADBEEFu32.to_be_bytes()); // checkSumAdjustment
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&800i16.to_be_bytes());
        hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
        hhea[8..10].copy_from_slice(&100i16.to_be_bytes());
        hhea[34..36].copy_from_slice(&1u16.to_be_bytes());
        let mut hmtx = vec![0u8; 4];
        hmtx[0..2].copy_from_slice(&600u16.to_be_bytes());
        let mut os2 = vec![0u8; 10];
        os2[4..6].copy_from_slice(&weight.to_be_bytes());
        os2[8..10].copy_from_slice(&fs_type.to_be_bytes());
        build_test_font(&[
            (b"head", head),
            (b"hhea", hhea),
            (b"hmtx", hmtx),
            (b"name", build_name_table(family, subfamily)),
            (b"OS/2", os2),
        ])
    }

    struct TempFont(PathBuf);

    impl TempFont {
        fn write(label: &str, bytes: &[u8]) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("yas-fonts-{}-{id}-{label}.ttc", std::process::id()));
            std::fs::write(&path, bytes).expect("write test font");
            Self(path)
        }

        fn overwrite(&self, bytes: &[u8]) {
            std::fs::write(&self.0, bytes).expect("replace test font");
        }

        fn set_len(&self, byte_len: u64) {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&self.0)
                .expect("open test font")
                .set_len(byte_len)
                .expect("resize test font");
        }
    }

    impl Drop for TempFont {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn enumerates_every_face_of_a_collection() {
        let ttc = build_test_ttc(&[
            face_with_names("Test Mono", "Regular"),
            face_with_names("Test Mono", "Bold"),
            face_with_names("Other", "Regular"),
        ]);
        let variants = variants_in_file("/x/test.ttc", &ttc, "Test Mono");
        assert_eq!(variants.len(), 2, "{variants:?}");
        assert_eq!(
            (variants[0].weight.as_str(), variants[0].face_index),
            ("normal", 0)
        );
        assert_eq!(
            (variants[1].weight.as_str(), variants[1].face_index),
            ("bold", 1)
        );
    }

    #[test]
    fn extracted_face_is_a_standalone_font() {
        let ttc = build_test_ttc(&[
            face_with_names("Test Mono", "Regular"),
            face_with_names("Test Mono", "Bold"),
        ]);
        let bold = extract_face(&ttc, 1).expect("extract");
        assert_eq!(face_count(&bold), 1);
        let info = read_font_info(&bold).expect("name table");
        assert_eq!(info.family, "Test Mono");
        assert_eq!(info.subfamily, "Bold");
        assert_eq!(read_advance_ratio_in(&bold, 0), Some(0.6));
        // Whole-file checksum must land on the magic constant, which is what
        // head.checkSumAdjustment exists to make true.
        assert_eq!(table_checksum(&bold), 0xB1B0AFBA);
    }

    #[test]
    fn extracted_face_directory_is_sorted_and_padded() {
        let ttc = build_test_ttc(&[face_with_names("Test Mono", "Regular")]);
        let face = extract_face(&ttc, 0).expect("extract");
        let num_tables = u16::from_be_bytes([face[4], face[5]]) as usize;
        let mut tags = Vec::new();
        for i in 0..num_tables {
            let rec = 12 + i * 16;
            tags.push(face[rec..rec + 4].to_vec());
            let off =
                u32::from_be_bytes([face[rec + 8], face[rec + 9], face[rec + 10], face[rec + 11]])
                    as usize;
            assert_eq!(off % 4, 0, "table {i} not 4-byte aligned");
        }
        let mut sorted = tags.clone();
        sorted.sort();
        assert_eq!(tags, sorted);
    }

    #[test]
    fn catalogue_describes_and_fetches_standalone_collection_faces() {
        let regular = face_with_metadata("Catalogue Mono", "Regular", 400, 0);
        let bold = face_with_metadata("Catalogue Mono", "Bold Italic", 700, 0);
        let ttc = build_test_ttc(&[regular, bold]);
        let file = TempFont::write("catalogue", &ttc);

        let catalogue = FontCatalog::from_paths(FontExportPolicy::Allow, [&file.0]);
        let summaries = catalogue.summaries();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].family, "Catalogue Mono");
        assert_eq!(summaries[0].face_count, 2);
        assert_eq!(
            summaries[0].flags,
            FONT_FAMILY_MONOSPACE | FONT_FAMILY_FETCHABLE
        );

        let description = catalogue.describe("cataloguemono").expect("family");
        assert_eq!(description.faces.len(), 2);
        assert_eq!(description.faces[0].weight, 400);
        assert_eq!(description.faces[0].style, FontStyle::Normal);
        assert_eq!(description.faces[0].metrics.units_per_em, 1000);
        assert_eq!(description.faces[0].metrics.cell_advance, 600);
        assert_eq!(description.faces[0].metrics.ascent, 800);
        assert_eq!(
            description.faces[0].postscript_name,
            "CatalogueMono-Regular"
        );
        assert_eq!(description.faces[1].weight, 700);
        assert_eq!(description.faces[1].style, FontStyle::Italic);

        for face in &description.faces {
            let bytes = catalogue.fetch_face(&face.content_hash).expect("fetch");
            assert!(!bytes.starts_with(b"ttcf"));
            assert_eq!(bytes.len() as u64, face.byte_len);
            assert_eq!(blake3_hash(&bytes), face.content_hash);
        }
    }

    #[test]
    fn catalogue_order_and_hashes_are_deterministic() {
        let alpha_bytes = face_with_metadata("Alpha Mono", "Regular", 400, 0);
        let zeta_bytes = face_with_metadata("Zeta Mono", "Bold", 700, 0);
        let alpha = TempFont::write("alpha", &alpha_bytes);
        let zeta = TempFont::write("zeta", &zeta_bytes);

        let first = FontCatalog::from_paths(FontExportPolicy::Allow, [&zeta.0, &alpha.0]);
        let second = FontCatalog::from_paths(FontExportPolicy::Allow, [&alpha.0, &zeta.0]);
        assert_eq!(first.families(), second.families());
        assert_eq!(
            first
                .summaries()
                .iter()
                .map(|family| family.family.as_str())
                .collect::<Vec<_>>(),
            ["Alpha Mono", "Zeta Mono"]
        );
        assert_eq!(
            first.families()[0].faces[0].content_hash,
            *blake3::hash(&alpha_bytes).as_bytes()
        );
    }

    #[test]
    fn export_policy_is_explicit_and_disables_fetch() {
        let bytes = face_with_metadata("Private Mono", "Regular", 400, 0);
        let file = TempFont::write("disabled", &bytes);
        let catalogue = FontCatalog::from_paths(FontExportPolicy::Deny, [&file.0]);
        let description = catalogue.describe("Private Mono").unwrap();
        let face = &description.faces[0];
        assert_eq!(face.export_status, FontExportStatus::DisabledByPolicy);
        assert_eq!(face.flags & FONT_FACE_FETCHABLE, 0);
        assert_eq!(description.flags & FONT_FAMILY_FETCHABLE, 0);
        assert_eq!(
            catalogue.fetch_face(&face.content_hash),
            Err(FontFetchError::DisabledByPolicy)
        );
    }

    #[test]
    fn embedding_restrictions_are_enforced_where_parseable() {
        for (fs_type, expected) in [
            (0x0002, FontExportStatus::RestrictedLicense),
            (0x0200, FontExportStatus::BitmapOnly),
        ] {
            let bytes = face_with_metadata("Restricted Mono", "Regular", 400, fs_type);
            let file = TempFont::write("restricted", &bytes);
            let catalogue = FontCatalog::from_paths(FontExportPolicy::Allow, [&file.0]);
            let face = &catalogue.families()[0].faces[0];
            assert_eq!(face.export_status, expected);
            assert_eq!(face.flags & FONT_FACE_FETCHABLE, 0);
            assert_eq!(
                catalogue.fetch_face(&face.content_hash),
                Err(FontFetchError::RestrictedEmbedding)
            );
        }

        // Preview/print embedding and no-subsetting are compatible with
        // delivering the complete standalone face.
        let bytes = face_with_metadata("Preview Mono", "Regular", 400, 0x0104);
        let file = TempFont::write("preview", &bytes);
        let catalogue = FontCatalog::from_paths(FontExportPolicy::Allow, [&file.0]);
        let face = &catalogue.families()[0].faces[0];
        assert_eq!(face.export_status, FontExportStatus::Allowed);
        assert!(catalogue.fetch_face(&face.content_hash).is_ok());
    }

    #[test]
    fn fetch_rejects_a_path_that_changed_after_cataloguing() {
        let original = face_with_metadata("Mutable Mono", "Regular", 400, 0);
        let replacement = face_with_metadata("Mutable Mono", "Bold", 700, 0);
        let file = TempFont::write("mutable", &original);
        let catalogue = FontCatalog::from_paths(FontExportPolicy::Allow, [&file.0]);
        let hash = catalogue.families()[0].faces[0].content_hash;
        file.overwrite(&replacement);
        assert_eq!(catalogue.fetch_face(&hash), Err(FontFetchError::Changed));
    }

    #[test]
    fn oversized_source_files_are_skipped_during_scan_and_rejected_during_fetch() {
        let bytes = face_with_metadata("Bounded Mono", "Regular", 400, 0);

        let scan_file = TempFont::write("oversized-scan", &bytes);
        scan_file.set_len(MAX_FONT_SOURCE_BYTES as u64 + 1);
        let scanned = FontCatalog::from_paths(FontExportPolicy::Allow, [&scan_file.0]);
        assert!(scanned.families().is_empty());

        let fetch_file = TempFont::write("oversized-fetch", &bytes);
        let catalogue = FontCatalog::from_paths(FontExportPolicy::Allow, [&fetch_file.0]);
        let hash = catalogue.families()[0].faces[0].content_hash;
        fetch_file.set_len(MAX_FONT_SOURCE_BYTES as u64 + 1);
        assert_eq!(catalogue.fetch_face(&hash), Err(FontFetchError::TooLarge));
    }

    #[test]
    fn subfamily_parsing() {
        assert_eq!(subfamily_to_weight_style("Regular"), ("normal", "normal"));
        assert_eq!(subfamily_to_weight_style("Bold"), ("bold", "normal"));
        assert_eq!(subfamily_to_weight_style("Italic"), ("normal", "italic"));
        assert_eq!(subfamily_to_weight_style("Bold Italic"), ("bold", "italic"));
        assert_eq!(
            subfamily_to_weight_style("Bold Oblique"),
            ("bold", "italic")
        );
    }

    #[test]
    fn detects_monospace_from_post_table() {
        let mut post = vec![0u8; 32];
        post[12..16].copy_from_slice(&1u32.to_be_bytes());
        let font = build_test_font(&[(b"post", post)]);
        assert!(read_is_monospace_in(&font, 0));
    }

    #[test]
    fn detects_monospace_from_uniform_hmtx_widths() {
        let mut hhea = vec![0u8; 36];
        hhea[34..36].copy_from_slice(&2u16.to_be_bytes());

        let mut hmtx = vec![0u8; 8];
        hmtx[0..2].copy_from_slice(&600u16.to_be_bytes());
        hmtx[4..6].copy_from_slice(&600u16.to_be_bytes());

        let font = build_test_font(&[(b"hhea", hhea), (b"hmtx", hmtx)]);
        assert!(read_is_monospace_in(&font, 0));
    }

    #[test]
    fn rejects_variable_width_fonts() {
        let mut hhea = vec![0u8; 36];
        hhea[34..36].copy_from_slice(&2u16.to_be_bytes());

        let mut hmtx = vec![0u8; 8];
        hmtx[0..2].copy_from_slice(&500u16.to_be_bytes());
        hmtx[4..6].copy_from_slice(&700u16.to_be_bytes());

        let font = build_test_font(&[(b"hhea", hhea), (b"hmtx", hmtx)]);
        assert!(!read_is_monospace_in(&font, 0));
    }

    // ── base64_encode ──

    #[test]
    fn base64_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn base64_one_byte() {
        assert_eq!(base64_encode(b"M"), "TQ==");
    }

    #[test]
    fn base64_two_bytes() {
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }

    #[test]
    fn base64_three_bytes() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
    }

    #[test]
    fn base64_rfc4648_vectors() {
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    // ── sfnt_offset ──

    #[test]
    fn sfnt_offset_too_short() {
        assert_eq!(sfnt_offset(b"abc"), None);
    }

    #[test]
    fn sfnt_offset_non_ttc() {
        let font = build_test_font(&[]);
        assert_eq!(sfnt_offset(&font), Some(0));
    }

    #[test]
    fn sfnt_offset_ttc_header() {
        let mut data = vec![0u8; 20];
        data[0..4].copy_from_slice(b"ttcf");
        data[12..16].copy_from_slice(&100u32.to_be_bytes());
        assert_eq!(sfnt_offset(&data), Some(100));
    }

    #[test]
    fn sfnt_offset_ttc_too_short() {
        let mut data = vec![0u8; 14];
        data[0..4].copy_from_slice(b"ttcf");
        assert_eq!(sfnt_offset(&data), None);
    }

    // ── table_slice ──

    #[test]
    fn table_slice_found() {
        let table_data = vec![1, 2, 3, 4];
        let font = build_test_font(&[(b"test", table_data.clone())]);
        let slice = table_slice_in(&font, 0, b"test");
        assert_eq!(slice, Some(table_data.as_slice()));
    }

    #[test]
    fn table_slice_not_found() {
        let font = build_test_font(&[(b"aaaa", vec![0])]);
        assert_eq!(table_slice_in(&font, 0, b"zzzz"), None);
    }

    #[test]
    fn table_slice_empty_font() {
        let font = build_test_font(&[]);
        assert_eq!(table_slice_in(&font, 0, b"test"), None);
    }

    // ── read_advance_ratio ──

    #[test]
    fn advance_ratio_basic() {
        let mut head = vec![0u8; 20];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());

        let mut hhea = vec![0u8; 36];
        hhea[34..36].copy_from_slice(&1u16.to_be_bytes());

        let mut hmtx = vec![0u8; 4];
        hmtx[0..2].copy_from_slice(&600u16.to_be_bytes());

        let font = build_test_font(&[(b"head", head), (b"hhea", hhea), (b"hmtx", hmtx)]);
        let ratio = read_advance_ratio_in(&font, 0).unwrap();
        assert!((ratio - 0.6).abs() < 1e-10);
    }

    #[test]
    fn advance_ratio_skips_zero_advances() {
        let mut head = vec![0u8; 20];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());

        let mut hhea = vec![0u8; 36];
        hhea[34..36].copy_from_slice(&2u16.to_be_bytes());

        let mut hmtx = vec![0u8; 8];
        hmtx[0..2].copy_from_slice(&0u16.to_be_bytes());
        hmtx[4..6].copy_from_slice(&500u16.to_be_bytes());

        let font = build_test_font(&[(b"head", head), (b"hhea", hhea), (b"hmtx", hmtx)]);
        let ratio = read_advance_ratio_in(&font, 0).unwrap();
        assert!((ratio - 0.5).abs() < 1e-10);
    }

    #[test]
    fn advance_ratio_no_head_table() {
        let hhea = vec![0u8; 36];
        let hmtx = vec![0u8; 4];
        let font = build_test_font(&[(b"hhea", hhea), (b"hmtx", hmtx)]);
        assert!(read_advance_ratio_in(&font, 0).is_none());
    }

    #[test]
    fn advance_ratio_zero_units_per_em() {
        let head = vec![0u8; 20];
        let hhea = vec![0u8; 36];
        let hmtx = vec![0u8; 4];
        let font = build_test_font(&[(b"head", head), (b"hhea", hhea), (b"hmtx", hmtx)]);
        assert!(read_advance_ratio_in(&font, 0).is_none());
    }

    // ── subfamily_to_weight_style (extra cases) ──

    #[test]
    fn subfamily_heavy() {
        assert_eq!(subfamily_to_weight_style("Heavy"), ("bold", "normal"));
    }

    #[test]
    fn subfamily_black() {
        assert_eq!(subfamily_to_weight_style("Black"), ("bold", "normal"));
    }

    #[test]
    fn subfamily_oblique() {
        assert_eq!(subfamily_to_weight_style("Oblique"), ("normal", "italic"));
    }

    #[test]
    fn subfamily_case_insensitive() {
        assert_eq!(subfamily_to_weight_style("BOLD ITALIC"), ("bold", "italic"));
        assert_eq!(subfamily_to_weight_style("bold italic"), ("bold", "italic"));
    }

    #[test]
    fn subfamily_unrecognized() {
        assert_eq!(subfamily_to_weight_style("Light"), ("normal", "normal"));
        assert_eq!(subfamily_to_weight_style("Thin"), ("normal", "normal"));
    }
}
