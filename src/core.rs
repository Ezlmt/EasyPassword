use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};
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

/// Password derivation algorithm version.
///
/// - `V1`: original algorithm. Only the first 128 bits of the Argon2 output are
///   consumed, which is not enough entropy for passwords of length >= 16: the
///   tail of the password becomes predictable (e.g. length 32 always ends with
///   `aaaaaaaa`). Kept only so existing passwords stay reproducible.
/// - `V2`: consumes the full Argon2 output as a big integer and scales the
///   output length with the requested password length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AlgorithmVersion {
    #[default]
    V1,
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GenerationMode {
    Argon2id,
    Concatenation,
}

impl Default for GenerationMode {
    fn default() -> Self {
        Self::Argon2id
    }
}

#[derive(Debug, Clone)]
pub struct PasswordConfig {
    pub length: usize,
    pub use_lowercase: bool,
    pub use_uppercase: bool,
    pub use_digits: bool,
    pub use_symbols: bool,
    pub mode: GenerationMode,
    pub algorithm: AlgorithmVersion,
}

impl Default for PasswordConfig {
    fn default() -> Self {
        Self {
            length: 16,
            use_lowercase: true,
            use_uppercase: true,
            use_digits: true,
            use_symbols: true,
            mode: GenerationMode::default(),
            algorithm: AlgorithmVersion::V2,
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
    if config.mode == GenerationMode::Concatenation {
        // Concatenation mode is intentionally simple: no implicit separator.
        // If you want a separator (e.g. '!'), include it in the master_key.
        return Ok(format!("{}{}", master_key, site));
    }

    let charset = config.build_charset();
    if charset.is_empty() {
        return Err(EasyPasswordError::PasswordGeneration(
            "At least one character class must be enabled".to_string(),
        ));
    }

    let site_normalized = site.to_lowercase();

    let password = match config.algorithm {
        AlgorithmVersion::V1 => {
            let salt = build_salt(&site_normalized, counter);
            let mut entropy = derive_entropy(master_key, &salt, ENTROPY_BYTES)?;
            let password = render_password(&entropy, &charset, config);
            entropy.zeroize();
            password
        }
        AlgorithmVersion::V2 => {
            let required_count = config.count_enabled_charsets();
            if config.length < required_count {
                return Err(EasyPasswordError::PasswordGeneration(format!(
                    "length {} is shorter than the number of enabled character classes ({})",
                    config.length, required_count
                )));
            }
            let salt = build_salt_v2(&site_normalized, counter, config.length);
            let mut entropy = derive_entropy(master_key, &salt, entropy_bytes_v2(config.length))?;
            let password = render_password_v2(&entropy, &charset, config);
            entropy.zeroize();
            password
        }
    };

    Ok(password)
}

/// Number of Argon2 output bytes used by V2.
///
/// Each output character consumes at most ~log2(94) + log2(length) bits, so
/// `2 * length` bytes leaves a comfortable surplus (which also keeps modulo
/// bias negligible). Never fewer than 64 bytes.
fn entropy_bytes_v2(length: usize) -> usize {
    ENTROPY_BYTES.max(length.saturating_mul(2))
}

fn build_salt(site: &str, counter: u32) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(site.as_bytes());
    hasher.update(counter.to_le_bytes());
    hasher.finalize().to_vec()
}

/// V2 salt: domain-separated and bound to the output length, so passwords of
/// different lengths for the same site are unrelated (in V1 they share
/// characters).
fn build_salt_v2(site: &str, counter: u32, length: usize) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"easypassword/v2\0");
    hasher.update(site.as_bytes());
    hasher.update(b"\0");
    hasher.update(counter.to_le_bytes());
    hasher.update((length as u32).to_le_bytes());
    hasher.finalize().to_vec()
}

