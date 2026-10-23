//! API key handling.
//!
//! The key is 32 random bytes, shared between Throne and the client. Its text
//! form is base64url without padding (43 characters).

use std::fmt;
use std::str::FromStr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;

use crate::error::{Error, Result};
use crate::protocol::{random_array, KEY_SIZE};

/// A 32-byte Throne API key.
///
/// `Debug` never prints the key bytes.
#[derive(Clone)]
pub struct ApiKey([u8; KEY_SIZE]);

impl ApiKey {
    /// Parses the text form of a key: base64url without padding.
    ///
    /// Surrounding whitespace (such as the newline at the end of a key file)
    /// is ignored. Anything else that does not decode to exactly 32 bytes is
    /// rejected, including padding characters.
    pub fn from_text(text: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(text.trim())
            .map_err(|_| Error::InvalidKey)?;
        let array: [u8; KEY_SIZE] = bytes.try_into().map_err(|_| Error::InvalidKey)?;
        Ok(Self(array))
    }

    /// Returns the text form of the key (43 characters, base64url, no padding).
    pub fn to_text(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Generates a new random key from the operating system's RNG.
    pub fn generate() -> Result<Self> {
        Ok(Self(random_array()?))
    }

    /// Returns the raw key bytes.
    pub fn as_bytes(&self) -> &[u8; KEY_SIZE] {
        &self.0
    }
}

impl FromStr for ApiKey {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::from_text(s)
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ApiKey {
        let mut bytes = [0u8; KEY_SIZE];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = i as u8;
        }
        ApiKey(bytes)
    }

    #[test]
    fn text_roundtrip() {
        let key = sample();
        let text = key.to_text();
        assert_eq!(text.len(), 43);
        assert!(!text.contains('='));
        let parsed = ApiKey::from_text(&text).expect("valid key text");
        assert_eq!(parsed.as_bytes(), key.as_bytes());
        let parsed: ApiKey = text.parse().expect("FromStr accepts key text");
        assert_eq!(parsed.as_bytes(), key.as_bytes());
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        let key = sample();
        let text = format!("  {}\r\n", key.to_text());
        let parsed = ApiKey::from_text(&text).expect("whitespace is trimmed");
        assert_eq!(parsed.as_bytes(), key.as_bytes());
    }

    #[test]
    fn wrong_length_is_rejected() {
        let short = URL_SAFE_NO_PAD.encode([7u8; 31]);
        let long = URL_SAFE_NO_PAD.encode([7u8; 33]);
        assert!(matches!(ApiKey::from_text(&short), Err(Error::InvalidKey)));
        assert!(matches!(ApiKey::from_text(&long), Err(Error::InvalidKey)));
        assert!(matches!(ApiKey::from_text(""), Err(Error::InvalidKey)));
    }

    #[test]
    fn padding_is_rejected() {
        let padded = format!("{}=", sample().to_text());
        assert!(matches!(ApiKey::from_text(&padded), Err(Error::InvalidKey)));
    }

    #[test]
    fn standard_base64_alphabet_is_rejected() {
        // '+' and '/' belong to standard base64, not base64url.
        assert!(matches!(
            ApiKey::from_text("+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+/+"),
            Err(Error::InvalidKey)
        ));
    }

    #[test]
    fn debug_redacts_the_key() {
        let key = sample();
        let shown = format!("{key:?}");
        assert_eq!(shown, "ApiKey(<redacted>)");
        assert!(!shown.contains(&key.to_text()));
    }

    #[test]
    fn generate_gives_distinct_keys() {
        let a = ApiKey::generate().expect("OS RNG");
        let b = ApiKey::generate().expect("OS RNG");
        assert_ne!(a.as_bytes(), b.as_bytes());
    }
}
