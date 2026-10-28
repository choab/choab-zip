//! Archive engine: format detection + unified read/write over pure-Rust codecs.
//!
//! Deliberately no shelling out to `7z`. Everything here is native Rust so the
//! binary is self-contained and fast.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

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
    Rar,
    /// Recognised by magic bytes but no reader is available.
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
            Format::Rar => "rar",
            Format::Unknown => "unknown",
        }
    }

    /// RAR is extract/list only — we never create it.
    #[allow(dead_code)] // part of the public engine API
    pub fn can_write(self) -> bool {
        matches!(
            self,
            Format::SevenZ | Format::Zip | Format::Tar | Format::TarGz | Format::TarXz | Format::TarBz2 | Format::TarZst
        )
    }

    pub fn extensions() -> &'static [(&'static str, Format)] {
        &[
            ("7z", Format::SevenZ),
            ("zip", Format::Zip),
            ("tar.gz", Format::TarGz),
            ("tgz", Format::TarGz),
            ("tar.xz", Format::TarXz),
            ("txz", Format::TarXz),
            ("tar.bz2", Format::TarBz2),
            ("tbz2", Format::TarBz2),
            ("tar.zst", Format::TarZst),
            ("tzst", Format::TarZst),
            ("tar", Format::Tar),
            ("gz", Format::Gz),
            ("xz", Format::Xz),
            ("bz2", Format::Bz2),
            ("rar", Format::Rar),
            // Multi-volume RAR parts are all openable as the archive itself.
            ("r00", Format::Rar),
            ("r01", Format::Rar),
        ]
    }
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
                return Format::TarGz;
            }
            if &head[..6] == b"\xFD7zXZ\0" {
                return Format::TarXz;
            }
            if &head[..4] == b"BZh " {
                return Format::TarBz2;
            }
            if &head[..4] == [0x28, 0xB5, 0x2F, 0xFD] {
                return Format::TarZst;
            }
            // Uncompressed tar: "ustar" sits at offset 257, too far for `head`.
            if let Ok(mut f2) = File::open(path) {
                use std::io::Seek;
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
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
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

pub type Result<T> = std::result::Result<T, Error>;

/// List the contents of an archive without extracting.
pub fn list(path: &Path) -> Result<Vec<Entry>> {
    let fmt = detect(path);
    match fmt {
        Format::SevenZ => list_7z(path),
        Format::Zip => list_zip(path),
        Format::Rar => list_rar(path),
        Format::Unknown => Err(Error::Other(
            "unrecognised archive format".to_string(),
        )),
        _ => list_tar(path),
    }
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
            // RAR stores pack sizes per file, but the crate does not expose
            // them in listing mode; report 0 rather than a guess.
            packed: 0,
            is_dir: h.is_directory(),
            modified: Some(dos_time(h.file_time)),
            crc: Some(h.file_crc),
            method: Some("RAR".to_string()),
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

fn list_7z(path: &Path) -> Result<Vec<Entry>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut out = Vec::new();
    sevenz_rust2::ArchiveReader::new(reader, sevenz_rust2::Password::empty())
        .map_err(|e| Error::Other(e.to_string()))?
        .for_each_entries(|entry, _reader| {
            out.push(Entry {
                path: entry.name().to_string(),
                size: entry.size(),
                // 7z stores pack sizes per folder, not per file; a per-entry
                // value would be a guess. Report 0 and let the UI blank it.
                packed: 0,
                is_dir: entry.is_directory(),
                modified: {
                    let d = entry.last_modified_date();
                    if d == sevenz_rust2::NtTime::NT_TIME_EPOCH {
                        None
                    } else {
                        Some(format_nt(u64::from(d)))
                    }
                },
                crc: entry.has_crc.then_some(entry.crc as u32),
                method: None,
            });
            Ok(true)
        })
        .map_err(|e| Error::Other(e.to_string()))?;

    Ok(out)
}

fn list_zip(path: &Path) -> Result<Vec<Entry>> {
    let file = File::open(path)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| Error::Other(e.to_string()))?;
    let mut out = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|e| Error::Other(e.to_string()))?;
        out.push(Entry {
            path: f.name().map(|c| c.to_string()).unwrap_or_default(),
            size: f.size(),
            packed: f.compressed_size(),
            is_dir: f.is_dir(),
            modified: f.last_modified().map(|d| zip_time(&d)),
            crc: Some(f.crc32()),
            method: Some(match f.compression() {
                zip::CompressionMethod::Stored => "Store",
                zip::CompressionMethod::Deflated => "Deflate",
                zip::CompressionMethod::Bzip2 => "BZip2",
                zip::CompressionMethod::Zstd => "ZSTD",
                zip::CompressionMethod::Lzma => "LZMA",
                _ => "?",
            }
            .to_string()),
        });
    }
    Ok(out)
}

