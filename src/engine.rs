//! Archive engine: format detection + unified read/write over pure-Rust codecs.
//!
//! Deliberately no shelling out to `7z`. Everything here is native Rust so the
//! binary is self-contained, cross-platform, and fast.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    SevenZ,
    Zip,
    Tar,
    TarGz,
    TarXz,
    TarBz2,
    TarZst,
    Gz,
    Xz,
    Bz2,
    Zst,
    Rar,
    /// Recognised by magic bytes or unknown extension where no reader is available.
    Unknown,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::SevenZ => "7z",
            Format::Zip => "zip",
            Format::Tar => "tar",
            Format::TarGz => "tar.gz",
            Format::TarXz => "tar.xz",
            Format::TarBz2 => "tar.bz2",
            Format::TarZst => "tar.zst",
            Format::Gz => "gz",
            Format::Xz => "xz",
            Format::Bz2 => "bz2",
            Format::Zst => "zst",
            Format::Rar => "rar",
            Format::Unknown => "unknown",
        }
    }

    /// RAR is extract/list only — we never create it.
    pub fn can_write(self) -> bool {
        matches!(
            self,
            Format::SevenZ
                | Format::Zip
                | Format::Tar
                | Format::TarGz
                | Format::TarXz
                | Format::TarBz2
                | Format::TarZst
                | Format::Gz
                | Format::Xz
                | Format::Bz2
                | Format::Zst
        )
    }

    #[allow(dead_code)]
    pub fn is_single_file(self) -> bool {
        matches!(self, Format::Gz | Format::Xz | Format::Bz2 | Format::Zst)
    }

    pub fn extensions() -> &'static [(&'static str, Format)] {
        &[
            // Compound tar extensions must come before single extensions (.gz, .xz, .bz2, .zst)
            ("tar.gz", Format::TarGz),
            ("tgz", Format::TarGz),
            ("tar.xz", Format::TarXz),
            ("txz", Format::TarXz),
            ("tar.bz2", Format::TarBz2),
            ("tbz2", Format::TarBz2),
            ("tbz", Format::TarBz2),
            ("tar.zst", Format::TarZst),
            ("tzst", Format::TarZst),
            ("7z", Format::SevenZ),
            ("cb7", Format::SevenZ),
            ("zip", Format::Zip),
            ("jar", Format::Zip),
            ("war", Format::Zip),
            ("ear", Format::Zip),
            ("apk", Format::Zip),
            ("aab", Format::Zip),
            ("ipa", Format::Zip),
            ("whl", Format::Zip),
            ("nupkg", Format::Zip),
            ("vsix", Format::Zip),
            ("crx", Format::Zip),
            ("xpi", Format::Zip),
            ("oxt", Format::Zip),
            ("cbz", Format::Zip),
            ("epub", Format::Zip),
            ("docx", Format::Zip),
            ("xlsx", Format::Zip),
            ("pptx", Format::Zip),
            ("odt", Format::Zip),
            ("ods", Format::Zip),
            ("odp", Format::Zip),
            ("tar", Format::Tar),
            ("cbt", Format::Tar),
            ("gz", Format::Gz),
            ("xz", Format::Xz),
            ("bz2", Format::Bz2),
            ("zst", Format::Zst),
            ("rar", Format::Rar),
            ("cbr", Format::Rar),
            // Multi-volume RAR parts are all openable as the archive itself.
            ("r00", Format::Rar),
            ("r01", Format::Rar),
        ]
    }
}

/// Compression level presets modelled after 7-Zip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionLevel {
    Store,
    Fastest,
    Fast,
    Normal,
    Maximum,
    Ultra,
}

impl CompressionLevel {
    #[allow(dead_code)]
    pub const ALL: [CompressionLevel; 6] = [
        CompressionLevel::Store,
        CompressionLevel::Fastest,
        CompressionLevel::Fast,
        CompressionLevel::Normal,
        CompressionLevel::Maximum,
        CompressionLevel::Ultra,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CompressionLevel::Store => "0 - Store",
            CompressionLevel::Fastest => "1 - Fastest",
            CompressionLevel::Fast => "3 - Fast",
            CompressionLevel::Normal => "5 - Normal",
            CompressionLevel::Maximum => "7 - Maximum",
            CompressionLevel::Ultra => "9 - Ultra",
        }
    }

    pub fn numeric_0_to_9(self) -> u32 {
        match self {
            CompressionLevel::Store => 0,
            CompressionLevel::Fastest => 1,
            CompressionLevel::Fast => 3,
            CompressionLevel::Normal => 6,
            CompressionLevel::Maximum => 7,
            CompressionLevel::Ultra => 9,
        }
    }

    pub fn zstd_level(self) -> i32 {
        match self {
            CompressionLevel::Store => 1,
            CompressionLevel::Fastest => 1,
            CompressionLevel::Fast => 3,
            CompressionLevel::Normal => 6,
            CompressionLevel::Maximum => 12,
            CompressionLevel::Ultra => 19,
        }
    }
}

/// Compression codecs supported when creating `.7z` archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SevenZMethod {
    Lzma2,
    Lzma,
    Zstd,
    Bzip2,
    Deflate,
    Brotli,
    Lz4,
    Copy,
}

impl SevenZMethod {
    pub const ALL: [SevenZMethod; 8] = [
        SevenZMethod::Lzma2,
        SevenZMethod::Lzma,
        SevenZMethod::Zstd,
        SevenZMethod::Bzip2,
        SevenZMethod::Deflate,
        SevenZMethod::Brotli,
        SevenZMethod::Lz4,
        SevenZMethod::Copy,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SevenZMethod::Lzma2 => "LZMA2",
            SevenZMethod::Lzma => "LZMA",
            SevenZMethod::Zstd => "ZSTD",
            SevenZMethod::Bzip2 => "BZip2",
            SevenZMethod::Deflate => "Deflate",
            SevenZMethod::Brotli => "Brotli",
            SevenZMethod::Lz4 => "LZ4",
            SevenZMethod::Copy => "Copy (Store)",
        }
    }
}

/// Compression codecs supported when creating `.zip` archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZipMethod {
    Deflate,
    Zstd,
    Bzip2,
    Lzma,
    Store,
}

impl ZipMethod {
    pub const ALL: [ZipMethod; 5] = [
        ZipMethod::Deflate,
        ZipMethod::Zstd,
        ZipMethod::Bzip2,
        ZipMethod::Lzma,
        ZipMethod::Store,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ZipMethod::Deflate => "Deflate",
            ZipMethod::Zstd => "ZSTD",
            ZipMethod::Bzip2 => "BZip2",
            ZipMethod::Lzma => "LZMA",
            ZipMethod::Store => "Store",
        }
    }
}

/// Configuration for creating a new archive.
#[derive(Debug, Clone)]
pub struct CreateOptions {
    pub format: Format,
    pub level: CompressionLevel,
    pub sevenz_method: SevenZMethod,
    pub zip_method: ZipMethod,
    pub solid: bool,
    pub password: Option<String>,
    pub encrypt_header: bool,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            format: Format::SevenZ,
            level: CompressionLevel::Normal,
            sevenz_method: SevenZMethod::Lzma2,
            zip_method: ZipMethod::Deflate,
            solid: true,
            password: None,
            encrypt_header: true,
        }
    }
}

/// Probe whether a reader begins with a valid POSIX/GNU `ustar` tar header.
fn stream_looks_like_tar<R: Read>(mut r: R) -> bool {
    let mut header = [0u8; 512];
    if r.read_exact(&mut header).is_err() {
        return false;
    }
    &header[257..262] == b"ustar"
}

pub fn detect(path: &Path) -> Format {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    for (ext, fmt) in Format::extensions() {
        if name.ends_with(&format!(".{ext}")) {
            return *fmt;
        }
    }
    // Magic-byte fallback
    if let Ok(mut f) = File::open(path) {
        let mut head = [0u8; 8];
        if f.read_exact(&mut head).is_ok() {
            if &head[..6] == b"7z\xBC\xAF\x27\x1C" {
                return Format::SevenZ;
            }
            // RAR 1.5-4.x and RAR 5.x
            if &head[..7] == b"Rar!\x1A\x07\x00" || &head[..8] == b"Rar!\x1A\x07\x01\x00" {
                return Format::Rar;
            }
            if &head[..4] == b"PK\x03\x04" || &head[..4] == b"PK\x05\x06" {
                return Format::Zip;
            }
            if &head[..2] == [0x1F, 0x8B] {
                if let Ok(f2) = File::open(path) {
                    if stream_looks_like_tar(flate2::read::GzDecoder::new(BufReader::new(f2))) {
                        return Format::TarGz;
                    }
                }
                return Format::Gz;
            }
            if &head[..6] == b"\xFD7zXZ\0" {
                if let Ok(f2) = File::open(path) {
                    if stream_looks_like_tar(xz2::read::XzDecoder::new(BufReader::new(f2))) {
                        return Format::TarXz;
                    }
                }
                return Format::Xz;
            }
            if &head[..3] == b"BZh" && head[3].is_ascii_digit() {
                if let Ok(f2) = File::open(path) {
                    if stream_looks_like_tar(bzip2::read::BzDecoder::new(BufReader::new(f2))) {
                        return Format::TarBz2;
                    }
                }
                return Format::Bz2;
            }
            if &head[..4] == [0x28, 0xB5, 0x2F, 0xFD] {
                if let Ok(f2) = File::open(path) {
                    if let Ok(dec) = zstd::stream::read::Decoder::new(BufReader::new(f2)) {
                        if stream_looks_like_tar(dec) {
                            return Format::TarZst;
                        }
                    }
                }
                return Format::Zst;
            }
            // Uncompressed tar: "ustar" sits at offset 257, too far for `head`.
            if let Ok(mut f2) = File::open(path) {
                if f2.seek(std::io::SeekFrom::Start(257)).is_ok() {
                    let mut magic = [0u8; 5];
                    if f2.read_exact(&mut magic).is_ok() && &magic == b"ustar" {
                        return Format::Tar;
                    }
                }
            }
        }
    }
    // Never guess: an unrecognised file is reported as unknown rather than
    // being handed to the 7z reader, which produced a confusing BadSignature.
    Format::Unknown
}

