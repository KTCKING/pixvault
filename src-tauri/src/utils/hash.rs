use std::path::Path;
use std::io::Read;

/// Compute BLAKE3 hash of a file
/// BLAKE3 is 10x faster than SHA-256 and designed for large files
pub fn blake3_hash(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 65536]; // 64KB buffer

    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Compute BLAKE3 hash of bytes (for thumbnails, etc.)
pub fn blake3_hash_bytes(data: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(data);
    hasher.finalize().to_hex().to_string()
}

/// Compute a simple perceptual hash (pHash) for an image
/// Uses DCT-based approach similar to pHash algorithm
pub fn perceptual_hash(path: &Path) -> Result<String, String> {
    let img = image::ImageReader::open(path)
        .map_err(|e| format!("Cannot open image: {}", e))?
        .decode()
        .map_err(|e| format!("Cannot decode: {}", e))?;

    // Resize to 32x32 grayscale
    let small = img.resize_exact(32, 32, image::imageops::FilterType::Lanczos3);
    let gray = small.to_luma8();

    // Compute DCT (simplified: just average-based hash for now)
    let pixels: Vec<u8> = gray.pixels().map(|p| p.0[0]).collect();
    let avg = pixels.iter().map(|&p| p as u64).sum::<u64>() / pixels.len() as u64;

    // Build hash: each bit represents whether pixel is above average
    let mut hash = String::with_capacity(64);
    for (i, &pixel) in pixels.iter().enumerate() {
        if i % 16 == 0 && i > 0 {
            hash.push(' ');
        }
        hash.push(if pixel as u64 > avg { '1' } else { '0' });
    }

    // Convert binary string to hex
    let binary_str: String = pixels.iter()
        .map(|&p| if p as u64 > avg { '1' } else { '0' })
        .collect();
    
    let hex: String = binary_str
        .as_bytes()
        .chunks(4)
        .map(|chunk| {
            let nibble = std::str::from_utf8(chunk).unwrap_or("0000");
            let val = u8::from_str_radix(nibble, 2).unwrap_or(0);
            format!("{:x}", val)
        })
        .collect();

    Ok(hex)
}