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