/// One row in the archive listing.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub packed: u64,
    pub is_dir: bool,
    pub modified: Option<String>,
    pub crc: Option<u32>,
    pub method: Option<String>,
    #[allow(dead_code)]
    pub encrypted: bool,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    PasswordRequired,
    WrongPassword,
    Other(String),
}

impl Error {
    pub fn is_password_error(&self) -> bool {
        matches!(self, Error::PasswordRequired | Error::WrongPassword)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::PasswordRequired => write!(f, "Password required to open encrypted archive"),
            Error::WrongPassword => write!(f, "Incorrect password for encrypted archive"),
            Error::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Other(s)
    }
}

fn map_7z_error(e: sevenz_rust2::Error, has_password: bool) -> Error {
    match e {
        sevenz_rust2::Error::PasswordRequired => Error::PasswordRequired,
        sevenz_rust2::Error::MaybeBadPassword(_) => {
            if has_password {
                Error::WrongPassword
            } else {
                Error::PasswordRequired
            }
        }
        sevenz_rust2::Error::NextHeaderCrcMismatch
        | sevenz_rust2::Error::ChecksumVerificationFailed
            if has_password =>
        {
            Error::WrongPassword
        }
        other => {
            let msg = other.to_string();
            if msg.contains("PasswordRequired") {
                Error::PasswordRequired
            } else if msg.contains("MaybeBadPassword") {
                if has_password {
                    Error::WrongPassword
                } else {
                    Error::PasswordRequired
                }
            } else {
                Error::Other(msg)
            }
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// List the contents of an archive without extracting.
// Convenience wrapper for callers that do not need progress or warnings.
#[allow(dead_code)]
pub fn list(path: &Path) -> Result<Vec<Entry>> {
    list_with_password(path, None)
}

/// List the contents of an archive, supplying an optional password for encrypted headers.
pub fn list_with_password(path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    Ok(list_with_report(path, password)?.0)
}

/// Like [`list_with_password`], but a truncated compressed stream is not an
/// error: everything readable is returned together with the reason the walk
/// stopped. A partially downloaded `.tar.gz` therefore still opens, listing the
/// entries that are actually present.
pub fn list_with_report(
    path: &Path,
    password: Option<&str>,
) -> Result<(Vec<Entry>, Option<String>)> {
    list_with_report_progress(path, password, None)
}

/// [`list_with_report`] while reporting how far through the file we are.
pub fn list_with_report_progress(
    path: &Path,
    password: Option<&str>,
    progress: Option<ProgressSink<'_>>,
) -> Result<(Vec<Entry>, Option<String>)> {
    let fmt = detect(path);
    match fmt {
        Format::SevenZ => Ok((list_7z(path, password)?, None)),
        Format::Zip => Ok((list_zip(path)?, None)),
        Format::Rar => Ok((list_rar(path)?, None)),
        Format::Gz | Format::Xz | Format::Bz2 | Format::Zst => {
            Ok((list_single(path, fmt)?, None))
        }
        Format::Unknown => Err(Error::Other("unrecognised archive format".to_string())),
        _ => list_tar(path, progress),
    }
}

/// Single-file compressed streams (`.gz`, `.xz`, `.bz2`, `.zst`) expose a single
/// virtual entry named after the archive without its compression suffix.
fn single_inner_name(archive: &Path, fmt: Format) -> String {
    let fname = archive
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("extracted");
    let suffix = format!(".{}", fmt.label());
    if fname.len() > suffix.len() && fname.to_ascii_lowercase().ends_with(&suffix) {
        fname[..fname.len() - suffix.len()].to_string()
    } else {
        archive
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("extracted")
            .to_string()
    }
}

fn list_single(path: &Path, fmt: Format) -> Result<Vec<Entry>> {
    let md = std::fs::metadata(path)?;
    let packed = md.len();
    let modified = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| format_epoch(d.as_secs()));

    let size = if fmt == Format::Gz && packed >= 4 {
        // GZIP stores ISIZE (uncompressed size modulo 2^32) in the last 4 bytes.
        let mut f = File::open(path)?;
        if f.seek(io::SeekFrom::End(-4)).is_ok() {
            let mut tail = [0u8; 4];
            if f.read_exact(&mut tail).is_ok() {
                u32::from_le_bytes(tail) as u64
            } else {
                0
            }
        } else {
            0
        }
    } else {
        0
    };

    Ok(vec![Entry {
        path: single_inner_name(path, fmt),
        size,
        packed,
        is_dir: false,
        modified,
        crc: None,
        method: Some(fmt.label().to_uppercase()),
        encrypted: false,
    }])
}

/// RAR is handled by the vendored unRAR C++ code (via the `unrar` crate).
/// We can list and extract it, but never create it.
fn list_rar(path: &Path) -> Result<Vec<Entry>> {
    let listing = unrar::Archive::new(path)
        .open_for_listing()
        .map_err(|e| Error::Other(format!("rar: {e}")))?;

    let mut out = Vec::new();
    for item in listing {
        let h = item.map_err(|e| Error::Other(format!("rar: {e}")))?;
        out.push(Entry {
            path: h.filename.to_string_lossy().replace('\\', "/"),
            size: h.unpacked_size,
            packed: 0,
            is_dir: h.is_directory(),
            modified: Some(dos_time(h.file_time)),
            crc: Some(h.file_crc),
            method: Some("RAR".to_string()),
            encrypted: h.is_encrypted(),
        });
    }
    Ok(out)
}

/// RAR stores MS-DOS timestamps. Convert to the same textual form the other
/// formats use, so sorting is consistent across formats.
fn dos_time(time: u32) -> String {
    if time == 0 {
        return String::new();
    }
    let date = time >> 16;
    let tm = time & 0xFFFF;
    let (y, mo, d) = (
        ((date >> 9) & 0x7F) + 1980,
        (date >> 5) & 0x0F,
        date & 0x1F,
    );
    let (h, mi) = ((tm >> 11) & 0x1F, (tm >> 5) & 0x3F);
    if mo == 0 || d == 0 {
        return String::new();
    }
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}")
}

fn make_7z_password(password: Option<&str>) -> sevenz_rust2::Password {
    match password.filter(|s| !s.is_empty()) {
        Some(p) => sevenz_rust2::Password::from(p),
        None => sevenz_rust2::Password::empty(),
    }
}

fn list_7z(path: &Path, password: Option<&str>) -> Result<Vec<Entry>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let has_pwd = password.is_some_and(|p| !p.is_empty());
    let pwd = make_7z_password(password);

    let ar = sevenz_rust2::ArchiveReader::new(reader, pwd)
        .map_err(|e| map_7z_error(e, has_pwd))?;
    let raw = ar.archive();

    let mut out = Vec::with_capacity(raw.files.len());
    let mut methods_buf = Vec::new();

    for (file_idx, entry) in raw.files.iter().enumerate() {
        methods_buf.clear();
        let _ = ar.file_compression_methods(entry.name(), &mut methods_buf);
        let encrypted = methods_buf
            .iter()
            .any(|m| m.id() == sevenz_rust2::EncoderMethod::AES256_SHA256.id());
        let method_str = if methods_buf.is_empty() {
            None
        } else {
            Some(
                methods_buf
                    .iter()
                    .map(|m| m.name())
                    .collect::<Vec<_>>()
                    .join("+"),
            )
        };

        // Compute packed size if the file maps to a block:
        // - For a 1-file block (non-solid), the block's pack size is exact.
        // - For a solid block, apportion pack size proportionally to uncompressed size.
        let packed = if entry.is_directory() || entry.size() == 0 {
            0
        } else if let Some(Some(block_idx)) = raw.stream_map.file_block_index.get(file_idx) {
            let first_idx = raw.stream_map.block_first_pack_stream_index().get(*block_idx).copied();
            if let (Some(block), Some(first_pack)) = (
                raw.blocks.get(*block_idx),
                first_idx,
            ) {
                let pack_sizes = raw.pack_sizes();
                let next_pack = raw
                    .stream_map
                    .block_first_pack_stream_index()
                    .get(block_idx + 1)
                    .copied()
                    .unwrap_or(pack_sizes.len());
                let block_pack: u64 = if first_pack <= next_pack && first_pack < pack_sizes.len() {
                    pack_sizes[first_pack..next_pack.min(pack_sizes.len())]
                        .iter()
                        .sum()
                } else {
                    0
                };
                let files_in_block = raw
                    .stream_map
                    .file_block_index
                    .iter()
                    .filter(|b| **b == Some(*block_idx))
                    .count();
                let block_unpack = block.get_unpack_size();
                if files_in_block <= 1 {
                    block_pack
                } else if block_unpack > 0 {
                    ((entry.size() as u128 * block_pack as u128) / block_unpack as u128) as u64
                } else {
                    0
                }
            } else {
                0
            }
        } else {
            0
        };

        out.push(Entry {
            path: entry.name().replace('\\', "/"),
            size: entry.size(),
            packed,
            is_dir: entry.is_directory(),
            modified: {
                let d = entry.last_modified_date();
                if d == sevenz_rust2::NtTime::NT_TIME_EPOCH || u64::from(d) == 0 {
                    None
                } else {
                    let s = format_nt(u64::from(d));
                    if s.is_empty() { None } else { Some(s) }
                }
            },
            crc: entry.has_crc.then_some(entry.crc as u32),
            method: method_str,
            encrypted,
        });
    }

    Ok(out)
}

