use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::core::{AlgorithmVersion, GenerationMode, PasswordConfig};
use crate::error::{EasyPasswordError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub default: DefaultConfig,
    #[serde(default)]
    pub sites: HashMap<String, SiteConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultConfig {
    pub master_key: Option<String>,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default = "default_length")]
    pub length: usize,
    #[serde(default = "default_true")]
    pub lowercase: bool,
    #[serde(default = "default_true")]
    pub uppercase: bool,
    #[serde(default = "default_true")]
    pub digits: bool,
    #[serde(default = "default_true")]
    pub symbols: bool,
    #[serde(default = "default_trigger_prefix")]
    pub trigger_prefix: String,
    #[serde(default = "default_concat_trigger_prefix")]
    pub concat_trigger_prefix: String,
    #[serde(default)]
    pub mode: GenerationMode,
    /// Derivation algorithm. Missing => "v1" so existing configs keep producing
    /// the same passwords; newly created configs are written with "v2".
    #[serde(default)]
    pub algorithm: AlgorithmVersion,
    /// Keys that end a trigger: any of "space", "enter", "tab".
    #[serde(default = "default_terminators")]
    pub terminators: Vec<Terminator>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Terminator {
    Space,
    Enter,
    Tab,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiteConfig {
    pub length: Option<usize>,
    pub lowercase: Option<bool>,
    pub uppercase: Option<bool>,
    pub digits: Option<bool>,
    pub symbols: Option<bool>,
    pub counter: Option<u32>,
    pub mode: Option<GenerationMode>,
    pub algorithm: Option<AlgorithmVersion>,
}

fn default_length() -> usize {
    16
}
fn default_true() -> bool {
    true
}
fn default_trigger_prefix() -> String {
    ";;".to_string()
}
fn default_concat_trigger_prefix() -> String {
    "!!".to_string()
}
// Only Space by default: Enter/Tab are delivered to the focused app before the
// password is injected (Enter submits the form, Tab moves focus elsewhere).
fn default_terminators() -> Vec<Terminator> {
    vec![Terminator::Space]
}

impl Default for DefaultConfig {
    fn default() -> Self {
        Self {
            master_key: None,
            autostart: false,
            length: default_length(),
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
            trigger_prefix: default_trigger_prefix(),
            concat_trigger_prefix: default_concat_trigger_prefix(),
            mode: GenerationMode::default(),
            algorithm: AlgorithmVersion::default(),
            terminators: default_terminators(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            let config: Config = toml::from_str(&content).map_err(EasyPasswordError::TomlParse)?;
            Ok(config)
        } else {
            let mut config = Config::default();
            config.default.algorithm = AlgorithmVersion::V2;
            config.save()?;
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content =
            toml::to_string_pretty(self).map_err(|e| EasyPasswordError::Config(e.to_string()))?;
        write_private(&path, content.as_bytes())?;
        Ok(())
    }

    pub fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| EasyPasswordError::Config("Cannot find config directory".to_string()))?;
        Ok(config_dir.join("easypassword").join("config.toml"))
    }

    /// Finds the per-site override for `site`, case-insensitively.
    ///
    /// Tries, in order: the full site (`www.github.com`), the site without a
    /// leading `www.` (`github.com`), and its first label (`github`), so that
    /// `[sites.github]` applies to `;;github.com`.
    ///
    /// With `algorithm = "v1"` (the legacy default) only the exact lowercase
    /// key is matched, exactly as before: widening the match would silently
    /// change existing passwords whose overrides never used to apply.
    pub fn find_site_config(&self, site: &str) -> Option<&SiteConfig> {
        let site_lower = site.to_lowercase();
        if self.default.algorithm == AlgorithmVersion::V1 {
            return self.sites.get(&site_lower);
        }

        let without_www = site_lower.strip_prefix("www.").unwrap_or(&site_lower);
        let first_label = without_www.split('.').next().unwrap_or(without_www);

        [site_lower.as_str(), without_www, first_label]
            .into_iter()
            .find_map(|candidate| {
                self.sites.get(candidate).or_else(|| {
                    self.sites
                        .iter()
                        .find(|(key, _)| key.to_lowercase() == candidate)
                        .map(|(_, cfg)| cfg)
                })
            })
    }

    pub fn get_password_config(&self, site: &str) -> PasswordConfig {
        let site_config = self.find_site_config(site);

        PasswordConfig {
            length: site_config
                .and_then(|s| s.length)
                .unwrap_or(self.default.length),
            use_lowercase: site_config
                .and_then(|s| s.lowercase)
                .unwrap_or(self.default.lowercase),
            use_uppercase: site_config
                .and_then(|s| s.uppercase)
                .unwrap_or(self.default.uppercase),
            use_digits: site_config
                .and_then(|s| s.digits)
                .unwrap_or(self.default.digits),
            use_symbols: site_config
                .and_then(|s| s.symbols)
                .unwrap_or(self.default.symbols),
            mode: site_config
                .and_then(|s| s.mode)
                .unwrap_or(self.default.mode),
            algorithm: site_config
                .and_then(|s| s.algorithm)
                .unwrap_or(self.default.algorithm),
        }
    }

    pub fn get_counter(&self, site: &str) -> u32 {
        self.find_site_config(site)
            .and_then(|s| s.counter)
            .unwrap_or(1)
    }
}

/// Writes a file readable only by the current user (0600 on Unix), since the
/// config contains the master key.
fn write_private(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        // `mode` only applies on creation; tighten pre-existing files too.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(content)
    }

    #[cfg(not(unix))]
    {
        fs::write(path, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Config {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn missing_algorithm_defaults_to_v1_for_compat() {
        let c = parse("[default]\nmaster_key = \"k\"\n");
        assert_eq!(c.default.algorithm, AlgorithmVersion::V1);
        assert_eq!(c.default.terminators, vec![Terminator::Space]);
    }

    #[test]
    fn algorithm_and_terminators_parse() {
        let c = parse(
            "[default]\nalgorithm = \"v2\"\nterminators = [\"space\", \"enter\"]\n\
             [sites.old]\nalgorithm = \"v1\"\n",
        );
        assert_eq!(c.default.algorithm, AlgorithmVersion::V2);
        assert_eq!(
            c.default.terminators,
            vec![Terminator::Space, Terminator::Enter]
        );
        assert_eq!(
            c.get_password_config("old.com").algorithm,
            AlgorithmVersion::V1
        );
        assert_eq!(
            c.get_password_config("new.com").algorithm,
            AlgorithmVersion::V2
        );
    }

    #[test]
    fn site_lookup_matches_first_label_and_www() {
        let c = parse("[default]\nalgorithm = \"v2\"\n[sites.GitHub]\nlength = 20\ncounter = 3\n");
        assert_eq!(c.get_password_config("github.com").length, 20);
        assert_eq!(c.get_password_config("www.GitHub.com").length, 20);
        assert_eq!(c.get_counter("github.com"), 3);
        assert_eq!(c.get_password_config("gitlab.com").length, 16);
    }

    #[test]
    fn exact_site_match_wins_over_first_label() {
        let c = parse(
            "[default]\nalgorithm = \"v2\"\n[sites.github]\nlength = 20\n[sites.\"github.com\"]\nlength = 24\n",
        );
        assert_eq!(c.get_password_config("github.com").length, 24);
    }

    #[test]
    fn v1_site_lookup_is_exact_only() {
        let c = parse("[sites.github]\nlength = 20\n");
        assert_eq!(c.get_password_config("github.com").length, 16);
        assert_eq!(c.get_password_config("GitHub").length, 20);
    }
}