fn list_tar(path: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    visit_tar(path, &mut |e| {
        entries.push(e);
        Ok(())
    })?;
    Ok(entries)
}

/// Walk a tar (optionally compressed) archive.
fn visit_tar<F>(path: &Path, sink: &mut F) -> Result<()>
where
    F: FnMut(Entry) -> Result<()>,
{
    let fmt = detect(path);
    let file = File::open(path)?;
    let buf = BufReader::new(file);

    let reader: Box<dyn Read> = match fmt {
        Format::TarGz => Box::new(flate2::read::GzDecoder::new(buf)),
        Format::TarXz => Box::new(xz2::read::XzDecoder::new(buf)),
        Format::TarBz2 => Box::new(bzip2::read::BzDecoder::new(buf)),
        Format::TarZst => Box::new(zstd::stream::read::Decoder::new(buf).map_err(|e| Error::Other(e.to_string()))?),
        _ => Box::new(buf),
    };

    let mut ar = tar::Archive::new(reader);
    for item in ar.entries().map_err(|e| Error::Other(e.to_string()))? {
        let item = item.map_err(|e| Error::Other(e.to_string()))?;
        let hdr = item.header();
        let is_dir = hdr.entry_type().is_dir();
        let size = hdr.size().unwrap_or(0);
        let mtime = hdr.mtime().ok().map(|t| format_epoch(t));
        let p = item
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        sink(Entry {
            path: p,
            size,
            packed: 0,
            is_dir,
            modified: mtime,
            crc: None,
            method: None,
        })?;
    }
    Ok(())
}

fn format_nt(nt: u64) -> String {    // NT epoch: 100-ns ticks since 1601-01-01.
    const EPOCH_DIFF: u64 = 11_644_473_600;
    let secs = nt / 10_000_000;
    if secs < EPOCH_DIFF {
        return String::new();
    }
    format_epoch(secs - EPOCH_DIFF)
}

fn format_epoch(secs: u64) -> String {
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
    let mlen = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0usize;
    while m < 12 && d >= mlen[m] {
        d -= mlen[m];
        m += 1;
    }
    format!("{y:04}-{:02}-{:02} {:02}:{:02}", m + 1, d + 1, h, mi)
}

fn zip_time(d: &zip::DateTime) -> String {
    // zip stores 1980-based local time without a timezone.
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        d.year(),
        d.month(),
        d.day(),
        d.hour(),
        d.minute()
    )
}

/// Extract a single entry, or every entry under a directory prefix.
///
/// Paths stay archive-relative, matching 7-Zip: selecting `sub/x.txt` writes
/// `<dest>/sub/x.txt`. Each file is decoded one at a time, so peak memory is
/// the largest selected file rather than the whole selection.
pub fn extract_entry(archive: &Path, entry_path: &str, dest: &Path) -> Result<usize> {
    std::fs::create_dir_all(dest)?;
    let want = entry_path.trim_end_matches('/');
    let prefix = format!("{want}/");

    let all = list(archive)?;
    let selected: Vec<String> = all
        .iter()
        .filter(|e| !e.is_dir && (e.path == want || e.path.starts_with(&prefix)))
        .map(|e| e.path.clone())
        .collect();

    if selected.is_empty() {
        return Err(Error::Other(format!("no such entry: {want}")));
    }

    let mut n = 0;
    for rel in &selected {
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = read_entry(archive, rel)?;
        std::fs::write(&out, data)?;
        n += 1;
    }
    Ok(n)
}

