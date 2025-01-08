/// The size of a kilobyte
pub static KILOBYTE: u64 = 1024;
/// The size of a megabyte
pub static MEGABYTE: u64 = 1024 * KILOBYTE;
/// The size of a gigabyte
pub static GIGABYTE: u64 = 1024 * MEGABYTE;

/// Format a byte size into a human-readable string
///
/// # Arguments
///
/// * `size` - The size in bytes
///
/// # Returns
///
/// A human-readable string representing the size
pub fn format_bytesize(size: u64) -> String {
    if size < KILOBYTE {
        return format!("{} B", size);
    } else if size < MEGABYTE {
        return format!("{:.2} KB", size as f64 / KILOBYTE as f64);
    } else if size < GIGABYTE {
        return format!("{:.2} MB", size as f64 / MEGABYTE as f64);
    }

    format!("{:.2} GB", size as f64 / GIGABYTE as f64)
}
