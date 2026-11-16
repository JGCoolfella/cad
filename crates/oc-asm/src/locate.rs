//! Finding a component's file.
//!
//! An assembly stores each component's path relative to its own folder, plus the absolute
//! path the file was last found at. Opening looks, in order (SOLIDWORKS' search for
//! referenced documents, reduced to what exists here; 🔍 the exact SOLIDWORKS order):
//! 1. the relative path, from the assembly's folder (moving the assembly together with
//!    its parts keeps it working);
//! 2. the stored absolute path;
//! 3. the file name in the assembly's folder;
//! 4. the file name in the extra search folders (System Options > File Locations >
//!    Referenced Documents, when it exists) and in the folders where earlier components of
//!    this assembly were found.
//!
//! File access goes through [`Store`], so tests can use files in memory.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where component files are read from.
pub trait Store {
    fn read(&self, path: &Path) -> Option<Vec<u8>>;
    fn exists(&self, path: &Path) -> bool {
        self.read(path).is_some()
    }
}

/// The file system.
pub struct Disk;

impl Store for Disk {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }
    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// Files in memory, by path (tests).
#[derive(Default)]
pub struct Memory(pub HashMap<PathBuf, Vec<u8>>);

impl Store for Memory {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        self.0.get(&normalize(path)).cloned()
    }
}

/// How a file was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    Relative,
    Absolute,
    AssemblyFolder,
    SearchFolder,
}

/// `a/./b/../c` → `a/c`, and `\` → `/` on every platform, so equal paths compare equal.
pub fn normalize(p: &Path) -> PathBuf {
    let text = p.to_string_lossy().replace('\\', "/");
    let mut out: Vec<&str> = Vec::new();
    let absolute = text.starts_with('/');
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." if out.last().is_some_and(|l| *l != ".." && !l.ends_with(':')) => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    let joined = out.join("/");
    PathBuf::from(if absolute { format!("/{joined}") } else { joined })
}

/// The `/`-separated path of `file` relative to the folder `dir` (`..` where needed), or
/// `None` when they share no root (other drives).
pub fn relative(file: &Path, dir: &Path) -> Option<String> {
    let f = normalize(file).to_string_lossy().into_owned();
    let d = normalize(dir).to_string_lossy().into_owned();
    let fs: Vec<&str> = f.split('/').collect();
    let ds: Vec<&str> = if d.is_empty() { vec![] } else { d.split('/').collect() };
    let same = |a: &str, b: &str| if cfg!(windows) { a.eq_ignore_ascii_case(b) } else { a == b };
    let common = fs.iter().zip(&ds).take_while(|(a, b)| same(a, b)).count();
    if common == 0 && !ds.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = std::iter::repeat_n("..", ds.len() - common).collect();
    parts.extend(&fs[common..]);
    Some(parts.join("/"))
}

/// Looks for a component file. `found_dirs` are folders earlier components were found in.
pub fn locate(store: &dyn Store, asm_dir: &Path, rel: &str, abs: Option<&str>, search: &[PathBuf], found_dirs: &[PathBuf]) -> Option<(PathBuf, Found)> {
    let candidate = |p: PathBuf| {
        let p = normalize(&p);
        store.exists(&p).then_some(p)
    };
    if let Some(p) = candidate(asm_dir.join(rel)) {
        return Some((p, Found::Relative));
    }
    if let Some(p) = abs.and_then(|a| candidate(PathBuf::from(a))) {
        return Some((p, Found::Absolute));
    }
    let name = Path::new(rel).file_name()?;
    if let Some(p) = candidate(asm_dir.join(name)) {
        return Some((p, Found::AssemblyFolder));
    }
    search.iter().chain(found_dirs).find_map(|d| candidate(d.join(name))).map(|p| (p, Found::SearchFolder))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(files: &[&str]) -> Memory {
        Memory(files.iter().map(|f| (normalize(Path::new(f)), vec![1])).collect())
    }

    #[test]
    fn search_order() {
        let dir = Path::new("/w/asm");
        let s = mem(&["/w/asm/parts/pin.ocpart", "/old/pin.ocpart", "/w/asm/bolt.ocpart", "/lib/nut.ocpart", "/w/other/plate.ocpart"]);
        let l = |rel: &str, abs: Option<&str>| locate(&s, dir, rel, abs, &[PathBuf::from("/lib")], &[PathBuf::from("/w/other")]);
        assert_eq!(l("parts/pin.ocpart", Some("/old/pin.ocpart")), Some((PathBuf::from("/w/asm/parts/pin.ocpart"), Found::Relative)));
        assert_eq!(l("gone/pin.ocpart", Some("/old/pin.ocpart")), Some((PathBuf::from("/old/pin.ocpart"), Found::Absolute)));
        assert_eq!(l("sub/bolt.ocpart", Some("/nowhere/bolt.ocpart")), Some((PathBuf::from("/w/asm/bolt.ocpart"), Found::AssemblyFolder)));
        assert_eq!(l("x/nut.ocpart", None), Some((PathBuf::from("/lib/nut.ocpart"), Found::SearchFolder)));
        assert_eq!(l("plate.ocpart", None).map(|x| x.1), Some(Found::SearchFolder));
        assert_eq!(l("missing.ocpart", None), None);
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("/w/asm/parts/pin.ocpart"), Path::new("/w/asm")).as_deref(), Some("parts/pin.ocpart"));
        assert_eq!(relative(Path::new("/w/lib/pin.ocpart"), Path::new("/w/asm")).as_deref(), Some("../lib/pin.ocpart"));
        assert_eq!(relative(Path::new("C:\\a\\b.ocpart"), Path::new("C:\\a")).as_deref(), Some("b.ocpart"));
        assert_eq!(relative(Path::new("D:/x/b.ocpart"), Path::new("C:/a")), None);
        assert_eq!(normalize(Path::new("a/./b/../c")), PathBuf::from("a/c"));
    }
}
