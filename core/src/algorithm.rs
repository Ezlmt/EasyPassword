//! Password derivation.
//!
//! ```text
//! seed     = Argon2id(m=64 MiB, t=3, p=1)(master_key, SHA256(SEED_DOMAIN || identity))   // once per session
//! okm      = HKDF-SHA256-Expand(prk = seed, info = site || counter || rules, len)       // per site, instant
//! password = render(okm, rules)                                                          // unbiased base conversion
//! ```
//!
//! The expensive, memory-hard step runs only once (when the master key is
//! entered); only the 32-byte seed is kept, never the master key itself.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::site::normalize_site;
use crate::Error;

const SEED_DOMAIN: &[u8] = b"EasyPassword/v2/seed\0";
const SITE_DOMAIN: &[u8] = b"EasyPassword/v2/site\0";
const FINGERPRINT_DOMAIN: &[u8] = b"EasyPassword/v2/fingerprint";

const ARGON2_M_KIB: u32 = 64 * 1024;
const ARGON2_T: u32 = 3;
const ARGON2_P: u32 = 1;

pub const MIN_LENGTH: usize = 4;
pub const MAX_LENGTH: usize = 128;

pub const LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
pub const UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
pub const DIGITS: &str = "0123456789";
/// Symbols accepted by virtually every site (no quotes, backslash, brackets,
/// `<>` or spaces).
pub const DEFAULT_SYMBOLS: &str = "!#$%&*+-.:=?@^_";

/// The 32-byte secret derived from the master key. This is what gets stored
/// in the OS keychain; the master key itself is never persisted.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MasterSeed([u8; 32]);

impl fmt::Debug for MasterSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MasterSeed(<redacted>)")
    }
}

impl MasterSeed {
    /// Runs the memory-hard KDF. Takes a few hundred milliseconds by design.
    ///
    /// `identity` is optional (may be empty). It acts as a personal salt, e.g.
    /// your email; it must be the same on every device. It is trimmed and
    /// lower-cased. The master key is used verbatim.
    pub fn derive(master_key: &str, identity: &str) -> Result<Self, Error> {
        if master_key.is_empty() {
            return Err(Error::EmptyMasterKey);
        }
        let identity = identity.trim().to_lowercase();
        let salt = Sha256::new()
            .chain_update(SEED_DOMAIN)
            .chain_update(identity.as_bytes())
            .finalize();

        let params = Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(32))
            .map_err(|e| Error::Kdf(e.to_string()))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        let mut seed = [0u8; 32];
        argon2
            .hash_password_into(master_key.as_bytes(), &salt, &mut seed)
            .map_err(|e| Error::Kdf(e.to_string()))?;
        Ok(Self(seed))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let arr: [u8; 32] = bytes.try_into().map_err(|_| Error::InvalidSeed)?;
        Ok(Self(arr))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Short, non-secret check code (e.g. `K7Q2M`) so the user can confirm
    /// they typed the same master key / identity as on another device.
    /// Reveals only 25 bits, which is useless for brute-forcing the seed.
    pub fn fingerprint(&self) -> String {
        const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.0).expect("any key length");
        mac.update(FINGERPRINT_DOMAIN);
        let tag = mac.finalize().into_bytes();
        let mut bits = u32::from_be_bytes([tag[0], tag[1], tag[2], tag[3]]);
        let mut out = String::with_capacity(5);
        for _ in 0..5 {
            out.push(ALPHABET[(bits >> 27) as usize] as char);
            bits <<= 5;
        }
        out
    }
}

/// What the generated password must look like.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub length: usize,
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    /// Allowed symbols; empty string disables symbols.
    pub symbols: String,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            length: 20,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: DEFAULT_SYMBOLS.to_string(),
        }
    }
}

impl Rules {
    /// Enabled character classes, in a fixed order.
    fn classes(&self) -> Result<Vec<Vec<char>>, Error> {
        let mut classes = Vec::with_capacity(4);
        if self.lowercase {
            classes.push(LOWERCASE.chars().collect());
        }
        if self.uppercase {
            classes.push(UPPERCASE.chars().collect());
        }
        if self.digits {
            classes.push(DIGITS.chars().collect());
        }
        let symbols = canonical_symbols(&self.symbols)?;
        if !symbols.is_empty() {
            classes.push(symbols.chars().collect());
        }
        Ok(classes)
    }

