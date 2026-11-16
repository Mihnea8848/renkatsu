// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What the installer carries: a zip of the files to install, appended to
//! the installer program by the release workflow, followed by its length
//! and a mark, so that one file is the whole download.
//!
//! ```text
//! [installer program][zip][zip length, u64 little endian][MARK]
//! ```

use anyhow::{bail, Context, Result};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

/// The last eight bytes of an installer that carries files.
pub const MARK: &[u8; 8] = b"RKPAYLD1";

/// The zip at the end of `bytes`, when there is one.
pub fn find(bytes: &[u8]) -> Option<&[u8]> {
    let n = bytes.len();
    if n < 16 || &bytes[n - 8..] != MARK {
        return None;
    }
    let len = u64::from_le_bytes(bytes[n - 16..n - 8].try_into().ok()?) as usize;
    let end = n - 16;
    let start = end.checked_sub(len)?;
    Some(&bytes[start..end])
}

/// Appends a zip to an installer program, as the release workflow does.
pub fn append(program: &[u8], zip: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(program.len() + zip.len() + 16);
    out.extend_from_slice(program);
    out.extend_from_slice(zip);
    out.extend_from_slice(&(zip.len() as u64).to_le_bytes());
    out.extend_from_slice(MARK);
    out
}

/// One file of the payload.
pub struct File {
    /// Its path inside the payload, with forward slashes.
    pub path: String,
    pub bytes: Vec<u8>,
    pub executable: bool,
}

/// Reads every file of a payload zip.
pub fn files(zip: &[u8]) -> Result<Vec<File>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).context("reading the installer's files")?;
    let mut out = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else {
            bail!("the installer holds a file with an unsafe name: {:?}", entry.name_raw());
        };
        let path = path.to_string_lossy().replace('\\', "/");
        let executable = entry.unix_mode().is_some_and(|m| m & 0o111 != 0);
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        out.push(File {
            path,
            bytes,
            executable,
        });
    }
    Ok(out)
}

/// The payload of the running installer.
pub fn own() -> Result<Vec<File>> {
    let exe: PathBuf = std::env::current_exe().context("finding the installer program")?;
    let bytes = std::fs::read(&exe).with_context(|| format!("reading {}", exe.display()))?;
    match find(&bytes) {
        Some(zip) => files(zip),
        None => bail!("this installer carries no Renkatsu files; download it again from the releases page"),
    }
}

/// Writes a payload file below `dir`.
pub fn write(dir: &Path, file: &File) -> Result<PathBuf> {
    let target = dir.join(&file.path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    // A running program cannot be overwritten in place, but it can be
    // replaced: write beside it and rename.
    let temporary = target.with_extension("new");
    std::fs::write(&temporary, &file.bytes).with_context(|| format!("writing {}", target.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if file.executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(mode))?;
    }
    std::fs::rename(&temporary, &target).with_context(|| format!("writing {}", target.display()))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut out);
            for (name, bytes, mode) in files {
                let options = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
                zip.start_file(*name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
            zip.finish().unwrap();
        }
        out.into_inner()
    }

    // The installer is for Linux: file modes and desktop paths are Unix ones.
    #[cfg(unix)]
    #[test]
    fn a_payload_is_found_after_the_program_and_read_back() {
        let zip = zip_of(&[("renkatsu", b"app", 0o755), ("LICENSES/OFL-1.1.txt", b"ofl", 0o644)]);
        let installer = append(b"program bytes", &zip);
        assert_eq!(find(&installer), Some(&zip[..]));
        let files = files(find(&installer).unwrap()).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!((files[0].path.as_str(), files[0].executable), ("renkatsu", true));
        assert_eq!(
            (files[1].path.as_str(), files[1].executable),
            ("LICENSES/OFL-1.1.txt", false)
        );
        assert_eq!(files[1].bytes, b"ofl");
        // A program without a payload, or a short or damaged end, has none.
        assert_eq!(find(b"program bytes"), None);
        assert_eq!(find(b"RKPAYLD1"), None);
        let mut broken = installer.clone();
        let n = broken.len();
        broken[n - 16..n - 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(find(&broken), None);
    }

    #[test]
    fn written_files_replace_the_old_ones() {
        let dir = std::env::temp_dir().join(format!("rk-installer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = |bytes: &[u8]| File {
            path: "bin/renkatsu".to_owned(),
            bytes: bytes.to_vec(),
            executable: true,
        };
        let target = write(&dir, &file(b"one")).unwrap();
        write(&dir, &file(b"two")).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"two");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
