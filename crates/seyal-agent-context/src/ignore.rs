//! Minimal `.gitignore`-style matcher (discovery hint; not a security boundary).

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct IgnoreRules {
    /// Patterns relative to the directory that owns the ignore file.
    patterns: Vec<IgnorePattern>,
}

#[derive(Clone, Debug)]
struct IgnorePattern {
    /// Directory containing the `.gitignore` that defined this pattern.
    base: PathBuf,
    negated: bool,
    directory_only: bool,
    /// Glob-ish pattern without leading `!` / trailing `/`.
    pattern: String,
}

impl IgnoreRules {
    pub fn load_from_repo(root: &Path) -> Self {
        let mut rules = Self::default();
        // Root .gitignore only for the permanent discovery path's default exclusion.
        let gitignore = root.join(".gitignore");
        if let Ok(text) = std::fs::read_to_string(&gitignore) {
            rules.extend_from_text(root, &text);
        }
        rules
    }

    pub fn extend_from_text(&mut self, base: &Path, text: &str) {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let mut pattern = trimmed;
            let mut negated = false;
            if let Some(rest) = pattern.strip_prefix('!') {
                negated = true;
                pattern = rest;
            }
            let mut directory_only = false;
            if let Some(rest) = pattern.strip_suffix('/') {
                directory_only = true;
                pattern = rest;
            }
            self.patterns.push(IgnorePattern {
                base: base.to_path_buf(),
                negated,
                directory_only,
                pattern: pattern.to_string(),
            });
        }
    }

    /// Last matching pattern wins (gitignore semantics).
    pub fn is_ignored(&self, absolute: &Path, is_dir: bool) -> bool {
        let mut ignored = false;
        for rule in &self.patterns {
            if let Ok(rel) = absolute.strip_prefix(&rule.base) {
                if rule.directory_only && !is_dir {
                    continue;
                }
                if match_pattern(&rule.pattern, rel) {
                    ignored = !rule.negated;
                }
            }
        }
        ignored
    }
}

fn match_pattern(pattern: &str, relative: &Path) -> bool {
    let rel = relative.to_string_lossy().replace('\\', "/");
    if pattern.contains('/') {
        return glob_match(pattern.trim_start_matches('/'), &rel);
    }
    // Basename match anywhere in the tree.
    relative
        .file_name()
        .map(|name| glob_match(pattern, &name.to_string_lossy()))
        .unwrap_or(false)
        || glob_match(pattern, &rel)
        || rel.split('/').any(|part| glob_match(pattern, part))
}

fn glob_match(pattern: &str, text: &str) -> bool {
    // Restricted `*` / `**` / exact support sufficient for discovery fixtures.
    if pattern == "**" || pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == text || text.ends_with(&format!("/{pattern}"));
    }
    if let Some(suffix) = pattern.strip_prefix("**/") {
        return text == suffix
            || text.ends_with(&format!("/{suffix}"))
            || text.contains(&format!("/{suffix}/"))
            || text.starts_with(&format!("{suffix}/"));
    }
    if let Some((pre, post)) = pattern.split_once('*') {
        return text.starts_with(pre)
            && text.ends_with(post)
            && text.len() >= pre.len() + post.len();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitignore_basename_and_negation() {
        let mut rules = IgnoreRules::default();
        rules.extend_from_text(Path::new("/repo"), "*.log\n!keep.log\nbuild/\n");
        assert!(rules.is_ignored(Path::new("/repo/a.log"), false));
        assert!(!rules.is_ignored(Path::new("/repo/keep.log"), false));
        assert!(rules.is_ignored(Path::new("/repo/build"), true));
    }
}
