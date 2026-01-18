use argon2::{Algorithm, Argon2, Params, Version};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::error::{EasyPasswordError, Result};

const LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

const ARGON2_M_COST: u32 = 19456;
const ARGON2_T_COST: u32 = 2;
const ARGON2_P_COST: u32 = 1;
const ENTROPY_BYTES: usize = 64;

#[derive(Debug, Clone)]
pub struct PasswordConfig {
    pub length: usize,
    pub use_lowercase: bool,
    pub use_uppercase: bool,
    pub use_digits: bool,
    pub use_symbols: bool,
}

impl Default for PasswordConfig {
    fn default() -> Self {
        Self {
            length: 16,
            use_lowercase: true,
            use_uppercase: true,
            use_digits: true,
            use_symbols: true,
        }
    }
}

impl PasswordConfig {
    fn build_charset(&self) -> Vec<char> {
        let mut charset = Vec::new();
        if self.use_lowercase {
            charset.extend(LOWERCASE.chars());
        }
        if self.use_uppercase {
            charset.extend(UPPERCASE.chars());
        }
        if self.use_digits {
            charset.extend(DIGITS.chars());
        }
        if self.use_symbols {
            charset.extend(SYMBOLS.chars());
        }
        charset
    }

    fn count_enabled_charsets(&self) -> usize {
        [
            self.use_lowercase,
            self.use_uppercase,
            self.use_digits,
            self.use_symbols,
        ]
        .iter()
        .filter(|&&x| x)
        .count()
    }
}

pub fn generate_password(
    master_key: &str,
    site: &str,
    counter: u32,
    config: &PasswordConfig,
) -> Result<String> {
    let charset = config.build_charset();
    if charset.is_empty() {
        return Err(EasyPasswordError::PasswordGeneration(
            "At least one character class must be enabled".to_string(),
        ));
    }

    let site_normalized = site.to_lowercase();
    let salt = build_salt(&site_normalized, counter);
    let mut entropy = derive_entropy(master_key, &salt)?;
    let password = render_password(&entropy, &charset, config);
    entropy.zeroize();

    Ok(password)
}

fn build_salt(site: &str, counter: u32) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(site.as_bytes());
    hasher.update(counter.to_le_bytes());
    hasher.finalize().to_vec()
}

fn derive_entropy(master_key: &str, salt: &[u8]) -> Result<Vec<u8>> {
    let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(ENTROPY_BYTES))
        .map_err(|e| EasyPasswordError::PasswordGeneration(e.to_string()))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut entropy = vec![0u8; ENTROPY_BYTES];

    argon2
        .hash_password_into(master_key.as_bytes(), salt, &mut entropy)
        .map_err(|e| EasyPasswordError::PasswordGeneration(e.to_string()))?;

    Ok(entropy)
}

fn render_password(entropy: &[u8], charset: &[char], config: &PasswordConfig) -> String {
    let mut quotient = bytes_to_big_uint(entropy);
    let charset_len = charset.len() as u128;
    let required_count = config.count_enabled_charsets();
    let base_length = config.length.saturating_sub(required_count);

    let mut password: Vec<char> = Vec::with_capacity(config.length);

    for _ in 0..base_length {
        let (new_quotient, remainder) = div_mod(quotient, charset_len);
        password.push(charset[remainder as usize]);
        quotient = new_quotient;
    }

    let mut required_chars = Vec::new();
    if config.use_lowercase {
        let (new_quotient, remainder) = div_mod(quotient, LOWERCASE.len() as u128);
        required_chars.push(LOWERCASE.chars().nth(remainder as usize).unwrap());
        quotient = new_quotient;
    }
    if config.use_uppercase {
        let (new_quotient, remainder) = div_mod(quotient, UPPERCASE.len() as u128);
        required_chars.push(UPPERCASE.chars().nth(remainder as usize).unwrap());
        quotient = new_quotient;
    }
    if config.use_digits {
        let (new_quotient, remainder) = div_mod(quotient, DIGITS.len() as u128);
        required_chars.push(DIGITS.chars().nth(remainder as usize).unwrap());
        quotient = new_quotient;
    }
    if config.use_symbols {
        let (new_quotient, remainder) = div_mod(quotient, SYMBOLS.len() as u128);
        required_chars.push(SYMBOLS.chars().nth(remainder as usize).unwrap());
        quotient = new_quotient;
    }

    for ch in required_chars {
        if password.is_empty() {
            password.push(ch);
        } else {
            let (new_quotient, pos) = div_mod(quotient, password.len() as u128);
            password.insert(pos as usize, ch);
            quotient = new_quotient;
        }
    }

    password.into_iter().collect()
}

fn bytes_to_big_uint(bytes: &[u8]) -> u128 {
    let mut result: u128 = 0;
    for &byte in bytes.iter().take(16) {
        result = (result << 8) | (byte as u128);
    }
    result
}

fn div_mod(dividend: u128, divisor: u128) -> (u128, u128) {
    (dividend / divisor, dividend % divisor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_generation() {
        let config = PasswordConfig::default();
        let pw1 = generate_password("master", "github.com", 1, &config).unwrap();
        let pw2 = generate_password("master", "github.com", 1, &config).unwrap();
        assert_eq!(pw1, pw2);
    }

    #[test]
    fn test_different_sites_different_passwords() {
        let config = PasswordConfig::default();
        let pw1 = generate_password("master", "github.com", 1, &config).unwrap();
        let pw2 = generate_password("master", "google.com", 1, &config).unwrap();
        assert_ne!(pw1, pw2);
    }

    #[test]
    fn test_case_insensitive_site() {
        let config = PasswordConfig::default();
        let pw1 = generate_password("master", "GitHub.com", 1, &config).unwrap();
        let pw2 = generate_password("master", "github.com", 1, &config).unwrap();
        assert_eq!(pw1, pw2);
    }

    #[test]
    fn test_password_length() {
        let config = PasswordConfig {
            length: 20,
            ..Default::default()
        };
        let pw = generate_password("master", "test.com", 1, &config).unwrap();
        assert_eq!(pw.len(), 20);
    }

    #[test]
    fn test_counter_changes_password() {
        let config = PasswordConfig::default();
        let pw1 = generate_password("master", "github.com", 1, &config).unwrap();
        let pw2 = generate_password("master", "github.com", 2, &config).unwrap();
        assert_ne!(pw1, pw2);
    }
}