fn derive_entropy(master_key: &str, salt: &[u8], output_len: usize) -> Result<Vec<u8>> {
    let params = Params::new(
        ARGON2_M_COST,
        ARGON2_T_COST,
        ARGON2_P_COST,
        Some(output_len),
    )
    .map_err(|e| EasyPasswordError::PasswordGeneration(e.to_string()))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut entropy = vec![0u8; output_len];

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

/// V2 renderer: same structure as V1, but draws digits from a big integer built
/// from the *entire* entropy buffer instead of only its first 16 bytes.
fn render_password_v2(entropy: &[u8], charset: &[char], config: &PasswordConfig) -> String {
    let mut num = BigUint::from_be_bytes(entropy);
    let required_count = config.count_enabled_charsets();
    let base_length = config.length.saturating_sub(required_count);

    let mut password: Vec<char> = Vec::with_capacity(config.length);
    for _ in 0..base_length {
        let idx = num.div_rem_small(charset.len() as u32);
        password.push(charset[idx as usize]);
    }

    let classes: [(bool, &str); 4] = [
        (config.use_lowercase, LOWERCASE),
        (config.use_uppercase, UPPERCASE),
        (config.use_digits, DIGITS),
        (config.use_symbols, SYMBOLS),
    ];
    let mut required_chars = Vec::with_capacity(required_count);
    for (enabled, class) in classes {
        if enabled {
            let class: Vec<char> = class.chars().collect();
            let idx = num.div_rem_small(class.len() as u32);
            required_chars.push(class[idx as usize]);
        }
    }

    for ch in required_chars {
        // Insert at any of len+1 positions (V1 could never append at the end).
        let pos = num.div_rem_small(password.len() as u32 + 1);
        password.insert(pos as usize, ch);
    }

    num.zeroize();
    password.into_iter().collect()
}

/// Minimal arbitrary-precision unsigned integer (big-endian u32 limbs),
/// supporting only what the renderer needs: division by a small divisor.
struct BigUint {
    limbs: Vec<u32>,
}

impl BigUint {
    fn from_be_bytes(bytes: &[u8]) -> Self {
        let pad = (4 - bytes.len() % 4) % 4;
        let mut padded = vec![0u8; pad];
        padded.extend_from_slice(bytes);
        let limbs = padded
            .chunks_exact(4)
            .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        padded.zeroize();
        Self { limbs }
    }

    /// Divides `self` in place by `divisor` and returns the remainder.
    fn div_rem_small(&mut self, divisor: u32) -> u32 {
        debug_assert!(divisor > 0);
        let d = divisor as u64;
        let mut rem: u64 = 0;
        for limb in self.limbs.iter_mut() {
            let cur = (rem << 32) | (*limb as u64);
            *limb = (cur / d) as u32;
            rem = cur % d;
        }
        rem as u32
    }

    fn zeroize(&mut self) {
        self.limbs.zeroize();
    }
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

    const V1_GOLDEN: &[(usize, &str)] = &[
        (8, r#"SRQt8^MT"#),
        (16, r#"RQMT`Gl*q:$3*xqM"#),
        (20, r#"#7YRQMT`l*q:$qMcHB</"#),
        (32, r#"!0AaRQMT`l*q:$qMHB</o]ndaaaaaaaa"#),
        (64, r#"!0AaRQMT`l*q:$qMHB</o]ndaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"#),
    ];
    const V2_GOLDEN: &[(usize, &str)] = &[
        (8, r#"-4!ePqNK"#),
        (16, r#"^qNF7lo]AbZ@F[hL"#),
        (20, r#"PM!ArRmSH(5&^dJMB8y-"#),
        (32, r#"H<8{C<hP'"MaJ+'iaN5R/fiv}_Qc5*MH"#),
        (64, r#"9{e}dL:7Pc`?:+A,w2c^D;\A'W\xC-4ncK4R"a|r\-`1jd1_k!_;w_|u:G7-%``&"#),
    ];

    fn v1() -> PasswordConfig {
        PasswordConfig {
            algorithm: AlgorithmVersion::V1,
            ..Default::default()
        }
    }

    // Golden vectors computed with an independent Python implementation
    // (argon2-cffi). V1 vectors pin backwards compatibility: they must never
    // change, or every existing user's passwords would change.
    #[test]
    fn test_v1_golden_vectors() {
        for (length, expected) in V1_GOLDEN {
            let config = PasswordConfig {
                length: *length,
                ..v1()
            };
            let pw = generate_password("master", "github.com", 1, &config).unwrap();
            assert_eq!(pw, *expected, "v1 length {}", length);
        }
    }

    #[test]
    fn test_v2_golden_vectors() {
        for (length, expected) in V2_GOLDEN {
            let config = PasswordConfig {
                length: *length,
                ..Default::default()
            };
            let pw = generate_password("master", "github.com", 1, &config).unwrap();
            assert_eq!(pw, *expected, "v2 length {}", length);
        }
    }

    #[test]
    fn test_v2_long_password_has_no_degenerate_tail() {
        let config = PasswordConfig {
            length: 64,
            ..Default::default()
        };
        let pw = generate_password("master", "github.com", 1, &config).unwrap();
        assert_eq!(pw.chars().count(), 64);
        assert!(!pw.ends_with("aaaa"), "degenerate tail: {}", pw);
    }

    #[test]
    fn test_v2_contains_every_enabled_class() {
        for site in ["a.com", "b.com", "c.com", "d.com", "e.com"] {
            let pw = generate_password("master", site, 1, &PasswordConfig::default()).unwrap();
            assert!(pw.chars().any(|c| LOWERCASE.contains(c)));
            assert!(pw.chars().any(|c| UPPERCASE.contains(c)));
            assert!(pw.chars().any(|c| DIGITS.contains(c)));
            assert!(pw.chars().any(|c| SYMBOLS.contains(c)));
        }
    }

    #[test]
    fn test_v2_rejects_too_short_length() {
        let config = PasswordConfig {
            length: 3,
            ..Default::default()
        };
        assert!(generate_password("master", "github.com", 1, &config).is_err());
    }

    #[test]
    fn test_versions_differ() {
        let a = generate_password("master", "github.com", 1, &v1()).unwrap();
        let b = generate_password("master", "github.com", 1, &PasswordConfig::default()).unwrap();
        assert_ne!(a, b);
    }

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

    #[test]
    fn test_concatenation_mode() {
        let config = PasswordConfig {
            mode: GenerationMode::Concatenation,
            ..Default::default()
        };
        let pw = generate_password("master", "github.com", 1, &config).unwrap();
        assert_eq!(pw, "mastergithub.com");
    }

    #[test]
    fn test_concatenation_mode_preserves_case() {
        let config = PasswordConfig {
            mode: GenerationMode::Concatenation,
            ..Default::default()
        };
        let pw = generate_password("master", "GitHub.com", 1, &config).unwrap();
        assert_eq!(pw, "masterGitHub.com");
    }

    #[test]
    fn test_concatenation_mode_allows_separator_in_master_key() {
        let config = PasswordConfig {
            mode: GenerationMode::Concatenation,
            ..Default::default()
        };
        let pw = generate_password("master!", "github.com", 1, &config).unwrap();
        assert_eq!(pw, "master!github.com");
    }
}
