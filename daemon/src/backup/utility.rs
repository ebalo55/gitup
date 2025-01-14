/// Compute the size variation between two sizes.
///
/// # Arguments
///
/// * `size` - The original size
/// * `new_size` - The new size
///
/// # Returns
///
/// The size variation as a string with a percentage.
pub fn compute_size_variation(size: f64, new_size: f64) -> String {
    let size_variation = ((size - new_size) / size) * 100.0 * -1.;
    format!(
        "{}{:.2}%",
        if size_variation >= 0. { "+" } else { "" },
        size_variation
    )
}

/// Replace a string recursively
///
/// # Arguments
///
/// * `haystack` - The string to search in
/// * `needle` - The string to replace
/// * `replace` - The string to replace with
///
/// # Returns
///
/// The string with the replacements
pub fn replace_recursive(haystack: String, needle: &str, replace: &str) -> String {
    let mut result = haystack.clone();
    while result.contains(needle) {
        result = result.replace(needle, replace);
    }
    result
}