/// Extract everything in `archive` into `dest`.
pub fn extract(archive: &Path, dest: &Path) -> Result<usize> {
    std::fs::create_dir_all(dest)?;
    let fmt = detect(archive);
    match fmt {
        Format::SevenZ => extract_7z(archive, dest),
        Format::Zip => extract_zip(archive, dest),
        Format::Gz => extract_single(archive, dest, "gz"),
        Format::Xz => extract_single(archive, dest, "xz"),
        Format::Bz2 => extract_single(archive, dest, "bz2"),
        Format::Rar => extract_rar(archive, dest),
        Format::Unknown => Err(Error::Other(
            "unrecognised archive format".to_string(),
        )),
        _ => extract_tar(archive, dest),
    }
}

fn extract_7z(archive: &Path, dest: &Path) -> Result<usize> {
    sevenz_rust2::decompress_file(archive, dest).map_err(|e| Error::Other(e.to_string()))?;
    // The API reports success without a count; enumerate what landed on disk.
    Ok(count_files(dest))
}

fn count_files(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                n += count_files(&p);
            } else {
                n += 1;
            }
        }
    }
    n
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<usize> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| Error::Other(e.to_string()))?;
    let mut count = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| Error::Other(e.to_string()))?;
        let Some(rel) = f.enclosed_name() else { continue };
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
        count += 1;
    }
    Ok(count)
}

fn extract_tar(archive: &Path, dest: &Path) -> Result<usize> {
    let fmt = detect(archive);
    let file = File::open(archive)?;
    let buf = BufReader::new(file);
    let reader: Box<dyn Read> = match fmt {
        Format::TarGz => Box::new(flate2::read::GzDecoder::new(buf)),
        Format::TarXz => Box::new(xz2::read::XzDecoder::new(buf)),
        Format::TarBz2 => Box::new(bzip2::read::BzDecoder::new(buf)),
        Format::TarZst => Box::new(zstd::stream::read::Decoder::new(buf).map_err(|e| Error::Other(e.to_string()))?),
        _ => Box::new(buf),
    };
    let mut ar = tar::Archive::new(reader);
    let mut count = 0;
    for item in ar.entries().map_err(|e| Error::Other(e.to_string()))? {
        let mut item = item.map_err(|e| Error::Other(e.to_string()))?;
        if item.path().map(|p| p.to_string_lossy().starts_with('/')).unwrap_or(false) {
            continue; // refuse absolute paths
        }
        item.unpack_in(dest).map_err(|e| Error::Other(e.to_string()))?;
        count += 1;
    }
    Ok(count)
}

fn extract_single(archive: &Path, dest: &Path, kind: &str) -> Result<usize> {
    let file = File::open(archive)?;
    let buf = BufReader::new(file);
    let mut reader: Box<dyn Read> = match kind {
        "gz" => Box::new(flate2::read::GzDecoder::new(buf)),
        "xz" => Box::new(xz2::read::XzDecoder::new(buf)),
        _ => Box::new(bzip2::read::BzDecoder::new(buf)),
    };
    let stem = archive
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("extracted");
    let out = dest.join(stem);
    let mut w = BufWriter::new(File::create(&out)?);
    io::copy(&mut reader, &mut w)?;
    Ok(1)
}