fn list_zip(path: &Path) -> Result<Vec<Entry>> {
    let file = File::open(path)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| Error::Other(e.to_string()))?;
    let mut out = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let f = zip.by_index_raw(i).map_err(|e| Error::Other(e.to_string()))?;
        let raw_name = f.name().map(|c| c.to_string()).unwrap_or_default();
        let clean_name = raw_name.replace('\\', "/");
        let is_dir = f.is_dir() || clean_name.ends_with('/');
        out.push(Entry {
            path: clean_name.trim_end_matches('/').to_string(),
            size: f.size(),
            packed: f.compressed_size(),
            is_dir,
            modified: f.last_modified().map(|d| zip_time(&d)),
            crc: Some(f.crc32()),
            method: Some(
                match f.compression() {
                    zip::CompressionMethod::Stored => "Store",
                    zip::CompressionMethod::Deflated => "Deflate",
                    zip::CompressionMethod::Bzip2 => "BZip2",
                    zip::CompressionMethod::Zstd => "ZSTD",
                    zip::CompressionMethod::Lzma => "LZMA",
                    _ => "?",
                }
                .to_string(),
            ),
            encrypted: f.encrypted(),
        });
    }
    Ok(out)
}

fn list_tar(
    path: &Path,
    progress: Option<ProgressSink<'_>>,
) -> Result<(Vec<Entry>, Option<String>)> {
    let mut entries = Vec::new();
    let mut sink = |e: Entry| {
        entries.push(e);
        Ok(())
    };
    let stopped = visit_tar(path, progress, &mut sink)?;
    Ok((entries, stopped))
}

/// Walk a tar (optionally compressed) archive.
///
/// Best effort by design: when the stream ends early — an interrupted download,
/// a damaged block — the entries read so far are still handed to `sink`, and the
/// reason the walk stopped is returned instead of being thrown away. Callers
/// turn that into a warning so the user can extract what is actually there.
fn visit_tar<F>(
    path: &Path,
    progress: Option<ProgressSink<'_>>,
    sink: &mut F,
) -> Result<Option<String>>
where
    F: FnMut(Entry) -> Result<()>,
{
    let fmt = detect(path);
    let (reader, ctx) = stream_reader(path, fmt, progress)?;

    let method_label = match fmt {
        Format::TarGz => "TAR+GZip",
        Format::TarXz => "TAR+XZ",
        Format::TarBz2 => "TAR+BZip2",
        Format::TarZst => "TAR+ZSTD",
        _ => "TAR",
    };

    let mut ar = tar::Archive::new(reader);
    let iter = match ar.entries() {
        Ok(it) => it,
        Err(e) => return Ok(Some(e.to_string())),
    };
    for item in iter {
        let item = match item {
            Ok(i) => i,
            Err(e) => return Ok(Some(e.to_string())),
        };
        let hdr = item.header();
        let is_dir = hdr.entry_type().is_dir();
        let size = hdr.size().unwrap_or(0);
        let mtime = hdr.mtime().ok().map(format_epoch);
        let p = item
            .path()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default()
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_string();
        if p.is_empty() {
            continue;
        }
        ctx.borrow_mut().entries += 1;
        sink(Entry {
            path: p,
            size,
            packed: 0,
            is_dir,
            modified: mtime,
            crc: None,
            method: Some(method_label.to_string()),
            encrypted: false,
        })?;
    }
    Ok(None)
}

fn format_nt(nt: u64) -> String {
    // NT epoch: 100-ns ticks since 1601-01-01.
    const EPOCH_DIFF: u64 = 11_644_473_600;
    let secs = nt / 10_000_000;
    if secs < EPOCH_DIFF {
        return String::new();
    }
    format_epoch(secs - EPOCH_DIFF)
}

pub fn format_epoch(secs: u64) -> String {
    // Minimal civil-date conversion; avoids pulling in chrono.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, mi) = (rem / 3600, (rem % 3600) / 60);

    let mut y = 1970i64;
    let mut d = days;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if d < len {
            break;
        }
        d -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mlen = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    while m < 12 && d >= mlen[m] {
        d -= mlen[m];
        m += 1;
    }
    format!("{y:04}-{:02}-{:02} {:02}:{:02}", m + 1, d + 1, h, mi)
}

fn zip_time(d: &zip::DateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        d.year(),
        d.month(),
        d.day(),
        d.hour(),
        d.minute()
    )
}

/// Sanitize an archive-relative path so it cannot escape `dest` via `..` or root prefixes.
pub fn sanitize_rel_path(rel: &str) -> Option<PathBuf> {
    let normalized = rel.replace('\\', "/");
    let mut out = PathBuf::new();
    for comp in Path::new(&normalized).components() {
        if let Component::Normal(c) = comp {
            out.push(c);
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Extract a single entry, or every entry under a directory prefix.
#[allow(dead_code)]
pub fn extract_entry(archive: &Path, entry_path: &str, dest: &Path) -> Result<ExtractReport> {
    extract_entries_with_password(archive, &[entry_path.to_string()], "", dest, None)
}

/// Extract one or more entries (files or directory prefixes) in a single streaming pass.
///
/// If `strip_prefix` is non-empty (e.g. `"subdir/"`), extracted paths are written
/// relative to that prefix when possible.
pub fn extract_entries_with_password(
    archive: &Path,
    targets: &[String],
    strip_prefix: &str,
    dest: &Path,
    password: Option<&str>,
) -> Result<ExtractReport> {
    extract_entries_with_progress(archive, targets, strip_prefix, dest, password, None)
}

/// [`extract_entries_with_password`] with progress reporting. Stream formats are
/// best effort here too: a truncated source keeps everything decoded so far.
pub fn extract_entries_with_progress(
    archive: &Path,
    targets: &[String],
    strip_prefix: &str,
    dest: &Path,
    password: Option<&str>,
    progress: Option<ProgressSink<'_>>,
) -> Result<ExtractReport> {
    std::fs::create_dir_all(dest)?;
    let all = list_with_password(archive, password)?;

    let mut wanted = HashSet::new();
    for t in targets {
        let want = t.trim_end_matches('/');
        let prefix = format!("{want}/");
        for e in &all {
            let p = e.path.trim_end_matches('/');
            if !e.is_dir && (p == want || p.starts_with(&prefix)) {
                wanted.insert(p.to_string());
            }
        }
    }

    if wanted.is_empty() {
        return Err(Error::Other("no matching files to extract".to_string()));
    }

    let map_out_path = |entry_path: &str| -> Option<PathBuf> {
        let rel = entry_path
            .strip_prefix(strip_prefix)
            .filter(|s| !s.is_empty())
            .unwrap_or(entry_path);
        sanitize_rel_path(rel).map(|clean| dest.join(clean))
    };

    let fmt = detect(archive);
    match fmt {
        Format::SevenZ => {
            let file = File::open(archive)?;
            let has_pwd = password.is_some_and(|p| !p.is_empty());
            let pwd = make_7z_password(password);
            let mut ar = sevenz_rust2::ArchiveReader::new(BufReader::new(file), pwd)
                .map_err(|e| map_7z_error(e, has_pwd))?;
            let mut extracted = 0usize;
            let total_wanted = wanted.len();
            ar.for_each_entries(|entry, reader| {
                let norm = entry.name().replace('\\', "/");
                let norm = norm.trim_end_matches('/');
                if !entry.is_directory() && wanted.contains(norm) {
                    if let Some(out) = map_out_path(norm) {
                        if let Some(parent) = out.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let mut w = BufWriter::new(File::create(&out)?);
                        io::copy(reader, &mut w)?;
                        w.flush()?;
                        extracted += 1;
                    }
                } else if !entry.is_directory() {
                    // Drain solid stream to keep decoder state valid for subsequent entries.
                    io::copy(reader, &mut io::sink())?;
                }
                Ok(extracted < total_wanted)
            })
            .map_err(|e| map_7z_error(e, has_pwd))?;
            Ok(ExtractReport {
                entries: extracted,
                ..Default::default()
            })
        }
        Format::Zip => {
            let file = File::open(archive)?;
            let mut zip = zip::ZipArchive::new(BufReader::new(file))
                .map_err(|e| Error::Other(e.to_string()))?;
            let mut extracted = 0usize;
            for i in 0..zip.len() {
                let mut f = zip.by_index(i).map_err(|e| Error::Other(e.to_string()))?;
                let norm = f
                    .name()
                    .map(|n| n.replace('\\', "/"))
                    .unwrap_or_default();
                let norm = norm.trim_end_matches('/');
                if !f.is_dir() && wanted.contains(norm) {
                    if let Some(out) = map_out_path(norm) {
                        if let Some(parent) = out.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let mut w = BufWriter::new(File::create(&out)?);
                        io::copy(&mut f, &mut w)?;
                        w.flush()?;
                        extracted += 1;
                    }
                }
            }
            Ok(ExtractReport {
                entries: extracted,
                ..Default::default()
            })
        }
        // Stream formats: one shared best-effort pass (filtered to the wanted names).
        Format::Gz | Format::Xz | Format::Bz2 | Format::Zst => {
            extract_stream_core(archive, dest, Some(&wanted), strip_prefix, progress)
        }
        Format::Rar => {
            let mut extracted = 0usize;
            let mut cursor = unrar::Archive::new(archive)
                .open_for_processing()
                .map_err(|e| Error::Other(format!("rar: {e}")))?;
            loop {
                match cursor.read_header() {
                    Ok(Some(header)) => {
                        let name = header
                            .entry()
                            .filename
                            .to_string_lossy()
                            .replace('\\', "/");
                        let norm = name.trim_end_matches('/');
                        if !header.entry().is_directory() && wanted.contains(norm) {
                            let (data, next) = header
                                .read()
                                .map_err(|e| Error::Other(format!("rar: {e}")))?;
                            if let Some(out) = map_out_path(norm) {
                                if let Some(parent) = out.parent() {
                                    std::fs::create_dir_all(parent)?;
                                }
                                std::fs::write(&out, data)?;
                                extracted += 1;
                            }
                            cursor = next;
                        } else {
                            cursor = header
                                .skip()
                                .map_err(|e| Error::Other(format!("rar: {e}")))?;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => return Err(Error::Other(format!("rar: {e}"))),
                }
            }
            Ok(ExtractReport {
                entries: extracted,
                ..Default::default()
            })
        }
        Format::Unknown => Err(Error::Other("unrecognised archive format".to_string())),
        _ => extract_stream_core(archive, dest, Some(&wanted), strip_prefix, progress),
    }
}

/// Extract everything in `archive` into `dest`.
// Convenience wrapper for callers that do not need progress or warnings.
#[allow(dead_code)]
pub fn extract(archive: &Path, dest: &Path) -> Result<ExtractReport> {
    extract_with_password(archive, dest, None)
}

/// Extract everything in `archive` into `dest` using an optional password.
// Convenience wrapper for callers that do not need progress or warnings.
#[allow(dead_code)]
pub fn extract_with_password(
    archive: &Path,
    dest: &Path,
    password: Option<&str>,
) -> Result<ExtractReport> {
    extract_with_progress(archive, dest, password, None)
}

/// Extract everything, reporting progress and degrading gracefully on damaged
/// streams. Stream formats (`.tar.gz`, `.gz`, `.xz`, …) extract as much as the
/// bytes on disk allow; whatever was written stays on disk.
pub fn extract_with_progress(
    archive: &Path,
    dest: &Path,
    password: Option<&str>,
    progress: Option<ProgressSink<'_>>,
) -> Result<ExtractReport> {
    std::fs::create_dir_all(dest)?;
    let fmt = detect(archive);
    match fmt {
        Format::SevenZ => {
            let n = extract_7z(archive, dest, password, progress)?;
            Ok(ExtractReport {
                entries: n,
                ..Default::default()
            })
        }
        Format::Zip => {
            let n = extract_zip(archive, dest, progress)?;
            Ok(ExtractReport {
                entries: n,
                ..Default::default()
            })
        }
        Format::Rar => {
            let n = extract_rar(archive, dest, progress)?;
            Ok(ExtractReport {
                entries: n,
                ..Default::default()
            })
        }
        Format::Unknown => Err(Error::Other("unrecognised archive format".to_string())),
        _ => extract_stream_core(archive, dest, None, "", progress),
    }
}

fn extract_7z(
    archive: &Path,
    dest: &Path,
    password: Option<&str>,
    progress: Option<ProgressSink<'_>>,
) -> Result<usize> {
    let file = File::open(archive)?;
    let has_pwd = password.is_some_and(|p| !p.is_empty());
    let pwd = make_7z_password(password);
    let mut ar = sevenz_rust2::ArchiveReader::new(BufReader::new(file), pwd)
        .map_err(|e| map_7z_error(e, has_pwd))?;
    let mut count = 0usize;
    ar.for_each_entries(|entry, reader| {
        let Some(rel) = sanitize_rel_path(entry.name()) else {
            return Ok(true);
        };
        let out = dest.join(rel);
        if entry.is_directory() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut w = BufWriter::new(File::create(&out)?);
            io::copy(reader, &mut w)?;
            w.flush()?;
            count += 1;
            if let Some(sink) = progress {
                sink(&Progress {
                    entries: count,
                    label: entry.name().to_string(),
                    ..Default::default()
                });
            }
        }
        Ok(true)
    })
    .map_err(|e| map_7z_error(e, has_pwd))?;
    Ok(count)
}

fn extract_zip(
    archive: &Path,
    dest: &Path,
    progress: Option<ProgressSink<'_>>,
) -> Result<usize> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| Error::Other(e.to_string()))?;
    let total = zip.len();
    let mut count = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| Error::Other(e.to_string()))?;
        let Some(rel) = f.enclosed_name() else {
            continue;
        };
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut w = BufWriter::new(File::create(&out)?);
        io::copy(&mut f, &mut w)?;
        w.flush()?;
        count += 1;
        if let Some(sink) = progress {
            sink(&Progress {
                entries: count,
                entries_total: total,
                label: f
                    .enclosed_name()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
                ..Default::default()
            });
        }
    }
    Ok(count)
}