    pub fn validate(&self) -> Result<(), Error> {
        let classes = self.classes()?;
        if classes.is_empty() {
            return Err(Error::NoCharacterClasses);
        }
        if !(MIN_LENGTH..=MAX_LENGTH).contains(&self.length) || self.length < classes.len() {
            return Err(Error::InvalidLength(self.length));
        }
        Ok(())
    }
}

/// Sorted, de-duplicated symbol set. Only printable, non-alphanumeric ASCII
/// is allowed, so the alphabet is identical on every platform/keyboard.
fn canonical_symbols(symbols: &str) -> Result<String, Error> {
    let mut chars: Vec<char> = Vec::new();
    for c in symbols.chars() {
        if !c.is_ascii_graphic() || c.is_ascii_alphanumeric() {
            return Err(Error::InvalidSymbol(c));
        }
        chars.push(c);
    }
    chars.sort_unstable();
    chars.dedup();
    Ok(chars.into_iter().collect())
}

/// Derives the password for `site` (normalized first, see [`normalize_site`]).
pub fn generate(
    seed: &MasterSeed,
    site: &str,
    counter: u32,
    rules: &Rules,
) -> Result<Zeroizing<String>, Error> {
    rules.validate()?;
    let site = normalize_site(site);
    if site.is_empty() {
        return Err(Error::EmptySite);
    }
    let classes = rules.classes()?;
    let symbols = canonical_symbols(&rules.symbols)?;

    // Everything that shapes the output goes into `info`, so changing any rule
    // yields an unrelated password rather than a "similar" one.
    let mut info = Vec::with_capacity(SITE_DOMAIN.len() + site.len() + symbols.len() + 16);
    info.extend_from_slice(SITE_DOMAIN);
    info.extend_from_slice(site.as_bytes());
    info.push(0);
    info.extend_from_slice(&counter.to_le_bytes());
    info.extend_from_slice(&(rules.length as u32).to_le_bytes());
    info.push((rules.lowercase as u8) | (rules.uppercase as u8) << 1 | (rules.digits as u8) << 2);
    info.extend_from_slice(symbols.as_bytes());

    // Each output char consumes < 7 + log2(length) bits; 2 bytes per char plus
    // 32 spare bytes keeps the base-conversion bias negligible (< 2^-200).
    let mut okm = Zeroizing::new(vec![0u8; 2 * rules.length + 32]);
    let hk = Hkdf::<Sha256>::from_prk(seed.as_bytes()).map_err(|_| Error::InvalidSeed)?;
    hk.expand(&info, &mut okm)
        .map_err(|e| Error::Kdf(e.to_string()))?;

    Ok(render(&okm, &classes, rules.length))
}

/// Legacy EasyPassword 0.x "concatenation" password: `prefix + site`,
/// byte for byte, with the site exactly as typed (only surrounding whitespace
/// is trimmed; no lower-casing or URL handling, to stay identical to 0.x).
pub fn concat(prefix: &str, site_literal: &str) -> Result<Zeroizing<String>, Error> {
    if prefix.is_empty() {
        return Err(Error::EmptyConcatPrefix);
    }
    let site = site_literal.trim();
    if site.is_empty() {
        return Err(Error::EmptySite);
    }
    let mut out = Zeroizing::new(String::with_capacity(prefix.len() + site.len()));
    out.push_str(prefix);
    out.push_str(site);
    Ok(out)
}

/// Base-converts `entropy` into `length` characters: `length - k` from the
/// union alphabet, then one from each of the `k` classes inserted at a derived
/// position, guaranteeing every enabled class appears.
fn render(entropy: &[u8], classes: &[Vec<char>], length: usize) -> Zeroizing<String> {
    let alphabet: Vec<char> = classes.iter().flatten().copied().collect();
    let mut num = BigUint::from_be_bytes(entropy);

    let mut out: Zeroizing<Vec<char>> = Zeroizing::new(Vec::with_capacity(length));
    for _ in 0..length - classes.len() {
        out.push(alphabet[num.div_rem(alphabet.len() as u32) as usize]);
    }
    for class in classes {
        let ch = class[num.div_rem(class.len() as u32) as usize];
        let pos = num.div_rem(out.len() as u32 + 1) as usize;
        out.insert(pos, ch);
    }
    Zeroizing::new(out.iter().collect())
}

/// Minimal big unsigned integer (big-endian u32 limbs) supporting in-place
/// division by a small divisor.
#[derive(Zeroize, ZeroizeOnDrop)]
struct BigUint {
    limbs: Vec<u32>,
}

