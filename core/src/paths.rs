//! Receiver-side path safety. Every name from the network goes through `sanitize`
//! and every write target through `resolve` before touching the filesystem.
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

/// Turn a peer-supplied relative path into a safe relative `PathBuf`, or reject it.
pub fn sanitize(rel: &str) -> Result<PathBuf> {
    let s: String = rel.nfc().collect();
    if s.is_empty() || s.len() > 4096 {
        bail!("invalid file name length");
    }
    let mut out = PathBuf::new();
    for comp in s.split(['/', '\\']) {
        if comp.is_empty() || comp == "." || comp == ".." {
            bail!("unsafe path {rel:?}");
        }
        if comp.len() > 255 {
            bail!("file name longer than 255 bytes: {comp:?}");
        }
        if comp.chars().any(|c| c.is_control()) {
            bail!("file name contains control characters: {comp:?}");
        }
        out.push(platform_name(comp));
    }
    Ok(out)
}

#[cfg(windows)]
fn platform_name(c: &str) -> String {
    let mut n: String = c
        .chars()
        .map(|ch| if "<>:\"|?*".contains(ch) { '_' } else { ch })
        .collect();
    n = n.trim_end_matches(['.', ' ']).to_string();
    let stem = n.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved || n.is_empty() {
        n.insert(0, '_');
    }
    n
}

#[cfg(not(windows))]
fn platform_name(c: &str) -> String {
    c.to_string()
}

/// Join under `root`, refusing to pass through any existing symlink below `root`.
pub fn resolve(root: &Path, rel: &Path) -> Result<PathBuf> {
    let mut p = root.to_path_buf();
    for c in rel.components() {
        p.push(c);
        if let Ok(m) = std::fs::symlink_metadata(&p) {
            if m.file_type().is_symlink() {
                bail!("refusing to write through symlink {}", p.display());
            }
        }
    }
    Ok(p)
}

/// Hidden partial file next to the target, so incomplete data never looks complete.
pub fn part_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = if name.len() > 240 {
        blake3::hash(name.as_bytes()).to_hex()[..32].to_string()
    } else {
        name
    };
    target.with_file_name(format!(".{base}.dbpart"))
}

/// `a.txt` -> `a (1).txt`, `a (2).txt`, ... first free name.
pub fn unique(target: &Path) -> PathBuf {
    if std::fs::symlink_metadata(target).is_err() {
        return target.to_path_buf();
    }
    let stem = target
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = target
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (1..)
        .map(|n| target.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_junk() {
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a/./b",
            "a//b",
            "..",
            "a\\..\\b",
            "x\0y",
            "a\nb",
            "\u{1b}[31m",
            &"a".repeat(256),
        ] {
            assert!(sanitize(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn accepts_real_names() {
        for good in [
            "photo.jpg",
            ".hidden",
            "Folder/sub/file.tar.gz",
            "ছবি/ফাইল.txt",
            "emoji 🎉 (1).png",
            "a b c/ d",
            &"a".repeat(255),
        ] {
            assert!(sanitize(good).is_ok(), "{good:?} should be accepted");
        }
        // NFD input becomes NFC so macOS/Android names compare equal
        assert_eq!(
            sanitize("e\u{301}.txt").unwrap(),
            PathBuf::from("\u{e9}.txt")
        );
    }

    #[test]
    fn symlink_escape_blocked_and_unique_names() {
        let d = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp", d.path().join("link")).unwrap();
            assert!(resolve(d.path(), Path::new("link/x")).is_err());
        }
        let t = d.path().join("a.txt");
        assert_eq!(unique(&t), t);
        std::fs::write(&t, "1").unwrap();
        assert_eq!(unique(&t), d.path().join("a (1).txt"));
        assert!(
            part_path(&d.path().join("n".repeat(255)))
                .file_name()
                .unwrap()
                .len()
                < 255
        );
    }
}