/// Progress of a streaming pass.
///
/// Two ways to measure it, because not every format knows both:
/// * `total > 0` — bytes of the *source file on disk*; works for every stream
///   format even when the uncompressed size is unknown (or the file is truncated).
/// * `entries_total > 0` — entry counts, for archives with a central index
///   (zip knows its length up front, 7z and rar do not).
#[derive(Debug, Clone, Default)]
pub struct Progress {
    /// Compressed source bytes consumed so far.
    pub consumed: u64,
    /// Bytes written to disk so far (uncompressed). The source can be read to
    /// the end well before decompression is finished.
    pub written: u64,
    /// Size of the source file on disk (0 when unknown).
    pub total: u64,
    /// Entries finished so far.
    pub entries: usize,
    /// Entries the archive declares, when it declares them (0 = unknown).
    pub entries_total: usize,
    /// Name currently being written.
    pub label: String,
}

/// Callback invoked while work is in flight. Kept `Fn` so it can be shared.
pub type ProgressSink<'a> = &'a dyn Fn(&Progress);

/// What an extraction actually managed to do.
///
/// `stopped` is set when the source ended (or failed) before the archive did.
/// In that case nothing is deleted: partially written files are listed in
/// `incomplete`, and a copy of the damaged archive is left next to them so the
/// user can see exactly which file was bad.
#[derive(Debug, Default, Clone)]
pub struct ExtractReport {
    pub entries: usize,
    pub bytes: u64,
    pub incomplete: Vec<String>,
    pub corrupt_copy: Option<PathBuf>,
    pub stopped: Option<String>,
}

impl ExtractReport {
    pub fn is_partial(&self) -> bool {
        self.stopped.is_some() || !self.incomplete.is_empty()
    }

    /// One line for the status bar: what happened, what was kept, why it stopped.
    pub fn summary(&self) -> String {
        let plural = |n: usize| if n == 1 { "file" } else { "files" };
        let kept = self.incomplete.len();

        let mut s = match (self.entries, kept) {
            (0, 0) => "Nothing extracted".to_string(),
            (0, _) => format!(
                "No complete files — {kept} partial {} kept on disk ({})",
                plural(kept),
                human(self.bytes)
            ),
            (n, 0) if self.bytes > 0 => {
                format!("Extracted {n} {} ({})", plural(n), human(self.bytes))
            }
            (n, 0) => format!("Extracted {n} {}", plural(n)),
            (n, _) => format!(
                "Extracted {n} complete {} ({}) — {kept} partial kept on disk: {}",
                plural(n),
                human(self.bytes),
                self.incomplete.join(", ")
            ),
        };
        if let Some(reason) = &self.stopped {
            s.push_str(&format!(" — stopped early: {reason}"));
        }
        if let Some(copy) = &self.corrupt_copy {
            s.push_str(&format!(" — damaged copy: {}", copy.display()));
        }
        s
    }
}

/// Bytes consumed so far, plus what the readers know about the current entry.
#[derive(Default)]
struct ProgCtx {
    consumed: u64,
    written: u64,
    total: u64,
    entries: usize,
    entries_total: usize,
    label: String,
}

/// Reader wrapper that counts the bytes pulled from disk and reports progress.
/// Counting happens at the file level so the number is meaningful even when the
/// decoder buffers far ahead, and it is throttled so the UI is not flooded.
struct Tap<'a, R> {
    inner: R,
    ctx: Rc<RefCell<ProgCtx>>,
    sink: Option<ProgressSink<'a>>,
    last: Instant,
}

impl<R: Read> Read for Tap<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            if let Some(sink) = self.sink {
                let report = {
                    let mut ctx = self.ctx.borrow_mut();
                    ctx.consumed += n as u64;
                    if self.last.elapsed() >= Duration::from_millis(70) {
                        self.last = Instant::now();
                        Some(Progress {
                            consumed: ctx.consumed,
                            written: ctx.written,
                            total: ctx.total,
                            entries: ctx.entries,
                            entries_total: ctx.entries_total,
                            label: ctx.label.clone(),
                        })
                    } else {
                        None
                    }
                };
                if let Some(p) = report {
                    sink(&p);
                }
            } else {
                self.ctx.borrow_mut().consumed += n as u64;
            }
        }
        Ok(n)
    }
}

/// Build the decoding reader for a stream format, wired to a progress context.
fn stream_reader<'a>(
    path: &Path,
    fmt: Format,
    sink: Option<ProgressSink<'a>>,
) -> Result<(Box<dyn Read + 'a>, Rc<RefCell<ProgCtx>>)> {
    let total = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let ctx = Rc::new(RefCell::new(ProgCtx {
        total,
        ..Default::default()
    }));
    let tap = Tap {
        inner: File::open(path)?,
        ctx: Rc::clone(&ctx),
        sink,
        last: Instant::now(),
    };
    let buf = BufReader::with_capacity(256 * 1024, tap);
    let reader: Box<dyn Read + 'a> = match fmt {
        Format::Gz | Format::TarGz => Box::new(flate2::read::GzDecoder::new(buf)),
        Format::Xz | Format::TarXz => Box::new(xz2::read::XzDecoder::new(buf)),
        Format::Bz2 | Format::TarBz2 => Box::new(bzip2::read::BzDecoder::new(buf)),
        Format::Zst | Format::TarZst => Box::new(
            zstd::stream::read::Decoder::new(buf).map_err(|e| Error::Other(e.to_string()))?,
        ),
        _ => Box::new(buf),
    };
    Ok((reader, ctx))
}

