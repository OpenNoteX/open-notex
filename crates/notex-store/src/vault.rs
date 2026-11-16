//! The Markdown vault: note bodies live here, not in SQLite.
//!
//! Writes are atomic (temporary file in the same directory, fsync, rename), so a
//! crash leaves either the old file or the new one. Frontmatter parsing keeps
//! unknown keys so a note edited by another tool is not rewritten lossily.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use thiserror::Error;

use notex_core::ItemId;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("frontmatter is not terminated: {0}")]
    UnterminatedFrontmatter(String),
    #[error("invalid frontmatter line: {0}")]
    InvalidLine(String),
    #[error("note has no id and could not be adopted: {0}")]
    Adoption(String),
}

/// Ordered frontmatter entries. Values are kept as raw text so keys this app
/// does not understand survive a save unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    entries: Vec<(String, String)>,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Replace in place, or insert. `id` always goes first so a file written by
    /// us is easy to recognise.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value;
            return;
        }
        if key == "id" {
            self.entries.insert(0, (key.to_owned(), value));
        } else {
            self.entries.push((key.to_owned(), value));
        }
    }

    pub fn tags(&self) -> Vec<String> {
        self.get("tags")
            .map(parse_list)
            .unwrap_or_default()
            .into_iter()
            .map(|tag| tag.trim_start_matches('#').to_owned())
            .filter(|tag| !tag.is_empty())
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    fn parse_block(block: &str) -> Result<Self, VaultError> {
        let mut entries = Vec::new();
        for raw in block.lines() {
            let line = raw.trim_end_matches('\r').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                return Err(VaultError::InvalidLine(line.to_owned()));
            };
            let key = key.trim();
            if key.is_empty() {
                return Err(VaultError::InvalidLine(line.to_owned()));
            }
            entries.push((key.to_owned(), value.trim().to_owned()));
        }
        Ok(Self { entries })
    }

    pub fn render(&self, body: &str) -> String {
        if self.entries.is_empty() {
            return body.to_owned();
        }
        let mut out = String::with_capacity(body.len() + 64);
        out.push_str("---\n");
        for (key, value) in &self.entries {
            out.push_str(key);
            out.push_str(": ");
            out.push_str(value);
            out.push('\n');
        }
        out.push_str("---\n");
        out.push_str(body.trim_start_matches('\n'));
        out
    }
}

fn parse_list(value: &str) -> Vec<String> {
    let value = value.trim();
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    inner
        .split(',')
        .map(|item| item.trim().trim_matches('"').trim_matches('\'').to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Split a source file into frontmatter text and body.
pub fn split_frontmatter(src: &str) -> Result<(Option<&str>, &str), VaultError> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);

    let rest = if let Some(rest) = src.strip_prefix("---\n") {
        rest
    } else if let Some(rest) = src.strip_prefix("---\r\n") {
        rest
    } else {
        return Ok((None, src));
    };

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" || trimmed == "..." {
            return Ok((
                Some(&rest[..offset]),
                rest[offset + line.len()..].trim_start_matches('\n'),
            ));
        }
        offset += line.len();
    }

    Err(VaultError::UnterminatedFrontmatter(src.to_owned()))
}

pub fn parse_note_source(src: &str) -> Result<(Frontmatter, String), VaultError> {
    let (block, body) = split_frontmatter(src)?;
    let frontmatter = match block {
        Some(block) => Frontmatter::parse_block(block)?,
        None => Frontmatter::default(),
    };
    Ok((frontmatter, body.to_owned()))
}

/// A parsed note: the indexable metadata plus its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteFile {
    pub path: PathBuf,
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub hash: u64,
    pub mtime_ns: i64,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub file: NoteFile,
    pub body: String,
    /// True when the file had no `id` and we wrote one into it.
    pub adopted: bool,
}

/// Read a note, adopting it (writing an `id`) if frontmatter has none.
pub fn read_note(path: &Path) -> Result<Note, VaultError> {
    let mut source = fs::read_to_string(path)?;
    let (mut frontmatter, body) = parse_note_source(&source)?;

    let adopted = frontmatter.get("id").is_none();
    if adopted {
        let id = ItemId::new();
        frontmatter.set("id", id.to_string());
        source = frontmatter.render(&body);
        write_atomic(path, &source)?;
    }

    let id = frontmatter
        .get("id")
        .ok_or_else(|| VaultError::Adoption(path.display().to_string()))?
        .to_owned();

    let metadata = fs::metadata(path)?;
    let mtime_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);

    let title = title_of(&body).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Untitled")
            .to_owned()
    });

    Ok(Note {
        file: NoteFile {
            path: path.to_path_buf(),
            id,
            title,
            tags: frontmatter.tags(),
            hash: content_hash(source.as_bytes()),
            mtime_ns,
            size: source.len() as i64,
        },
        body,
        adopted,
    })
}

