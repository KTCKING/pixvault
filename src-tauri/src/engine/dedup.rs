use std::collections::HashMap;
use std::path::Path;
use crate::engine::db::FileEntry;

/// Duplicate detection engine
/// Uses two strategies:
/// 1. Exact match: BLAKE3 hash (100% identical)
/// 2. Perceptual match: pHash (visually similar, different resolutions/compression)
pub struct DedupEngine {
    /// Threshold for perceptual hash similarity (0-64, lower = stricter)
    phash_threshold: u32,
}

#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub hash: String,
    pub files: Vec<FileEntry>,
    pub match_type: MatchType,
    pub total_size_wasted: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchType {
    Exact,
    Perceptual,
}

impl DedupEngine {
    pub fn new() -> Self {
        DedupEngine {
            phash_threshold: 8, // Hamming distance threshold
        }
    }

    /// Find exact duplicates (same BLAKE3 hash)
    pub fn find_exact_duplicates(&self, files: &[FileEntry]) -> Vec<DuplicateGroup> {
        let mut hash_map: HashMap<&str, Vec<&FileEntry>> = HashMap::new();

        for file in files {
            if let Some(ref hash) = file.blake3_hash {
                hash_map.entry(hash.as_str()).or_default().push(file);
            }
        }

        hash_map
            .into_iter()
            .filter(|(_, group)| group.len() > 1)
            .map(|(hash, group)| {
                let wasted = group.iter().skip(1).map(|f| f.size_bytes).sum();
                DuplicateGroup {
                    hash: hash.to_string(),
                    files: group.into_iter().cloned().collect(),
                    match_type: MatchType::Exact,
                    total_size_wasted: wasted,
                }
            })
            .collect()
    }

    /// Find perceptual duplicates (similar images)
    /// Requires pHash to be already computed for all files
    pub fn find_perceptual_duplicates(&self, files: &[FileEntry]) -> Vec<DuplicateGroup> {
        let mut groups: Vec<DuplicateGroup> = Vec::new();
        let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (i, file_a) in files.iter().enumerate() {
            let hash_a = match &file_a.phash {
                Some(h) => h,
                None => continue,
            };
            if processed.contains(&file_a.path) {
                continue;
            }

            let mut group: Vec<FileEntry> = vec![file_a.clone()];
            for file_b in files.iter().skip(i + 1) {
                let hash_b = match &file_b.phash {
                    Some(h) => h,
                    None => continue,
                };
                if processed.contains(&file_b.path) {
                    continue;
                }

                if let Ok(dist) = Self::hamming_distance(hash_a, hash_b) {
                    if dist <= self.phash_threshold {
                        group.push(file_b.clone());
                        processed.insert(file_b.path.clone());
                    }
                }
            }

            if group.len() > 1 {
                processed.insert(file_a.path.clone());
                let wasted = group.iter().skip(1).map(|f| f.size_bytes).sum();
                groups.push(DuplicateGroup {
                    hash: hash_a.clone(),
                    files: group,
                    match_type: MatchType::Perceptual,
                    total_size_wasted: wasted,
                });
            }
        }

        groups
    }

    /// Compute Hamming distance between two hex-encoded hash strings
    fn hamming_distance(hash_a: &str, hash_b: &str) -> Result<u32, String> {
        if hash_a.len() != hash_b.len() {
            return Err("Hash length mismatch".into());
        }

        let mut distance = 0u32;
        for (a, b) in hash_a.chars().zip(hash_b.chars()) {
            let val_a = u8::from_str_radix(&a.to_string(), 16).unwrap_or(0);
            let val_b = u8::from_str_radix(&b.to_string(), 16).unwrap_or(0);
            distance += (val_a ^ val_b).count_ones();
        }
        Ok(distance)
    }
}