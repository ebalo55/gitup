use sha3::{Digest, Sha3_256};

/// Hashes the data using the SHA3 algorithm
pub fn sha3(data: &[u8]) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    let result = hasher.finalize();

    format!("{:x}", result)
}
