//! Resolving `(import …)` against the filesystem.

use std::path::{Path, PathBuf};

use tot::Dialect;
use tot::template::{Imports, Loaded};

/// Reads imports from disk, **relative to the file doing the importing**.
///
/// Relative to the importer rather than to where `tot` was invoked is the only answer that
/// makes a fragment relocatable: a directory of templates that import each other keeps working
/// wherever it is checked out, and whatever directory the build is run from.
pub struct Files;

impl Imports for Files {
    fn load(&mut self, from: &str, target: &str) -> Result<Loaded, String> {
        let base = Path::new(from).parent().unwrap_or_else(|| Path::new(""));
        let path = base.join(target);
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot import `{target}`: {}: {e}", path.display()))?;
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
    match path.extension().and_then(|e| e.to_str()) {
        Some("tott") => Dialect::Template,
        _ => Dialect::Data,
    }
}

/// What to call a file, in diagnostics and for deciding whether two imports are the same file.
///
/// Canonical, so that `a/../b.tot` and `b.tot` are one file and a cycle through them is caught;
/// then trimmed back to something a reader recognizes — relative to the working directory where
/// it can be, and without the `\\?\` prefix Windows canonicalization adds.
pub fn name(path: &Path) -> String {
    let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let full = trim_verbatim(&full.to_string_lossy());

    let Ok(cwd) = std::env::current_dir() else {
        return full;
    };
    let cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    let cwd = trim_verbatim(&cwd.to_string_lossy());

    match full.strip_prefix(&cwd) {
        Some(rest) if !rest.is_empty() => rest.trim_start_matches(['/', '\\']).to_string(),
        _ => full,
    }
}

fn trim_verbatim(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

/// Where `tot build` writes, when it was not told: the input with its `.tott` suffix taken off.
///
/// A template and the document built from it are the same configuration in two forms, so they
/// share a name. `--check` compares against this, which is what makes it a one-word CI check.
pub fn output_for(input: &str) -> Option<PathBuf> {
    let path = Path::new(input);
    if path.extension().and_then(|e| e.to_str()) != Some("tott") {
        return None;
    }
    Some(path.with_extension("tot"))
}
