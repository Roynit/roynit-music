//! A tiny self-extracting archive: `<program bytes><entries><footer>` appended to one file.
//! Each entry is gzip-compressed; the footer holds the entries' length and a magic tag.
use anyhow::{Context, Result, bail, ensure};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

const MAGIC: &[u8; 8] = b"ROYPAY01";
const FOOTER: u64 = 16;
const MAX_ENTRY: u64 = 512 * 1024 * 1024;

/// Offset where the payload starts and its length; `None` when the file carries none.
pub fn locate(file: &mut std::fs::File) -> Result<Option<(u64, u64)>> {
    let len = file.metadata()?.len();
    if len < FOOTER {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(len - FOOTER))?;
    let mut footer = [0u8; 16];
    file.read_exact(&mut footer)?;
    if &footer[8..] != MAGIC {
        return Ok(None);
    }
    let payload = u64::from_le_bytes(footer[..8].try_into()?);
    ensure!(payload <= len - FOOTER, "The installer payload is damaged");
    Ok(Some((len - FOOTER - payload, payload)))
}

/// Append `files` (stored under their file names) to a copy of `program`.
pub fn pack(program: &Path, output: &Path, files: &[&Path]) -> Result<()> {
    // Windows file names ignore case; packing a file onto itself would truncate the program first.
    if let (Ok(a), Ok(b)) = (program.canonicalize(), output.canonicalize()) {
        ensure!(
            a != b,
            "The output must be a different file from the program"
        );
    }
    let mut source = std::fs::File::open(program)?;
    let end = locate(&mut source)?.map_or(source.metadata()?.len(), |(start, _)| start);
    source.seek(SeekFrom::Start(0))?;
    let mut out = std::fs::File::create(output)?;
    std::io::copy(&mut (&mut source).take(end), &mut out)?;
    let mut payload = 0u64;
    for path in files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .context("File name is not UTF-8")?;
        ensure!(name.len() <= u16::MAX as usize, "File name too long");
        let raw = std::fs::read(path).with_context(|| format!("Cannot read {}", path.display()))?;
        let mut gz = GzEncoder::new(Vec::new(), Compression::best());
        gz.write_all(&raw)?;
        let gz = gz.finish()?;
        out.write_all(&(name.len() as u16).to_le_bytes())?;
        out.write_all(name.as_bytes())?;
        out.write_all(&(raw.len() as u64).to_le_bytes())?;
        out.write_all(&(gz.len() as u64).to_le_bytes())?;
        out.write_all(&gz)?;
        payload += 2 + name.len() as u64 + 16 + gz.len() as u64;
    }
    out.write_all(&payload.to_le_bytes())?;
    out.write_all(MAGIC)?;
    out.sync_all()?;
    Ok(())
}

/// Read every entry as `(name, bytes)`.
pub fn entries(file: &mut std::fs::File) -> Result<Vec<(String, Vec<u8>)>> {
    let Some((start, len)) = locate(file)? else {
        return Ok(Vec::new());
    };
    file.seek(SeekFrom::Start(start))?;
    let mut data = Vec::with_capacity(len as usize);
    (&mut *file).take(len).read_to_end(&mut data)?;
    let mut cursor = std::io::Cursor::new(data);
    let mut out = Vec::new();
    while (cursor.position()) < len {
        let mut small = [0u8; 2];
        cursor.read_exact(&mut small)?;
        let mut name = vec![0u8; u16::from_le_bytes(small) as usize];
        cursor.read_exact(&mut name)?;
        let name = String::from_utf8(name)?;
        ensure!(
            !name.is_empty() && !name.contains(['/', '\\']) && name != "..",
            "Unsafe entry name"
        );
        let mut big = [0u8; 8];
        cursor.read_exact(&mut big)?;
        let raw_len = u64::from_le_bytes(big);
        cursor.read_exact(&mut big)?;
        let gz_len = u64::from_le_bytes(big);
        ensure!(
            raw_len <= MAX_ENTRY && gz_len <= MAX_ENTRY,
            "Entry is too large"
        );
        let mut gz = vec![0u8; gz_len as usize];
        cursor.read_exact(&mut gz)?;
        let mut raw = Vec::with_capacity(raw_len as usize);
        GzDecoder::new(&gz[..])
            .take(MAX_ENTRY)
            .read_to_end(&mut raw)?;
        if raw.len() as u64 != raw_len {
            bail!("Entry {name} is damaged");
        }
        out.push((name, raw));
    }
    Ok(out)
}

/// The program's own bytes without the payload (what an uninstaller copy needs).
pub fn program_bytes(file: &mut std::fs::File) -> Result<Vec<u8>> {
    let end = locate(file)?.map_or(file.metadata()?.len(), |(start, _)| start);
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::with_capacity(end as usize);
    (&mut *file).take(end).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pack_then_read_roundtrips_and_the_program_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("setup.exe");
        std::fs::write(&program, b"MZ-program-bytes").unwrap();
        let a = dir.path().join("riff-desktop.exe");
        let b = dir.path().join("signing-status.json");
        std::fs::write(&a, vec![7u8; 50_000]).unwrap();
        std::fs::write(&b, b"{}").unwrap();
        let packed = dir.path().join("Packed.exe");
        pack(&program, &packed, &[&a, &b]).unwrap();
        let mut f = std::fs::File::open(&packed).unwrap();
        let got = entries(&mut f).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], ("riff-desktop.exe".to_string(), vec![7u8; 50_000]));
        assert_eq!(got[1].1, b"{}");
        assert_eq!(program_bytes(&mut f).unwrap(), b"MZ-program-bytes");
        assert!(pack(&packed, &packed, &[&b]).is_err());
        // Packing an already packed file replaces the payload instead of stacking it.
        let again = dir.path().join("Again.exe");
        pack(&packed, &again, &[&b]).unwrap();
        let mut f = std::fs::File::open(&again).unwrap();
        assert_eq!(entries(&mut f).unwrap().len(), 1);
        assert_eq!(program_bytes(&mut f).unwrap(), b"MZ-program-bytes");
    }
    #[test]
    fn a_plain_executable_has_no_payload() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain.exe");
        std::fs::write(
            &plain,
            b"MZ plain executable, long enough to read a footer from",
        )
        .unwrap();
        let mut f = std::fs::File::open(&plain).unwrap();
        assert!(entries(&mut f).unwrap().is_empty());
    }
}