impl BigUint {
    fn from_be_bytes(bytes: &[u8]) -> Self {
        let pad = (4 - bytes.len() % 4) % 4;
        let mut padded = Zeroizing::new(vec![0u8; pad]);
        padded.extend_from_slice(bytes);
        let limbs = padded
            .chunks(4)
            .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Self { limbs }
    }

    fn div_rem(&mut self, divisor: u32) -> u32 {
        debug_assert!(divisor > 0);
        let d = u64::from(divisor);
        let mut rem = 0u64;
        for limb in &mut self.limbs {
            let cur = (rem << 32) | u64::from(*limb);
            *limb = (cur / d) as u32;
            rem = cur % d;
        }
        rem as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    /// Argon2 with 64 MiB is slow in debug builds; derive once per test binary.
    fn seed() -> &'static MasterSeed {
        static SEED: OnceLock<MasterSeed> = OnceLock::new();
        SEED.get_or_init(|| MasterSeed::derive("correct horse battery staple", "").unwrap())
    }

    fn test_seed(byte: u8) -> MasterSeed {
        MasterSeed::from_bytes(&[byte; 32]).unwrap()
    }

    #[test]
    fn concat_matches_legacy_behaviour() {
        // Same expectations as the 0.x test-suite.
        assert_eq!(concat("master", "github.com").unwrap().as_str(), "mastergithub.com");
        assert_eq!(concat("master", "GitHub.com").unwrap().as_str(), "masterGitHub.com");
        assert_eq!(concat("master!", "github.com").unwrap().as_str(), "master!github.com");
        assert_eq!(concat("m ", " www.x.com ").unwrap().as_str(), "m www.x.com");
        assert!(matches!(concat("", "x"), Err(Error::EmptyConcatPrefix)));
        assert!(matches!(concat("m", "  "), Err(Error::EmptySite)));
    }

    #[test]
    fn deterministic() {
        let r = Rules::default();
        let a = generate(seed(), "github.com", 1, &r).unwrap();
        let b = generate(seed(), "github.com", 1, &r).unwrap();
        assert_eq!(*a, *b);
        assert_eq!(a.chars().count(), 20);
    }

    #[test]
    fn site_normalization_applies() {
        let r = Rules::default();
        let a = generate(&test_seed(1), "https://www.GitHub.com/login", 1, &r).unwrap();
        let b = generate(&test_seed(1), "github.com", 1, &r).unwrap();
        assert_eq!(*a, *b);
    }

    #[test]
    fn inputs_change_output() {
        let s = test_seed(7);
        let r = Rules::default();
        let base = generate(&s, "github.com", 1, &r).unwrap();
        assert_ne!(*base, *generate(&s, "gitlab.com", 1, &r).unwrap());
        assert_ne!(*base, *generate(&s, "github.com", 2, &r).unwrap());
        assert_ne!(
            *base,
            *generate(&test_seed(8), "github.com", 1, &r).unwrap()
        );
        let longer = generate(
            &s,
            "github.com",
            1,
            &Rules {
                length: 21,
                ..r.clone()
            },
        )
        .unwrap();
        assert!(!longer.starts_with(base.as_str()) && !longer.contains(&base[..10]));
        let nosym = Rules {
            symbols: String::new(),
            ..r.clone()
        };
        assert_ne!(*base, *generate(&s, "github.com", 1, &nosym).unwrap());
    }

    #[test]
    fn symbol_order_does_not_matter() {
        let s = test_seed(3);
        let a = Rules {
            symbols: "!@#".into(),
            ..Rules::default()
        };
        let b = Rules {
            symbols: "#!@@".into(),
            ..Rules::default()
        };
        assert_eq!(
            *generate(&s, "x.com", 1, &a).unwrap(),
            *generate(&s, "x.com", 1, &b).unwrap()
        );
    }

