use std::fmt::{Debug, Display, Formatter};

/// Check if the capacity is too large
///
/// # Arguments
///
/// * `capacity` - The capacity to check
///
/// # Returns
///
/// * `Ok(())` if the capacity is not too large
/// * `Err(CryptError::DataTooLong(capacity))` if the capacity is too large
const fn check_capacity(capacity: (usize, bool)) -> Result<(), CryptError> {
    if capacity.1 {
        Err(CryptError::DataTooLong(capacity.0))
    } else {
        Ok(())
    }
}

/// Create an error for an invalid encoding length
///
/// # Arguments
///
/// * `length` - The length of the encoding
///
/// # Returns
///
/// A `CryptError` for an invalid encoding length
fn make_invalid_encoding_length(length: usize) -> CryptError {
    CryptError::InvalidEncodingLength("base64".to_owned(), length)
}

/// Push a character to the output if it exists in the alphabet
///
/// # Arguments
///
/// * `alphabet` - The alphabet to check the character against
/// * `output` - The output vector to push the character to
/// * `value` - The value to check against the alphabet
///
/// # Returns
///
/// * `Ok(())` if the character was pushed successfully
/// * `Err(CryptError::InvalidCharacterInput)` if the character is not in the alphabet
fn checked_push(alphabet: &[u8], output: &mut Vec<u8>, value: u32) -> Result<(), CryptError> {
    alphabet
        .get(value as usize)
        .map_or(Err(CryptError::InvalidCharacterInput), |value| {
            output.push(*value);
            Ok(())
        })
}

/// Create an error for an invalid number of padding bytes
///
/// # Arguments
///
/// * `length` - The length of the padding bytes
///
/// # Returns
///
/// A `CryptError` for an invalid number of padding bytes
fn make_invalid_padding_bytes_encoding_length(length: usize) -> CryptError {
    CryptError::InvalidEncodingLength("base64 padding bytes".to_owned(), length)
}

#[derive(Clone, PartialEq, Eq)]
pub enum CryptError {
    /// The key length is invalid (expected, received)
    InvalidKeyLength(u8, usize),
    /// The nonce length is invalid (expected, received)
    InvalidNonceLength(u8, usize),
    /// Invalid character in input
    InvalidCharacterInput,
    /// Cannot decode the data
    CannotDecode,
    /// Cannot encode the data
    CannotEncode,
    /// The public key is missing for or invalid for the operation
    MissingOrInvalidPublicKey,
    /// The secret key is missing for or invalid for the operation
    MissingOrInvalidSecretKey,
    /// The provided data is too long, overflowing the maximum size
    DataTooLong(usize),
    /// The provided data is too short, underflowing the minimum size
    DataTooShort(usize),
    /// Invalid encoding character (encoding, character)
    InvalidEncodingCharacter(String, char),
    /// Invalid encoding length (encoding, length)
    InvalidEncodingLength(String, usize),
    /// The internal encoding bitmask overflowed
    EncodingBitmaskOverflow(usize),
}

impl Debug for CryptError {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        // Delegate to Display
        write!(f, "{}", self)
    }
}

