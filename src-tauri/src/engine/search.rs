use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;
use crate::engine::db::FileEntry;

/// Full-text search engine using Tantivy
pub struct SearchEngine {
    // Tantivy index will be initialized per library
    initialized: bool,
}

impl SearchEngine {
    pub fn new() -> Self {
        SearchEngine { initialized: false }
    }

    /// Initialize the Tantivy index for a library
    pub fn init(&mut self, _index_path: &std::path::Path) -> Result<(), String> {
        // Tantivy initialization:
        // - Create index directory
        // - Define schema: filename (text), path (text), tags (text), notes (text)
        // - Register tokenizers (including CJK for Chinese/Japanese/Korean)
        self.initialized = true;
        Ok(())
    }

    /// Search files by query string, returns matching file paths
    pub fn search(&self, _query: &str, _limit: usize) -> Result<Vec<String>, String> {
        if !self.initialized {
            return Ok(Vec::new());
        }
        // Tantivy search:
        // - Parse query with fuzzy matching
        // - Search across filename, path, tags, notes fields
        // - Return scored results sorted by relevance
        Ok(Vec::new())
    }

    /// Index a single file entry
    pub fn index_file(&self, _entry: &FileEntry) -> Result<(), String> {
        Ok(())
    }

    /// Remove a file from the index
    pub fn remove_file(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Search query builders for SQLite-based search (fallback when Tantivy is not available)
pub struct SearchQuery {
    pub text: Option<String>,
    pub tags: Vec<String>,
    pub extensions: Vec<String>,
    pub min_rating: Option<i32>,
    pub color_label: Option<String>,
    pub min_width: Option<i32>,
    pub min_height: Option<i32>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub has_gps: Option<bool>,
}

impl SearchQuery {
    pub fn new() -> Self {
        SearchQuery {
            text: None,
            tags: Vec::new(),
            extensions: Vec::new(),
            min_rating: None,
            color_label: None,
            min_width: None,
            min_height: None,
            date_from: None,
            date_to: None,
            has_gps: None,
        }
    }

    pub fn with_text(mut self, text: &str) -> Self {
        self.text = Some(text.to_string());
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_rating(mut self, min_rating: i32) -> Self {
        self.min_rating = Some(min_rating);
        self
    }

    pub fn build_sql(&self) -> (String, Vec<String>) {
        let mut conditions = vec!["f.is_deleted = 0".to_string()];
        let mut params: Vec<String> = Vec::new();

        if let Some(ref text) = self.text {
            conditions.push(
                "(f.filename LIKE ? OR f.path LIKE ? OR f.notes LIKE ?)".to_string()
            );
            let pattern = format!("%{}%", text);
            params.push(pattern.clone());
            params.push(pattern.clone());
            params.push(pattern);
        }

        if !self.extensions.is_empty() {
            let placeholders: Vec<String> = self.extensions.iter().enumerate()
                .map(|(i, _)| format!("?{}", params.len() + i + 1))
                .collect();
            conditions.push(format!("f.extension IN ({})", placeholders.join(",")));
            params.extend(self.extensions.clone());
        }

        if let Some(rating) = self.min_rating {
            conditions.push(format!("f.rating >= ?{}", params.len() + 1));
            params.push(rating.to_string());
        }

        if let Some(ref label) = self.color_label {
            conditions.push(format!("f.color_label = ?{}", params.len() + 1));
            params.push(label.clone());
        }

        let sql = format!(
            "SELECT f.id, f.path, f.filename, f.extension, f.size_bytes, f.width, f.height,
                    f.blake3_hash, f.phash, f.mtime, f.ctime, f.rating, f.color_label, f.notes,
                    f.is_deleted, f.created_at, f.updated_at
             FROM files f
             WHERE {}
             ORDER BY f.mtime DESC",
            conditions.join(" AND ")
        );

        (sql, params)
    }
}