    #[test]
    fn every_class_present_and_alphabet_respected() {
        let s = test_seed(9);
        let rules = Rules {
            length: 8,
            symbols: "!@".into(),
            ..Rules::default()
        };
        for i in 0..200 {
            let pw = generate(&s, &format!("site{i}.com"), 1, &rules).unwrap();
            assert_eq!(pw.chars().count(), 8);
            assert!(pw.chars().any(|c| c.is_ascii_lowercase()));
            assert!(pw.chars().any(|c| c.is_ascii_uppercase()));
            assert!(pw.chars().any(|c| c.is_ascii_digit()));
            assert!(pw.chars().any(|c| c == '!' || c == '@'));
            assert!(pw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '!' || c == '@'));
        }
    }

    #[test]
    fn long_passwords_are_not_degenerate() {
        let s = test_seed(5);
        let pw = generate(
            &s,
            "x.com",
            1,
            &Rules {
                length: MAX_LENGTH,
                ..Rules::default()
            },
        )
        .unwrap();
        assert_eq!(pw.chars().count(), MAX_LENGTH);
        let tail: Vec<char> = pw.chars().rev().take(16).collect();
        assert!(tail.windows(2).any(|w| w[0] != w[1]), "{}", pw.as_str());
    }

    #[test]
    fn character_distribution_is_roughly_uniform() {
        // Digits-only, 64 chars x 500 sites = 32000 samples over 10 buckets.
        let s = test_seed(11);
        let rules = Rules {
            length: 64,
            lowercase: false,
            uppercase: false,
            digits: true,
            symbols: String::new(),
        };
        let mut counts = [0u32; 10];
        for i in 0..500 {
            for c in generate(&s, &format!("{i}"), 1, &rules).unwrap().chars() {
                counts[c.to_digit(10).unwrap() as usize] += 1;
            }
        }
        for c in counts {
            assert!((2900..=3500).contains(&c), "{counts:?}");
        }
    }

    #[test]
    fn rejects_bad_rules() {
        let s = test_seed(1);
        let none = Rules {
            lowercase: false,
            uppercase: false,
            digits: false,
            symbols: String::new(),
            length: 10,
        };
        assert!(matches!(
            generate(&s, "x", 1, &none),
            Err(Error::NoCharacterClasses)
        ));
        assert!(matches!(
            generate(
                &s,
                "x",
                1,
                &Rules {
                    length: 3,
                    ..Rules::default()
                }
            ),
            Err(Error::InvalidLength(3))
        ));
        assert!(matches!(
            generate(
                &s,
                "x",
                1,
                &Rules {
                    length: 129,
                    ..Rules::default()
                }
            ),
            Err(Error::InvalidLength(129))
        ));
        assert!(matches!(
            generate(
                &s,
                "x",
                1,
                &Rules {
                    symbols: "a!".into(),
                    ..Rules::default()
                }
            ),
            Err(Error::InvalidSymbol('a'))
        ));
        assert!(matches!(
            generate(
                &s,
                "x",
                1,
                &Rules {
                    symbols: "! ".into(),
                    ..Rules::default()
                }
            ),
            Err(Error::InvalidSymbol(' '))
        ));
        assert!(matches!(
            generate(&s, "  ", 1, &Rules::default()),
            Err(Error::EmptySite)
        ));
    }

    #[test]
    fn seed_derivation() {
        assert!(matches!(
            MasterSeed::derive("", ""),
            Err(Error::EmptyMasterKey)
        ));
        // identity is trimmed / case-insensitive, master key is not
        let a = MasterSeed::derive("k", " Me@Example.com ").unwrap();
        let b = MasterSeed::derive("k", "me@example.com").unwrap();
        let c = MasterSeed::derive("k", "").unwrap();
        let d = MasterSeed::derive("K", "").unwrap();
        assert_eq!(a.as_bytes(), b.as_bytes());
        assert_ne!(a.as_bytes(), c.as_bytes());
        assert_ne!(c.as_bytes(), d.as_bytes());
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_ne!(a.fingerprint(), c.fingerprint());
        assert_eq!(c.fingerprint().len(), 5);
        assert_eq!(format!("{c:?}"), "MasterSeed(<redacted>)");
    }

    #[test]
    fn seed_roundtrip() {
        let s = seed();
        let restored = MasterSeed::from_bytes(s.as_bytes()).unwrap();
        assert_eq!(restored.as_bytes(), s.as_bytes());
        assert!(MasterSeed::from_bytes(&[0; 31]).is_err());
    }

    include!("../tests/golden.inc");

    /// Golden vectors from the independent Python reference
    /// (`scripts/reference.py`). They pin the algorithm: if they change,
    /// every user's passwords change.
    #[test]
    fn golden_vectors() {
        let s = seed();
        for (site, counter, length, symbols, expected_fp, expected) in GOLDEN {
            assert_eq!(s.fingerprint(), *expected_fp);
            let rules = Rules {
                length: *length,
                symbols: (*symbols).to_string(),
                ..Rules::default()
            };
            let pw = generate(s, site, *counter, &rules).unwrap();
            assert_eq!(pw.as_str(), *expected, "{site} {counter} {length}");
        }
    }
}