impl Display for CryptError {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        #[expect(
            clippy::pattern_type_mismatch,
            reason = "Cannot dereference into the Display trait implementation"
        )]
        match self {
            Self::InvalidKeyLength(bytes, received) => {
                write!(
                    f,
                    "Invalid key length, expected {} bytes, got {}",
                    bytes, received
                )
            }
            Self::InvalidNonceLength(bytes, received) => {
                write!(
                    f,
                    "Invalid nonce length, expected {} bytes, got {}",
                    bytes, received
                )
            }
            Self::InvalidCharacterInput => {
                write!(f, "Invalid character in input")
            }
            Self::CannotDecode => {
                write!(f, "Cannot decode the data")
            }
            Self::MissingOrInvalidPublicKey => {
                write!(f, "The receiver public key is missing or invalid")
            }
            Self::MissingOrInvalidSecretKey => {
                write!(f, "The sender secret key is missing or invalid")
            }
            Self::DataTooLong(overflowing_size) => {
                write!(
                    f,
                    "The provided data is too long, overflowing the maximum size resulting in: {}",
                    overflowing_size
                )
            }
            Self::DataTooShort(underflowing_size) => {
                write!(
                    f,
                    "The provided data is too short, underflowing the minimum size resulting in: {}",
                    underflowing_size
                )
            }
            Self::InvalidEncodingCharacter(encoding, char) => {
                write!(
                    f,
                    "Invalid character in input for encoding '{}': '{}'",
                    encoding, char
                )
            }
            Self::InvalidEncodingLength(encoding, size) => {
                write!(
                    f,
                    "Invalid length in input for encoding '{}': '{}'",
                    encoding, size
                )
            }
            Self::CannotEncode => {
                write!(f, "Cannot encode the data")
            }
            Self::EncodingBitmaskOverflow(bitmask) => {
                write!(f, "The internal encoding bitmask overflowed: {}", bitmask)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Variant {
    Url,
    #[default]
    Standard,
}

impl Variant {
    fn get_alphabet(&self) -> &'static [u8] {
        match *self {
            Self::Url => b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
            Self::Standard => b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
        }
    }

    fn get_padding(&self) -> Option<u8> {
        match *self {
            Self::Url => None,
            Self::Standard => Some(b'='),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Encoder {
    /// Which variant of base64 to use
    variant: Variant,
}

impl Encoder {
    pub const fn new(variant: Variant) -> Self {
        Self {
            variant,
        }
    }

    pub fn encode(&self, data: &[u8]) -> Result<String, CryptError> {
        let alphabet = self.variant.get_alphabet();
        let padding = self.variant.get_padding();

        let capacity = data.len().overflowing_mul(4);
        check_capacity(capacity)?;

        let capacity = capacity.0.overflowing_add(2);
        check_capacity(capacity)?;

        let capacity = capacity.0.overflowing_div(3);
        check_capacity(capacity)?;

        let mut output = Vec::with_capacity(capacity.0);

        let mut chunks = data.chunks_exact(3);

        // Process each chunk of 3 bytes
        for chunk in &mut chunks {
            let n = chunk.first().map_or_else(
                || Err(make_invalid_encoding_length(chunk.len())),
                |first| {
                    chunk.get(1).map_or_else(
                        || Err(make_invalid_encoding_length(chunk.len())),
                        |second| {
                            chunk.get(2).map_or_else(
                                || Err(make_invalid_encoding_length(chunk.len())),
                                |third| Ok(((*first as u32) << 16) | ((*second as u32) << 8) | (*third as u32)),
                            )
                        },
                    )
                },
            )?;

            checked_push(alphabet, &mut output, (n >> 18) & 0x3f)?;
            checked_push(alphabet, &mut output, (n >> 12) & 0x3f)?;
            checked_push(alphabet, &mut output, (n >> 6) & 0x3f)?;
            checked_push(alphabet, &mut output, n & 0x3f)?;
        }

        // Handle remaining bytes (if any)
        let rem = chunks.remainder();
        if !rem.is_empty() {
            if rem.len() > 2 {
                return Err(make_invalid_padding_bytes_encoding_length(rem.len()));
            }

            let n = if rem.len() == 1 {
                rem.first().map_or_else(
                    || Err(make_invalid_padding_bytes_encoding_length(rem.len())),
                    |first| Ok((*first as u32) << 16),
                )
            } else {
                rem.first().map_or_else(
                    || Err(make_invalid_padding_bytes_encoding_length(rem.len())),
                    |first| {
                        rem.get(1).map_or_else(
                            || Err(make_invalid_padding_bytes_encoding_length(rem.len())),
                            |second| Ok(((*first as u32) << 16) | ((*second as u32) << 8)),
                        )
                    },
                )
            }?;

            checked_push(alphabet, &mut output, (n >> 18) & 0x3f)?;
            checked_push(alphabet, &mut output, (n >> 12) & 0x3f)?;

            if rem.len() == 2 {
                checked_push(alphabet, &mut output, (n >> 6) & 0x3f)?;
            } else if let Some(pad) = padding {
                output.push(pad);
            }

            if let Some(pad) = padding {
                output.push(pad);
            }
        }

        Ok(String::from_utf8(output).unwrap())
    }

    pub fn decode(&self, data: &str) -> Result<Vec<u8>, CryptError> {
        let alphabet = self.variant.get_alphabet();
        let padding = self.variant.get_padding();
        let bytes = data.as_bytes();

        let capacity = bytes.len().overflowing_mul(3);
        if capacity.1 {
            return Err(CryptError::DataTooLong(capacity.0));
        }

        let capacity = capacity.0.overflowing_div(4);
        if capacity.1 {
            return Err(CryptError::InvalidEncodingLength(
                "base64".to_owned(),
                bytes.len(),
            ));
        }
        let mut output = Vec::with_capacity(capacity.0);

        let mut buffer = [0u32; 4];
        let mut index = 0u8;

        for &byte in bytes {
            if Some(byte) == padding || byte == b'=' {
                break;
            }

            let value = alphabet
                .iter()
                .position(|&c| c == byte)
                .ok_or(CryptError::InvalidEncodingCharacter(
                    "base64".to_owned(),
                    byte as char,
                ))? as u32;

            if let Some(buffer_position) = buffer.get_mut(index as usize) {
                *buffer_position = value;
            } else {
                return Err(CryptError::InvalidEncodingLength(
                    "base64".to_owned(),
                    bytes.len(),
                ));
            }
            index = index.saturating_add(1);

            if index == 4 {
                let n = (buffer[0] << 18) | (buffer[1] << 12) | (buffer[2] << 6) | buffer[3];
                output.push(((n >> 16) & 0xff) as u8);
                output.push(((n >> 8) & 0xff) as u8);
                output.push((n & 0xff) as u8);
                index = 0;
            }
        }

        // Handle remaining bytes
        if index > 1 {
            let n = (buffer[0] << 18) | (buffer[1] << 12);
            output.push(((n >> 16) & 0xff) as u8);

            if index == 3 {
                let n = n | (buffer[2] << 6);
                output.push(((n >> 8) & 0xff) as u8);
            }
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode_url() {
        let data = b"Hello, World!".to_vec();

        let encoder = Encoder::new(Variant::Url);
        let encoded = encoder.encode(data.as_slice()).unwrap();
        assert_eq!(encoded, "SGVsbG8sIFdvcmxkIQ");

        // no_std_println!("Base64 URL: {}", encoded);
    }

    #[test]
    fn test_base64_decode_url() {
        let data = "SGVsbG8sIFdvcmxkIQ==";

        let encoder = Encoder::new(Variant::Url);
        let decoded = encoder.decode(data).unwrap();
        assert_eq!(decoded, b"Hello, World!".to_vec());
    }
    #[test]
    fn test_base64_encode() {
        let data = b"Hello, World!".to_vec();

        let encoder = Encoder::new(Variant::Standard);
        let encoded = encoder.encode(data.as_slice()).unwrap();
        assert_eq!(encoded, "SGVsbG8sIFdvcmxkIQ==");

        // no_std_println!("Base64: {}", encoded);
    }

    #[test]
    fn test_base64_decode() {
        let data = "SGVsbG8sIFdvcmxkIQ==";

        let encoder = Encoder::new(Variant::Standard);
        let decoded = encoder.decode(data).unwrap();
        assert_eq!(decoded, b"Hello, World!".to_vec());
    }
}
