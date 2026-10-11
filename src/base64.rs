//! Minimal, dependency-free Base64 (RFC 4648 standard alphabet, padded).
//!
//! Used to embed binary glTF/GLB model data inside the text mission/plane files
//! and the text wire protocol.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes to a padded Base64 string.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decode a Base64 string. Whitespace is ignored; `=` padding is optional.
/// Returns `None` on any invalid character.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for ch in text.chars() {
        if ch == '=' || ch.is_ascii_whitespace() {
            continue;
        }
        let value = decode_char(ch)?;
        buffer = (buffer << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

fn decode_char(ch: char) -> Option<u8> {
    let index = match ch {
        'A'..='Z' => ch as u8 - b'A',
        'a'..='z' => ch as u8 - b'a' + 26,
        '0'..='9' => ch as u8 - b'0' + 52,
        '+' => 62,
        '/' => 63,
        _ => return None,
    };
    Some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rfc4648_vectors() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn round_trips_arbitrary_bytes() {
        for len in 0..64 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let encoded = encode(&bytes);
            assert_eq!(decode(&encoded).as_deref(), Some(bytes.as_slice()));
        }
    }

    #[test]
    fn rejects_invalid_characters() {
        assert!(decode("Zm9v!").is_none());
        assert!(decode("Zm9v_").is_none()); // `_` is the URL-safe alphabet, not ours
    }

    #[test]
    fn ignores_whitespace_and_padding() {
        assert_eq!(decode("Zm9v\n YmFy").as_deref(), Some(b"foobar".as_slice()));
        assert_eq!(decode("Zg").as_deref(), Some(b"f".as_slice()));
    }
}
