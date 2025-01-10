/// Pad a number with zeros to a certain length
///
/// # Arguments
///
/// * `num` - The number to pad
/// * `length` - The length of the padded number
///
/// # Returns
///
/// The padded number
pub fn pad_number(num: u32, length: usize) -> String { format!("{:0width$}", num, width = length) }