/// First `# Heading`, else `None`.
pub fn title_of(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|title| title.trim().to_owned())
        .filter(|title| !title.is_empty())
}

/// FNV-1a 64. A change detector for the index, not a security primitive, and
/// deliberately stable across platforms and runs (unlike `DefaultHasher`).
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Write a file so that readers see the old or the new contents, never a partial
/// write: temp file in the same directory, fsync, rename.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), VaultError> {
    let parent = path.parent().ok_or_else(|| {
        VaultError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "note has no parent directory",
        ))
    })?;
    fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(contents.as_bytes())?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// Every `.md` file under `root`, depth-first, skipping dot-directories such as
/// `.git` and `.obsidian`.
pub fn scan_paths(root: &Path) -> Result<Vec<PathBuf>, VaultError> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(VaultError::Io(error)),
        };

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && name.ends_with(".md") {
                found.push(path);
            }
        }
    }

    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn frontmatter_round_trips_unknown_keys() {
        let src = "\u{feff}---
id: 01ARZ3NDEKTSV4RRFFQ69G5FAV
tags: [work, review]
aliases: [planning, weekly]
cssclasses: wide
---
# Weekly review

Body text stays as written.
";
        let (frontmatter, body) = parse_note_source(src).unwrap();
        assert_eq!(frontmatter.tags(), vec!["work", "review"]);
        assert_eq!(frontmatter.get("cssclasses"), Some("wide"));
        assert!(body.starts_with("# Weekly review"));

        let rendered = frontmatter.render(&body);
        let (again, body_again) = parse_note_source(&rendered).unwrap();
        assert_eq!(again, frontmatter);
        assert_eq!(body_again, body);
    }

    #[test]
    fn reading_a_note_adopts_a_missing_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ideas.md");
        fs::write(&path, "# Ideas\n\n- one\n").unwrap();

        let note = read_note(&path).unwrap();
        assert!(note.adopted);
        assert_eq!(note.file.title, "Ideas");
        assert_eq!(note.body, "# Ideas\n\n- one\n");

        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.starts_with("---\nid: "));
        assert!(saved.ends_with("# Ideas\n\n- one\n"));

        let second = read_note(&path).unwrap();
        assert!(!second.adopted);
        assert_eq!(second.file.id, note.file.id);
    }

    #[test]
    fn atomic_write_replaces_content_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("note.md");
        write_atomic(&path, "first").unwrap();
        write_atomic(&path, "second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != "note.md")
            .collect();
        assert!(leftovers.is_empty(), "no temp files remain");
    }

    #[test]
    fn scan_skips_hidden_directories_and_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join("notes/deep")).unwrap();
        fs::write(root.join("a.md"), "# A\n").unwrap();
        fs::write(root.join("notes/deep/b.md"), "# B\n").unwrap();
        fs::write(root.join(".obsidian/config.md"), "# no\n").unwrap();
        fs::write(root.join("image.png"), "no").unwrap();

        let paths = scan_paths(root).unwrap();
        let names: Vec<_> = paths
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["a.md", "notes/deep/b.md"]);
    }

    #[test]
    fn unterminated_frontmatter_is_an_error_not_a_silent_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.md");
        fs::write(&path, "---\nid: x\n# no terminator\n").unwrap();
        assert!(matches!(
            read_note(&path),
            Err(VaultError::UnterminatedFrontmatter(_))
        ));
        // The file is untouched.
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "---\nid: x\n# no terminator\n"
        );
    }

    #[test]
    fn crlf_frontmatter_parses() {
        let src = "---\r\nid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\r\ntags: [a]\r\n---\r\n# T\r\n";
        let (frontmatter, body) = parse_note_source(src).unwrap();
        assert_eq!(frontmatter.tags(), vec!["a"]);
        assert!(body.starts_with("# T"));
    }

    #[test]
    fn hash_changes_with_content_and_is_stable() {
        assert_eq!(content_hash(b"abc"), content_hash(b"abc"));
        assert_ne!(content_hash(b"abc"), content_hash(b"abd"));
    }
}