/// Create an archive from a list of source paths.
pub fn create(fmt: Format, sources: &[PathBuf], target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    // Fail loudly instead of writing a tar with a misleading extension.
    if !fmt.can_write() {
        return Err(Error::Other(format!(
            "cannot create {} archives (extract/list only)",
            fmt.label()
        )));
    }
    match fmt {
        Format::SevenZ => {
            let w = sevenz_rust2::ArchiveWriter::create(target)
                .map_err(|e| Error::Other(e.to_string()))?;
            let mut w = w;
            for src in sources {
                push_7z(&mut w, src, src)?;
            }
            w.finish().map_err(|e| Error::Other(e.to_string()))?;
            Ok(())
        }
        Format::Zip => create_zip(sources, target),
        Format::Tar | Format::TarGz | Format::TarXz | Format::TarBz2 | Format::TarZst => {
            create_tar(fmt, sources, target)
        }
        Format::Rar | Format::Unknown | Format::Gz | Format::Xz | Format::Bz2 => {
            Err(Error::Other(format!(
                "cannot create {} archives",
                fmt.label()
            )))
        }
    }
}

fn push_7z<W: Write + std::io::Seek>(
    w: &mut sevenz_rust2::ArchiveWriter<W>,
    src: &Path,
    base: &Path,
) -> Result<()> {
    let name = src
        .strip_prefix(base.parent().unwrap_or(Path::new("")))
        .unwrap_or(src);
    if src.is_dir() {
        w.push_archive_entry(
            sevenz_rust2::ArchiveEntry::new_directory(&name.to_string_lossy()),
            None::<File>,
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        for e in std::fs::read_dir(src)? {
            push_7z(w, &e?.path(), base)?;
        }
    } else {
        let f = File::open(src)?;
        w.push_archive_entry(
            sevenz_rust2::ArchiveEntry::new_file(&name.to_string_lossy()),
            Some(f),
        )
        .map_err(|e| Error::Other(e.to_string()))?;
    }
    Ok(())
}

fn create_zip(sources: &[PathBuf], target: &Path) -> Result<()> {
    let file = File::create(target)?;
    let mut zw = zip::ZipWriter::new(BufWriter::new(file));
    let opts = zip::write::SimpleFileOptions::default();
    for src in sources {
        add_zip(&mut zw, src, opts, src.parent().unwrap_or(Path::new("")))?;
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
        for e in std::fs::read_dir(src)? {
            add_zip(zw, &e?.path(), opts, base)?;
        }
    } else {
        zw.start_file(name, opts)
            .map_err(|e| Error::Other(e.to_string()))?;
        let mut f = File::open(src)?;
        io::copy(&mut f, zw)?;
    }
    Ok(())
}

fn create_tar(fmt: Format, sources: &[PathBuf], target: &Path) -> Result<()> {
    let file = File::create(target)?;
    let buf = BufWriter::new(file);
    let writer: Box<dyn Write> = match fmt {
        Format::TarGz => Box::new(flate2::write::GzEncoder::new(buf, flate2::Compression::default())),
        Format::TarXz => Box::new(xz2::write::XzEncoder::new(buf, 6)),
        Format::TarBz2 => Box::new(bzip2::write::BzEncoder::new(buf, bzip2::Compression::default())),
        Format::TarZst => Box::new(zstd::stream::write::Encoder::new(buf, 3).map_err(|e| Error::Other(e.to_string()))?.auto_finish()),
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
pub fn container_size(path: &Path) -> u64 {    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Read one entry fully into memory. Powers the built-in viewer, so opening a
/// file inside an archive never shells out to another program.
pub fn read_entry(archive: &Path, entry_path: &str) -> Result<Vec<u8>> {
    match detect(archive) {
        Format::SevenZ => read_entry_7z(archive, entry_path),
        Format::Zip => read_entry_zip(archive, entry_path),
        Format::Rar => read_entry_rar(archive, entry_path),
        Format::Unknown => Err(Error::Other(
            "unrecognised archive format".to_string(),
        )),
        _ => read_entry_tar(archive, entry_path),
    }
}

fn extract_rar(archive: &Path, dest: &Path) -> Result<usize> {
    let mut count = 0usize;
    let open = unrar::Archive::new(archive)
        .open_for_processing()
        .map_err(|e| Error::Other(format!("rar: {e}")))?;
    let mut cursor = open;
    loop {
        match cursor.read_header() {
            Ok(Some(header)) => {
                let is_dir = header.entry().is_directory();
                // `extract_with_base` extracts byte-identically into a
                // DIRECTORY; `extract_to` would write to `dest` as a filename.
                let next = header
                    .extract_with_base(dest)
                    .map_err(|e| Error::Other(format!("rar: {e}")))?;
                if !is_dir {
                    count += 1;
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
    let open = unrar::Archive::new(archive)
        .open_for_processing()
        .map_err(|e| Error::Other(format!("rar: {e}")))?;
    let mut cursor = open;
    loop {
        match cursor.read_header() {
            Ok(Some(header)) => {
                let name = header.entry().filename.to_string_lossy().replace('\\', "/");
                if name == want && !header.entry().is_directory() {
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

fn read_entry_7z(archive: &Path, want: &str) -> Result<Vec<u8>> {
    let file = File::open(archive)?;
    let mut ar = sevenz_rust2::ArchiveReader::new(
        BufReader::new(file),
        sevenz_rust2::Password::empty(),
    )
    .map_err(|e| Error::Other(e.to_string()))?;
    // `read_file` decodes through preceding entries in the solid block. Using
    // `for_each_entries` and stopping early would corrupt the decoder state and
    // poison every later read in the same block.
    ar.read_file(want).map_err(|e| Error::Other(e.to_string()))
}

fn read_entry_zip(archive: &Path, want: &str) -> Result<Vec<u8>> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| Error::Other(e.to_string()))?;
    let mut f = zip
        .by_name(want)
        .map_err(|e| Error::Other(e.to_string()))?;
    let mut buf = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

fn read_entry_tar(archive: &Path, want: &str) -> Result<Vec<u8>> {
    let fmt = detect(archive);
    let file = File::open(archive)?;
    let buf = BufReader::new(file);
    let reader: Box<dyn Read> = match fmt {
        Format::TarGz => Box::new(flate2::read::GzDecoder::new(buf)),
        Format::TarXz => Box::new(xz2::read::XzDecoder::new(buf)),
        Format::TarBz2 => Box::new(bzip2::read::BzDecoder::new(buf)),
        Format::TarZst => Box::new(
            zstd::stream::read::Decoder::new(buf).map_err(|e| Error::Other(e.to_string()))?
        ),
        _ => Box::new(buf),
    };
    let mut ar = tar::Archive::new(reader);
    for item in ar.entries().map_err(|e| Error::Other(e.to_string()))? {
        let mut item = item.map_err(|e| Error::Other(e.to_string()))?;
        let p = item
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        if p == want {
            let mut buf = Vec::new();
            item.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    Err(Error::Other(format!("entry not found: {want}")))
}

/// Verify every entry decodes. Returns (checked, errors).
pub fn verify(archive: &Path) -> Result<(usize, Vec<String>)> {
    let entries = list(archive)?;
    let mut checked = 0usize;
    let mut errs = Vec::new();
    for e in entries.iter().filter(|e| !e.is_dir) {
        match read_entry(archive, &e.path) {
            Ok(data) => {
                if data.len() as u64 != e.size {
                    errs.push(format!(
                        "{}: size {} != declared {}",
                        e.path,
                        data.len(),
                        e.size
                    ));
                } else if let Some(want) = e.crc {
                    let got = crc32(&data);
                    if got != want {
                        errs.push(format!(
                            "{}: CRC {got:08X} != {want:08X}",
                            e.path
                        ));
                    }
                }
                checked += 1;
            }
            Err(err) => errs.push(format!("{}: {err}", e.path)),
        }
    }
    Ok((checked, errs))
}

/// CRC-32 (IEEE), used by zip and 7z entry checksums.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}
