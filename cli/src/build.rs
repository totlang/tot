//! Resolving `(import …)` against the filesystem.

use std::path::{Path, PathBuf};

use tot::Dialect;
use tot::template::{Imports, Loaded};

/// Reads imports from disk, **relative to the file doing the importing**.
///
/// Relative to the importer rather than to where `tot` was invoked is the only answer that
/// makes a fragment relocatable: a directory of templates that import each other keeps working
/// wherever it is checked out, and whatever directory the build is run from.
#[derive(Default)]
pub struct Files {
    loaded: Vec<PathBuf>,
}

impl Files {
    /// Every file this importer read, so a caller can refuse to write over one of them. A
    /// fragment is as much a file being read as the template is, and losing one to `--out`
    /// is the same loss.
    pub fn loaded(&self) -> &[PathBuf] {
        &self.loaded
    }
}

impl Imports for Files {
    fn load(&mut self, from: &str, target: &str) -> Result<Loaded, String> {
        let base = Path::new(from).parent().unwrap_or_else(|| Path::new(""));
        let path = base.join(target);
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot import `{target}`: {}: {e}", path.display()))?;
        self.loaded.push(path.clone());
        Ok(Loaded {
            name: name(&path),
            source,
            dialect: dialect(&path),
        })
    }
}

/// Which language a file is written in, which follows its extension. A `.tot` file is data even
/// when a template imported it, so its parens stay ordinary characters.
pub fn dialect(path: &Path) -> Dialect {
    match extension(path).as_deref() {
        Some("tott") => Dialect::Template,
        _ => Dialect::Data,
    }
}

/// A path's extension, lowercased.
///
/// Case-folded because Windows filenames are, so `CONFIG.TOT` and `config.tot` are one file
/// there and must not be two different kinds of input depending on which spelling reached the
/// command line. Folding everywhere beats folding on one platform: a rule that holds only on
/// some machines is worse than either answer.
fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

/// What to call a file, in diagnostics and for deciding whether two imports are the same file.
///
/// Canonical, so that `a/../b.tot` and `b.tot` are one file and a cycle through them is caught;
/// then trimmed back to something a reader recognizes — relative to the working directory where
/// it can be, and without the `\\?\` prefix Windows canonicalization adds.
pub fn name(path: &Path) -> String {
    let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    // Trimmed **component by component**, never as text. `C:\a\b.tot` does not sit inside
    // `C:\ab`, but its spelling starts with that one's — and a textual trim would say it did,
    // handing back `.tot` under a name that belongs to a different file. This name is the
    // import cache's key, the cycle detector's key, and the directory further imports resolve
    // against, so two files sharing one would build the wrong document without saying so.
    let relative = std::env::current_dir()
        .ok()
        .map(|cwd| std::fs::canonicalize(&cwd).unwrap_or(cwd))
        .and_then(|cwd| full.strip_prefix(&cwd).map(Path::to_path_buf).ok())
        .filter(|rest| !rest.as_os_str().is_empty());

    trim_verbatim(&relative.as_deref().unwrap_or(&full).to_string_lossy())
}

fn trim_verbatim(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

/// Whether two paths name the same file that already exists.
///
/// Canonical, so `a/../b.tot` and `b.tot` are one file. A path that does not exist yet cannot
/// be the one being read, so failing to canonicalize is an honest `false`.
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Whether this path names a document rather than a template.
pub fn is_document(path: &Path) -> bool {
    extension(path).as_deref() == Some("tot")
}

/// Where `tot build` writes, when it was not told: the input with its `.tott` suffix taken off.
///
/// A template and the document built from it are the same configuration in two forms, so they
/// share a name. `--check` compares against this, which is what makes it a one-word CI check.
pub fn output_for(input: &str) -> Option<PathBuf> {
    let path = Path::new(input);
    if extension(path).as_deref() != Some("tott") {
        return None;
    }
    Some(path.with_extension("tot"))
}