/// Copy `r` into `w` in chunks, returning the bytes written and any error.
///
/// Unlike `io::copy` this keeps the byte count on failure, which is what lets a
/// half-written file be reported and kept instead of vanishing.
fn copy_stream<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    ctx: Option<&RefCell<ProgCtx>>,
    sink: Option<ProgressSink<'_>>,
) -> (u64, Option<io::Error>) {
    let mut buf = vec![0u8; 256 * 1024];
    let mut written = 0u64;
    let mut last = Instant::now();
    loop {
        match r.read(&mut buf) {
            Ok(0) => return (written, None),
            Ok(n) => {
                if let Err(e) = w.write_all(&buf[..n]) {
                    return (written, Some(e));
                }
                written += n as u64;
                // Tick from the write side too: a 300 MB tar of zeros is 300 KB
                // on disk, so the source is exhausted almost immediately while
                // the decompressor keeps working for seconds.
                let tick = match (ctx, sink) {
                    (Some(ctx), Some(_)) => {
                        let mut c = ctx.borrow_mut();
                        c.written += n as u64;
                        if last.elapsed() >= Duration::from_millis(90) {
                            last = Instant::now();
                            Some(Progress {
                                consumed: c.consumed,
                                written: c.written,
                                total: c.total,
                                entries: c.entries,
                                entries_total: c.entries_total,
                                label: c.label.clone(),
                            })
                        } else {
                            None
                        }
                    }
                    (Some(ctx), None) => {
                        ctx.borrow_mut().written += n as u64;
                        None
                    }
                    _ => None,
                };
                if let (Some(p), Some(sink)) = (tick, sink) {
                    sink(&p);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return (written, Some(e)),
        }
    }
}

fn is_stream_tar(fmt: Format) -> bool {
    matches!(
        fmt,
        Format::Tar | Format::TarGz | Format::TarXz | Format::TarBz2 | Format::TarZst
    )
}

/// The heart of best-effort extraction: decode, write, keep going.
///
/// Nothing is buffered whole and nothing is rolled back. A file is created and
/// written while the stream is decoded, so an interrupted or truncated source
/// leaves every complete file in place plus whatever fragment it managed to
/// write — and a copy of the damaged archive beside them.
fn extract_stream_core(
    archive: &Path,
    dest: &Path,
    filter: Option<&HashSet<String>>,
    strip_prefix: &str,
    progress: Option<ProgressSink<'_>>,
) -> Result<ExtractReport> {
    let fmt = detect(archive);
    std::fs::create_dir_all(dest)?;
    let (reader, ctx) = stream_reader(archive, fmt, progress)?;
    let mut report = ExtractReport::default();

    if is_stream_tar(fmt) {
        let mut ar = tar::Archive::new(reader);
        let iter = match ar.entries() {
            Ok(it) => it,
            Err(e) => {
                report.stopped = Some(e.to_string());
                return Ok(report_finish(archive, dest, report));
            }
        };
        for item in iter {
            let mut item = match item {
                Ok(i) => i,
                Err(e) => {
                    report.stopped = Some(e.to_string());
                    break;
                }
            };
            let raw = item
                .path()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let name = raw.trim_start_matches("./").trim_end_matches('/').to_string();
            if name.is_empty() || name.starts_with('/') {
                continue;
            }
            let is_dir = item.header().entry_type().is_dir();
            if let Some(want) = filter {
                if !want.contains(&name) {
                    continue;
                }
            }
            let rel_name = name
                .strip_prefix(strip_prefix)
                .filter(|s| !s.is_empty())
                .unwrap_or(&name);
            let Some(rel) = sanitize_rel_path(rel_name) else {
                continue;
            };
            let out = dest.join(rel);
            if is_dir {
                let _ = std::fs::create_dir_all(&out);
                continue;
            }
            if let Some(parent) = out.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            ctx.borrow_mut().label = name.clone();
            let mut w = match File::create(&out) {
                Ok(f) => BufWriter::new(f),
                Err(e) => {
                    report.stopped = Some(format!("{}: {e}", out.display()));
                    break;
                }
            };
            let (written, err) = copy_stream(&mut item, &mut w, Some(&ctx), progress);
            let flush_err = w.flush().err();
            report.bytes += written;
            match err.or(flush_err) {
                None => {
                    report.entries += 1;
                    let mut c = ctx.borrow_mut();
                    c.entries += 1;
                    let label = c.label.clone();
                    let p = Progress {
                        consumed: c.consumed,
                        written: c.written,
                        total: c.total,
                        entries: c.entries,
                        entries_total: c.entries_total,
                        label: label.clone(),
                    };
                    drop(c);
                    if let Some(sink) = progress {
                        sink(&p);
                    }
                }
                Some(e) => {
                    // Keep the fragment on disk: the user may still recover data
                    // from it, and knowing which file broke is the point.
                    report.incomplete.push(name.clone());
                    report.stopped = Some(e.to_string());
                    break;
                }
            }
        }
    } else {
        let name = single_inner_name(archive, fmt);
        let wanted = filter.map(|f| f.contains(&name)).unwrap_or(true);
        let mut reader = reader;
        if wanted {
            let rel_name = name
                .strip_prefix(strip_prefix)
                .filter(|s| !s.is_empty())
                .unwrap_or(&name);
            let Some(rel) = sanitize_rel_path(rel_name) else {
                return Err(Error::Other(format!("unusable entry name: {name}")));
            };
            let out = dest.join(rel);
            ctx.borrow_mut().label = name.clone();
            let mut w = BufWriter::new(File::create(&out)?);
            let (written, err) = copy_stream(&mut reader, &mut w, Some(&ctx), progress);
            let flush_err = w.flush().err();
            report.bytes += written;
            match err.or(flush_err) {
                None => {
                    report.entries = 1;
                    ctx.borrow_mut().entries = 1;
                }
                Some(e) => {
                    report.incomplete.push(name);
                    report.stopped = Some(e.to_string());
                }
            }
        }
    }

    Ok(report_finish(archive, dest, report))
}

/// Copy the damaged source next to the extracted files so nothing about the
/// failure is lost, and return the report unchanged otherwise.
fn report_finish(archive: &Path, dest: &Path, mut report: ExtractReport) -> ExtractReport {
    if report.stopped.is_none() {
        return report;
    }
    let name = archive
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("archive");
    let same_dir = match (archive.parent(), dest.canonicalize()) {
        (Some(p), Ok(d)) => p.canonicalize().map(|c| c == d).unwrap_or(false),
        _ => false,
    };
    if same_dir {
        return report;
    }
    let copy = dest.join(format!("{name}.corrupt"));
    if std::fs::copy(archive, &copy).is_ok() {
        report.corrupt_copy = Some(copy);
    }
    report
}

/// Create an archive from a list of source paths using default options.
#[allow(dead_code)]
pub fn create(fmt: Format, sources: &[PathBuf], target: &Path) -> Result<()> {
    let opts = CreateOptions {
        format: fmt,
        ..CreateOptions::default()
    };
    create_with_options(&opts, sources, target)
}

/// Create an archive from a list of source paths with full 7-Zip options.
pub fn create_with_options(
    opts: &CreateOptions,
    sources: &[PathBuf],
    target: &Path,
) -> Result<()> {
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let fmt = opts.format;
    if !fmt.can_write() {
        return Err(Error::Other(format!(
            "cannot create {} archives (extract/list only)",
            fmt.label()
        )));
    }
    match fmt {
        Format::SevenZ => create_7z(opts, sources, target),
        Format::Zip => create_zip(opts, sources, target),
        Format::Tar | Format::TarGz | Format::TarXz | Format::TarBz2 | Format::TarZst => {
            create_tar(opts, sources, target)
        }
        Format::Gz | Format::Xz | Format::Bz2 | Format::Zst => {
            create_single(opts, sources, target)
        }
        Format::Rar | Format::Unknown => Err(Error::Other(format!(
            "cannot create {} archives",
            fmt.label()
        ))),
    }
}

fn create_7z(opts: &CreateOptions, sources: &[PathBuf], target: &Path) -> Result<()> {
    use sevenz_rust2::encoder_options::*;
    use sevenz_rust2::{EncoderConfiguration, EncoderMethod};

    let mut w =
        sevenz_rust2::ArchiveWriter::create(target).map_err(|e| Error::Other(e.to_string()))?;

    let lvl = opts.level.numeric_0_to_9();
    let codec_cfg: EncoderConfiguration = if opts.level == CompressionLevel::Store
        || opts.sevenz_method == SevenZMethod::Copy
    {
        EncoderConfiguration::new(EncoderMethod::COPY)
    } else {
        match opts.sevenz_method {
            SevenZMethod::Lzma2 => {
                let threads = std::thread::available_parallelism()
                    .map(|n| n.get() as u32)
                    .unwrap_or(1)
                    .clamp(1, 8);
                if threads > 1 {
                    Lzma2Options::from_level_mt(lvl, threads, 16 * 1024 * 1024).into()
                } else {
                    Lzma2Options::from_level(lvl).into()
                }
            }
            SevenZMethod::Lzma => LzmaOptions::from_level(lvl).into(),
            SevenZMethod::Zstd => {
                ZstandardOptions::from_level(opts.level.zstd_level() as u32).into()
            }
            SevenZMethod::Bzip2 => Bzip2Options::from_level(lvl.clamp(1, 9)).into(),
            SevenZMethod::Deflate => DeflateOptions::from_level(lvl.clamp(1, 9)).into(),
            SevenZMethod::Brotli => BrotliOptions::from_quality_window(lvl.clamp(1, 11), 22).into(),
            SevenZMethod::Lz4 => Lz4Options::default().into(),
            SevenZMethod::Copy => EncoderConfiguration::new(EncoderMethod::COPY),
        }
    };

    let mut methods = Vec::new();
    if let Some(pwd) = opts.password.as_deref().filter(|s| !s.is_empty()) {
        methods.push(AesEncoderOptions::new(sevenz_rust2::Password::from(pwd)).into());
        w.set_encrypt_header(opts.encrypt_header);
    } else {
        w.set_encrypt_header(false);
    }
    methods.push(codec_cfg);
    w.set_content_methods(methods);

    if opts.solid && opts.level != CompressionLevel::Store && opts.sevenz_method != SevenZMethod::Copy
    {
        let mut dir_entries = Vec::new();
        let mut file_entries = Vec::new();
        let mut file_readers = Vec::new();
        for src in sources {
            let base = src.parent().unwrap_or(Path::new(""));
            collect_7z_solid(src, base, &mut dir_entries, &mut file_entries, &mut file_readers)?;
        }
        for d in dir_entries {
            w.push_archive_entry(d, None::<File>)
                .map_err(|e| Error::Other(e.to_string()))?;
        }
        if !file_entries.is_empty() {
            w.push_archive_entries(file_entries, file_readers)
                .map_err(|e| Error::Other(e.to_string()))?;
        }
    } else {
        for src in sources {
            push_7z(&mut w, src, src)?;
        }
    }

    w.finish().map_err(|e| Error::Other(e.to_string()))?;
    Ok(())
}

fn collect_7z_solid(
    src: &Path,
    base: &Path,
    dirs: &mut Vec<sevenz_rust2::ArchiveEntry>,
    files: &mut Vec<sevenz_rust2::ArchiveEntry>,
    readers: &mut Vec<sevenz_rust2::SourceReader<BufReader<File>>>,
) -> Result<()> {
    let rel = src.strip_prefix(base).unwrap_or(src);
    let name = rel.to_string_lossy().replace('\\', "/");
    if src.is_dir() {
        dirs.push(sevenz_rust2::ArchiveEntry::from_path(src, name));
        let mut children: Vec<PathBuf> = std::fs::read_dir(src)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        children.sort();
        for child in children {
            collect_7z_solid(&child, base, dirs, files, readers)?;
        }
    } else {
        let f = File::open(src)?;
        files.push(sevenz_rust2::ArchiveEntry::from_path(src, name));
        readers.push(sevenz_rust2::SourceReader::new(BufReader::new(f)));
    }
    Ok(())
}

fn push_7z<W: Write + std::io::Seek>(
    w: &mut sevenz_rust2::ArchiveWriter<W>,
    src: &Path,
    base: &Path,
) -> Result<()> {
    let rel = src
        .strip_prefix(base.parent().unwrap_or(Path::new("")))
        .unwrap_or(src);
    let name = rel.to_string_lossy().replace('\\', "/");
    if src.is_dir() {
        w.push_archive_entry(
            sevenz_rust2::ArchiveEntry::from_path(src, name),
            None::<File>,
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        let mut children: Vec<PathBuf> = std::fs::read_dir(src)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        children.sort();
        for child in children {
            push_7z(w, &child, base)?;
        }
    } else {
        let f = File::open(src)?;
        w.push_archive_entry(
            sevenz_rust2::ArchiveEntry::from_path(src, name),
            Some(BufReader::new(f)),
        )
        .map_err(|e| Error::Other(e.to_string()))?;
    }
    Ok(())
}

fn create_zip(opts: &CreateOptions, sources: &[PathBuf], target: &Path) -> Result<()> {
    let file = File::create(target)?;
    let mut zw = zip::ZipWriter::new(BufWriter::new(file));
    let method = if opts.level == CompressionLevel::Store || opts.zip_method == ZipMethod::Store {
        zip::CompressionMethod::Stored
    } else {
        match opts.zip_method {
            ZipMethod::Deflate => zip::CompressionMethod::Deflated,
            ZipMethod::Zstd => zip::CompressionMethod::Zstd,
            ZipMethod::Bzip2 => zip::CompressionMethod::Bzip2,
            ZipMethod::Lzma => zip::CompressionMethod::Lzma,
            ZipMethod::Store => zip::CompressionMethod::Stored,
        }
    };
    let zip_opts = zip::write::SimpleFileOptions::default().compression_method(method);
    for src in sources {
        add_zip(
            &mut zw,
            src,
            zip_opts,
            src.parent().unwrap_or(Path::new("")),
        )?;
    }
    zw.finish().map_err(|e| Error::Other(e.to_string()))?;
    Ok(())
}

fn add_zip<W: Write + io::Seek>(
    zw: &mut zip::ZipWriter<W>,
    src: &Path,
    opts: zip::write::SimpleFileOptions,
    base: &Path,
) -> Result<()> {
    let name = src.strip_prefix(base).unwrap_or(src);
    let name = name.to_string_lossy().replace('\\', "/");
    if src.is_dir() {
        zw.add_directory(format!("{name}/"), opts)
            .map_err(|e| Error::Other(e.to_string()))?;
        let mut children: Vec<PathBuf> = std::fs::read_dir(src)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        children.sort();
        for child in children {
            add_zip(zw, &child, opts, base)?;
        }
    } else {
        zw.start_file(name, opts)
            .map_err(|e| Error::Other(e.to_string()))?;
        let mut f = File::open(src)?;
        io::copy(&mut f, zw)?;
    }
    Ok(())
}

fn create_tar(opts: &CreateOptions, sources: &[PathBuf], target: &Path) -> Result<()> {
    let fmt = opts.format;
    let lvl = opts.level.numeric_0_to_9();
    let file = File::create(target)?;
    let buf = BufWriter::new(file);
    let writer: Box<dyn Write> = match fmt {
        Format::TarGz => Box::new(flate2::write::GzEncoder::new(
            buf,
            flate2::Compression::new(lvl),
        )),
        Format::TarXz => Box::new(xz2::write::XzEncoder::new(buf, lvl)),
        Format::TarBz2 => Box::new(bzip2::write::BzEncoder::new(
            buf,
            bzip2::Compression::new(lvl.clamp(1, 9)),
        )),
        Format::TarZst => Box::new(
            zstd::stream::write::Encoder::new(buf, opts.level.zstd_level())
                .map_err(|e| Error::Other(e.to_string()))?
                .auto_finish(),
        ),
        _ => Box::new(buf),
    };
    let mut tb = tar::Builder::new(writer);
    for src in sources {
        let base = src.parent().unwrap_or(Path::new(""));
        if src.is_dir() {
            tb.append_dir_all(src.file_name().unwrap_or_default(), src)
                .map_err(|e| Error::Other(e.to_string()))?;
        } else {
            tb.append_path_with_name(src, src.strip_prefix(base).unwrap_or(src))
                .map_err(|e| Error::Other(e.to_string()))?;
        }
    }
    tb.finish().map_err(|e| Error::Other(e.to_string()))?;
    Ok(())
}

fn create_single(opts: &CreateOptions, sources: &[PathBuf], target: &Path) -> Result<()> {
    if sources.len() != 1 || sources[0].is_dir() {
        return Err(Error::Other(format!(
            "{} only compresses a single file (use tar.{} for folders or multiple files)",
            opts.format.label(),
            opts.format.label()
        )));
    }
    let src = &sources[0];
    let mut input = BufReader::new(File::open(src)?);
    let out = BufWriter::new(File::create(target)?);
    let lvl = opts.level.numeric_0_to_9();

    match opts.format {
        Format::Gz => {
            let mut enc = flate2::write::GzEncoder::new(out, flate2::Compression::new(lvl));
            io::copy(&mut input, &mut enc)?;
            enc.finish()?;
        }
        Format::Xz => {
            let mut enc = xz2::write::XzEncoder::new(out, lvl);
            io::copy(&mut input, &mut enc)?;
            enc.finish()?;
        }
        Format::Bz2 => {
            let mut enc =
                bzip2::write::BzEncoder::new(out, bzip2::Compression::new(lvl.clamp(1, 9)));
            io::copy(&mut input, &mut enc)?;
            enc.finish()?;
        }
        Format::Zst => {
            let mut enc = zstd::stream::write::Encoder::new(out, opts.level.zstd_level())
                .map_err(|e| Error::Other(e.to_string()))?;
            io::copy(&mut input, &mut enc)?;
            enc.finish()?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

pub fn human(n: u64) -> String {
    const U: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut d = n as f64;
    let mut i = 0;
    while d >= 1024.0 && i < 5 {
        d /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{d:.1} {}", U[i])
    }
}

/// Size of the backing file on disk.
pub fn container_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Read one entry fully into memory. Powers the built-in viewer, so opening a
/// file inside an archive never shells out to another program.
#[allow(dead_code)]
pub fn read_entry(archive: &Path, entry_path: &str) -> Result<Vec<u8>> {
    read_entry_with_password(archive, entry_path, None)
}

/// Read one entry fully into memory, supplying an optional password.
pub fn read_entry_with_password(
    archive: &Path,
    entry_path: &str,
    password: Option<&str>,
) -> Result<Vec<u8>> {
    let fmt = detect(archive);
    match fmt {
        Format::SevenZ => read_entry_7z(archive, entry_path, password),
        Format::Zip => read_entry_zip(archive, entry_path),
        Format::Rar => read_entry_rar(archive, entry_path),
        Format::Gz | Format::Xz | Format::Bz2 | Format::Zst => read_entry_single(archive, fmt),
        Format::Unknown => Err(Error::Other("unrecognised archive format".to_string())),
        _ => read_entry_tar(archive, entry_path),
    }
}

fn read_entry_single(archive: &Path, fmt: Format) -> Result<Vec<u8>> {
    let file = File::open(archive)?;
    let buf = BufReader::new(file);
    let mut reader: Box<dyn Read> = match fmt {
        Format::Gz => Box::new(flate2::read::GzDecoder::new(buf)),
        Format::Xz => Box::new(xz2::read::XzDecoder::new(buf)),
        Format::Bz2 => Box::new(bzip2::read::BzDecoder::new(buf)),
        Format::Zst => Box::new(
            zstd::stream::read::Decoder::new(buf).map_err(|e| Error::Other(e.to_string()))?,
        ),
        _ => return Err(Error::Other("unsupported single-stream format".to_string())),
    };
    let mut out = Vec::new();
    reader.read_to_end(&mut out)?;
    Ok(out)
}

fn extract_rar(
    archive: &Path,
    dest: &Path,
    progress: Option<ProgressSink<'_>>,
) -> Result<usize> {
    let mut count = 0usize;
    let open = unrar::Archive::new(archive)
        .open_for_processing()
        .map_err(|e| Error::Other(format!("rar: {e}")))?;
    let mut cursor = open;
    loop {
        match cursor.read_header() {
            Ok(Some(header)) => {
                let is_dir = header.entry().is_directory();
                let label = header.entry().filename.to_string_lossy().to_string();
                let next = header
                    .extract_with_base(dest)
                    .map_err(|e| Error::Other(format!("rar: {e}")))?;
                if !is_dir {
                    count += 1;
                    if let Some(sink) = progress {
                        sink(&Progress {
                            entries: count,
                            label,
                            ..Default::default()
                        });
                    }
                }
                cursor = next;
            }
            Ok(None) => break,
            Err(e) => return Err(Error::Other(format!("rar: {e}"))),
        }
    }
    Ok(count)
}

fn read_entry_rar(archive: &Path, want: &str) -> Result<Vec<u8>> {
    let want_norm = want.trim_end_matches('/');
    let open = unrar::Archive::new(archive)
        .open_for_processing()
        .map_err(|e| Error::Other(format!("rar: {e}")))?;
    let mut cursor = open;
    loop {
        match cursor.read_header() {
            Ok(Some(header)) => {
                let name = header
                    .entry()
                    .filename
                    .to_string_lossy()
                    .replace('\\', "/");
                if name.trim_end_matches('/') == want_norm && !header.entry().is_directory() {
                    let (data, _rest) = header
                        .read()
                        .map_err(|e| Error::Other(format!("rar: {e}")))?;
                    return Ok(data);
                }
                cursor = header
                    .skip()
                    .map_err(|e| Error::Other(format!("rar: {e}")))?;
            }
            Ok(None) => break,
            Err(e) => return Err(Error::Other(format!("rar: {e}"))),
        }
    }
    Err(Error::Other(format!("entry not found: {want}")))
}

fn read_entry_7z(archive: &Path, want: &str, password: Option<&str>) -> Result<Vec<u8>> {
    let file = File::open(archive)?;
    let has_pwd = password.is_some_and(|p| !p.is_empty());
    let pwd = make_7z_password(password);
    let mut ar = sevenz_rust2::ArchiveReader::new(BufReader::new(file), pwd)
        .map_err(|e| map_7z_error(e, has_pwd))?;
    ar.read_file(want).map_err(|e| map_7z_error(e, has_pwd))
}

fn read_entry_zip(archive: &Path, want: &str) -> Result<Vec<u8>> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| Error::Other(e.to_string()))?;
    if let Ok(mut f) = zip.by_name(want) {
        let mut buf = Vec::with_capacity(f.size().min(16 * 1024 * 1024) as usize);
        f.read_to_end(&mut buf)?;
        return Ok(buf);
    }
    let want_norm = want.replace('\\', "/");
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| Error::Other(e.to_string()))?;
        let name = f
            .name()
            .map(|n| n.replace('\\', "/"))
            .unwrap_or_default();
        if name.trim_end_matches('/') == want_norm.trim_end_matches('/') {
            let mut buf = Vec::with_capacity(f.size().min(16 * 1024 * 1024) as usize);
            f.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    Err(Error::Other(format!("entry not found: {want}")))
}

fn read_entry_tar(archive: &Path, want: &str) -> Result<Vec<u8>> {
    let want_norm = want.trim_start_matches("./").trim_end_matches('/');
    let fmt = detect(archive);
    let file = File::open(archive)?;
    let buf = BufReader::new(file);
    let reader: Box<dyn Read> = match fmt {
        Format::TarGz => Box::new(flate2::read::GzDecoder::new(buf)),
        Format::TarXz => Box::new(xz2::read::XzDecoder::new(buf)),
        Format::TarBz2 => Box::new(bzip2::read::BzDecoder::new(buf)),
        Format::TarZst => Box::new(
            zstd::stream::read::Decoder::new(buf).map_err(|e| Error::Other(e.to_string()))?,
        ),
        _ => Box::new(buf),
    };
    let mut ar = tar::Archive::new(reader);
    for item in ar.entries().map_err(|e| Error::Other(e.to_string()))? {
        let mut item = item.map_err(|e| Error::Other(e.to_string()))?;
        let p = item
            .path()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let p_norm = p.trim_start_matches("./").trim_end_matches('/');
        if p_norm == want_norm {
            let mut buf = Vec::new();
            item.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    Err(Error::Other(format!("entry not found: {want}")))
}

/// Verify every entry decodes in a single streaming pass. Returns `(checked, errors)`.
#[allow(dead_code)]
pub fn verify(archive: &Path) -> Result<(usize, Vec<String>)> {
    verify_with_password(archive, None)
}

/// Verify every entry decodes in a single streaming pass, with optional password.
pub fn verify_with_password(
    archive: &Path,
    password: Option<&str>,
) -> Result<(usize, Vec<String>)> {
    let fmt = detect(archive);
    let mut checked = 0usize;
    let mut errs = Vec::new();

    match fmt {
        Format::SevenZ => {
            let file = File::open(archive)?;
            let has_pwd = password.is_some_and(|p| !p.is_empty());
            let pwd = make_7z_password(password);
            let mut ar = sevenz_rust2::ArchiveReader::new(BufReader::new(file), pwd)
                .map_err(|e| map_7z_error(e, has_pwd))?;
            ar.for_each_entries(|entry, reader| {
                if !entry.is_directory() {
                    let mut hasher = Crc32Hasher::new();
                    let mut buf = [0u8; 32 * 1024];
                    let mut n_read = 0u64;
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                hasher.update(&buf[..n]);
                                n_read += n as u64;
                            }
                            Err(e) => {
                                errs.push(format!("{}: {e}", entry.name()));
                                break;
                            }
                        }
                    }
                    if n_read != entry.size() {
                        errs.push(format!(
                            "{}: size {} != declared {}",
                            entry.name(),
                            n_read,
                            entry.size()
                        ));
                    } else if entry.has_crc {
                        let got = hasher.finalize();
                        let want = entry.crc as u32;
                        if got != want {
                            errs.push(format!(
                                "{}: CRC {got:08X} != {want:08X}",
                                entry.name()
                            ));
                        }
                    }
                    checked += 1;
                }
                Ok(true)
            })
            .map_err(|e| map_7z_error(e, has_pwd))?;
        }
        Format::Zip => {
            let file = File::open(archive)?;
            let mut zip = zip::ZipArchive::new(BufReader::new(file))
                .map_err(|e| Error::Other(e.to_string()))?;
            for i in 0..zip.len() {
                match zip.by_index(i) {
                    Ok(mut f) => {
                        if f.is_dir() {
                            continue;
                        }
                        let name = f.name().map(|s| s.to_string()).unwrap_or_default();
                        let expected_size = f.size();
                        let expected_crc = f.crc32();
                        let mut hasher = Crc32Hasher::new();
                        let mut buf = [0u8; 32 * 1024];
                        let mut n_read = 0u64;
                        let mut read_err = None;
                        loop {
                            match f.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => {
                                    hasher.update(&buf[..n]);
                                    n_read += n as u64;
                                }
                                Err(e) => {
                                    read_err = Some(e);
                                    break;
                                }
                            }
                        }
                        if let Some(e) = read_err {
                            errs.push(format!("{name}: {e}"));
                        } else if n_read != expected_size {
                            errs.push(format!("{name}: size {n_read} != declared {expected_size}"));
                        } else {
                            let got = hasher.finalize();
                            if got != expected_crc {
                                errs.push(format!("{name}: CRC {got:08X} != {expected_crc:08X}"));
                            }
                        }
                        checked += 1;
                    }
                    Err(e) => errs.push(format!("entry #{i}: {e}")),
                }
            }
        }
        Format::Gz | Format::Xz | Format::Bz2 | Format::Zst => {
            match read_entry_single(archive, fmt) {
                Ok(_) => checked = 1,
                Err(e) => errs.push(e.to_string()),
            }
        }
        _ => {
            let entries = list_with_password(archive, password)?;
            for e in entries.iter().filter(|e| !e.is_dir) {
                match read_entry_with_password(archive, &e.path, password) {
                    Ok(data) => {
                        if e.size > 0 && data.len() as u64 != e.size {
                            errs.push(format!(
                                "{}: size {} != declared {}",
                                e.path,
                                data.len(),
                                e.size
                            ));
                        } else if let Some(want) = e.crc {
                            let got = crc32(&data);
                            if got != want {
                                errs.push(format!("{}: CRC {got:08X} != {want:08X}", e.path));
                            }
                        }
                        checked += 1;
                    }
                    Err(err) => errs.push(format!("{}: {err}", e.path)),
                }
            }
        }
    }

    Ok((checked, errs))
}

/// Streaming CRC-32 (IEEE) hasher.
pub struct Crc32Hasher {
    state: u32,
}

impl Crc32Hasher {
    pub fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut crc = self.state;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        self.state = crc;
    }

    pub fn finalize(self) -> u32 {
        !self.state
    }
}

/// CRC-32 (IEEE), used by zip and 7z entry checksums.
pub fn crc32(data: &[u8]) -> u32 {
    let mut h = Crc32Hasher::new();
    h.update(data);
    h.finalize()
}

/// Pure-Rust SHA-256 implementation for 7-Zip style checksum inspection.
pub struct Sha256Hasher {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Sha256Hasher {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
                0x1f83d9ab, 0x5be0cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    fn compress_block(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(Self::K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                Self::compress_block(&mut self.state, &self.buf);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            let block: &[u8; 64] = data[..64].try_into().unwrap();
            Self::compress_block(&mut self.state, block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    pub fn finalize_hex(mut self) -> String {
        let bit_len = self.total_len.wrapping_mul(8);
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            self.buf[self.buf_len..64].fill(0);
            Self::compress_block(&mut self.state, &self.buf);
            self.buf_len = 0;
        }
        self.buf[self.buf_len..56].fill(0);
        self.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
        Self::compress_block(&mut self.state, &self.buf);

        let mut out = String::with_capacity(64);
        for word in self.state {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{word:08x}");
        }
        out
    }
}

/// Compute `(CRC32_hex, SHA256_hex)` for in-memory bytes.
#[allow(dead_code)]
pub fn hash_bytes(data: &[u8]) -> (String, String) {
    let c = crc32(data);
    let mut sha = Sha256Hasher::new();
    sha.update(data);
    (format!("{c:08X}"), sha.finalize_hex())
}

/// Compute `(CRC32_hex, SHA256_hex)` for a file on disk by streaming in 64 KB chunks.
pub fn hash_file(path: &Path) -> Result<(String, String)> {
    let mut f = BufReader::new(File::open(path)?);
    let mut crc = Crc32Hasher::new();
    let mut sha = Sha256Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        crc.update(&buf[..n]);
        sha.update(&buf[..n]);
    }
    Ok((format!("{:08X}", crc.finalize()), sha.finalize_hex()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_and_crc32_match_known_vectors() {
        let (crc, sha) = hash_bytes(b"hello world");
        assert_eq!(crc, "0D4A1185");
        assert_eq!(
            sha,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn sanitizes_traversal_paths() {
        assert_eq!(
            sanitize_rel_path("../../etc/passwd"),
            Some(PathBuf::from("etc/passwd"))
        );
        assert_eq!(
            sanitize_rel_path("/abs/path/file.txt"),
            Some(PathBuf::from("abs/path/file.txt"))
        );
    }

    #[test]
    fn roundtrip_encrypted_7z_and_single_zst() {
        let tmp = std::env::temp_dir().join(format!("choab-zip-engine-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let src_file = tmp.join("secret.txt");
        std::fs::write(&src_file, b"top secret rust 7z payload\n").unwrap();

        // 1. Encrypted 7z roundtrip
        let arc_7z = tmp.join("encrypted.7z");
        let opts = CreateOptions {
            format: Format::SevenZ,
            level: CompressionLevel::Normal,
            sevenz_method: SevenZMethod::Lzma2,
            zip_method: ZipMethod::Deflate,
            solid: true,
            password: Some("hunter2".to_string()),
            encrypt_header: true,
        };
        create_with_options(&opts, &[src_file.clone()], &arc_7z).unwrap();

        // Without password should report password required
        assert!(list_with_password(&arc_7z, None).is_err());

        // With password should list, read, and verify
        let entries = list_with_password(&arc_7z, Some("hunter2")).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].encrypted);
        let data = read_entry_with_password(&arc_7z, "secret.txt", Some("hunter2")).unwrap();
        assert_eq!(data, b"top secret rust 7z payload\n");
        let (checked, errs) = verify_with_password(&arc_7z, Some("hunter2")).unwrap();
        assert_eq!(checked, 1);
        assert!(errs.is_empty());

        // 2. Single-file .zst roundtrip
        let arc_zst = tmp.join("secret.txt.zst");
        let zst_opts = CreateOptions {
            format: Format::Zst,
            ..CreateOptions::default()
        };
        create_with_options(&zst_opts, &[src_file], &arc_zst).unwrap();
        assert_eq!(detect(&arc_zst), Format::Zst);
        let zst_entries = list(&arc_zst).unwrap();
        assert_eq!(zst_entries.len(), 1);
        assert_eq!(zst_entries[0].path, "secret.txt");
        let zst_data = read_entry(&arc_zst, "secret.txt").unwrap();
        assert_eq!(zst_data, b"top secret rust 7z payload\n");

        let _ = std::fs::remove_dir_all(&tmp);
    }
    // ---------------------------------------------- best-effort extraction

    /// A tar.gz whose payload does not compress, so cutting the file lands in
    /// the middle of the stream instead of at a convenient boundary.
    fn write_targz(path: &Path, entries: &[(&str, Vec<u8>)]) {
        let f = File::create(path).unwrap();
        let enc = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
        let mut b = tar::Builder::new(enc);
        for (name, data) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_mtime(1_700_000_000);
            h.set_cksum();
            b.append_data(&mut h, name, &data[..]).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
    }

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "choab-zip-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Deterministic, poorly compressible payload.
    fn noise(len: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(len);
        let mut x: u64 = 0x243f_6a88_85a3_08d3;
        while v.len() < len {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            v.extend_from_slice(&x.to_le_bytes());
        }
        v.truncate(len);
        v
    }

    #[test]
    fn truncated_tar_gz_keeps_what_it_could_decode() {
        let tmp = scratch("truncated");
        let src = tmp.join("full.tar.gz");
        write_targz(
            &src,
            &[
                ("a/first.bin", noise(80_000)),
                ("a/second.bin", noise(200_000)),
                ("a/third.bin", noise(200_000)),
            ],
        );
        let bytes = std::fs::read(&src).unwrap();
        let cut = tmp.join("cut.tar.gz");
        std::fs::write(&cut, &bytes[..bytes.len() * 5 / 8]).unwrap();

        let out = tmp.join("out");
        let report = extract_with_progress(&cut, &out, None, None).unwrap();

        // The run reports itself as partial and says why.
        assert!(report.is_partial(), "expected a partial report");
        assert!(report.stopped.is_some(), "expected a stop reason");

        // Nothing was thrown away: the first entry is complete on disk.
        let first = out.join("a/first.bin");
        assert!(first.is_file(), "first entry should have been kept");
        assert_eq!(
            std::fs::read(&first).unwrap(),
            noise(80_000),
            "kept file must be byte-exact"
        );

        // Whatever fragment was written for the entry that broke is kept too.
        let kept: usize = report
            .incomplete
            .iter()
            .filter(|n| out.join(n).exists())
            .count();
        assert_eq!(kept, report.incomplete.len(), "partial files must stay on disk");

        // And the damaged source is copied next to them.
        let copy = report.corrupt_copy.expect("damaged copy expected");
        assert!(copy.is_file());
        assert_eq!(std::fs::metadata(&copy).unwrap().len(), bytes.len() as u64 * 5 / 8);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn intact_tar_gz_reports_no_warning_and_writes_no_copy() {
        let tmp = scratch("intact");
        let src = tmp.join("ok.tar.gz");
        write_targz(&src, &[("d/one.txt", b"one".to_vec()), ("d/two.txt", b"two".to_vec())]);

        let out = tmp.join("out");
        let report = extract_with_progress(&src, &out, None, None).unwrap();
        assert!(!report.is_partial());
        assert_eq!(report.entries, 2);
        assert!(report.corrupt_copy.is_none());
        assert_eq!(std::fs::read(out.join("d/two.txt")).unwrap(), b"two");
        assert!(!out.join("ok.tar.gz.corrupt").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn selected_extraction_honours_strip_prefix() {
        let tmp = scratch("selected");
        let src = tmp.join("sel.tar.gz");
        write_targz(
            &src,
            &[
                ("top/keep/x.txt", b"x".to_vec()),
                ("top/keep/y.txt", b"y".to_vec()),
                ("top/skip/z.txt", b"z".to_vec()),
            ],
        );

        let out = tmp.join("out");
        let report = extract_entries_with_password(
            &src,
            &["top/keep".to_string()],
            "top/keep",
            &out,
            None,
        )
        .unwrap();
        assert_eq!(report.entries, 2);
        assert!(out.join("x.txt").is_file());
        assert!(out.join("y.txt").is_file());
        assert!(!out.join("z.txt").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn listing_a_truncated_stream_reports_entries_and_a_warning() {
        let tmp = scratch("listtrunc");
        let src = tmp.join("list.tar.gz");
        write_targz(
            &src,
            &[("a.bin", noise(120_000)), ("b.bin", noise(120_000))],
        );
        let bytes = std::fs::read(&src).unwrap();
        let cut = tmp.join("cut.tar.gz");
        std::fs::write(&cut, &bytes[..bytes.len() * 2 / 3]).unwrap();

        let (entries, warning) = list_with_report(&cut, None).unwrap();
        assert!(!entries.is_empty(), "readable entries should be listed");
        assert!(warning.is_some(), "a truncated source must warn");

        let _ = std::fs::remove_dir_all(&tmp);
    }

}
