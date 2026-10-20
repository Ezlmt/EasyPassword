//! User settings (`settings.toml`). Contains no secrets: the master seed lives
//! in the OS keychain.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::algorithm::Rules;
use crate::site::normalize_site;
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    /// Type the password into the previously focused window.
    #[default]
    Type,
    /// Copy to the clipboard and clear it after `clipboard_clear_seconds`.
    Copy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Global shortcut that opens the popup, e.g. `CommandOrControl+Alt+P`.
    pub shortcut: String,
    /// What Enter does in the popup (the other action is on Ctrl/Cmd+Enter).
    pub output: OutputMode,
    pub clipboard_clear_seconds: u32,
    /// Remember site names (not passwords) for autocomplete.
    pub remember_sites: bool,
    /// Informational copy of the identity used when the seed was derived.
    pub identity: String,
    pub defaults: Rules,
    pub sites: BTreeMap<String, SiteEntry>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: "CommandOrControl+Alt+P".to_string(),
            output: OutputMode::Type,
            clipboard_clear_seconds: 20,
            remember_sites: true,
            identity: String::new(),
            defaults: Rules::default(),
            sites: BTreeMap::new(),
        }
    }
}

/// Per-site overrides. `None` means "use the default".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteEntry {
    pub counter: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lowercase: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uppercase: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digits: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols: Option<String>,
    /// Optional login name shown in the popup, purely informational.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub username: String,
    /// Unix seconds; used to rank autocomplete suggestions.
    pub last_used: u64,
}

impl Default for SiteEntry {
    fn default() -> Self {
        Self {
            counter: 1,
            length: None,
            lowercase: None,
            uppercase: None,
            digits: None,
            symbols: None,
            username: String::new(),
            last_used: 0,
        }
    }
}

impl SiteEntry {
    pub fn is_customized(&self) -> bool {
        self.counter != 1
            || self.length.is_some()
            || self.lowercase.is_some()
            || self.uppercase.is_some()
            || self.digits.is_some()
            || self.symbols.is_some()
            || !self.username.is_empty()
    }
}

/// Effective parameters for one site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteParams {
    pub site: String,
    pub counter: u32,
    pub rules: Rules,
    pub username: String,
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self, Error> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| Error::Config(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Atomic write (temp file + rename), user-only permissions on Unix.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        let text = toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        {
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn params_for(&self, site: &str) -> SiteParams {
        let site = normalize_site(site);
        let entry = self.sites.get(&site).cloned().unwrap_or_default();
        let d = &self.defaults;
        SiteParams {
            counter: entry.counter.max(1),
            rules: Rules {
                length: entry.length.unwrap_or(d.length),
                lowercase: entry.lowercase.unwrap_or(d.lowercase),
                uppercase: entry.uppercase.unwrap_or(d.uppercase),
                digits: entry.digits.unwrap_or(d.digits),
                symbols: entry.symbols.clone().unwrap_or_else(|| d.symbols.clone()),
            },
            username: entry.username,
            site,
        }
    }

    /// Stores `params` as overrides relative to the defaults (only fields that
    /// differ are written, so later default changes still apply elsewhere).
    pub fn set_params(&mut self, params: &SiteParams, now: u64) {
        let d = &self.defaults;
        let r = &params.rules;
        let last_used = self.sites.get(&params.site).map_or(now, |e| e.last_used.max(now));
        let entry = SiteEntry {
            counter: params.counter.max(1),
            length: (r.length != d.length).then_some(r.length),
            lowercase: (r.lowercase != d.lowercase).then_some(r.lowercase),
            uppercase: (r.uppercase != d.uppercase).then_some(r.uppercase),
            digits: (r.digits != d.digits).then_some(r.digits),
            symbols: (r.symbols != d.symbols).then(|| r.symbols.clone()),
            username: params.username.trim().to_string(),
            last_used,
        };
        self.sites.insert(params.site.clone(), entry);
    }

    /// Records that `site` was used. Returns whether anything changed.
    pub fn touch(&mut self, site: &str, now: u64) -> bool {
        let site = normalize_site(site);
        if site.is_empty() {
            return false;
        }
        match self.sites.get_mut(&site) {
            Some(entry) => entry.last_used = now,
            None if self.remember_sites => {
                self.sites.insert(site, SiteEntry { last_used: now, ..SiteEntry::default() });
            }
            None => return false,
        }
        true
    }

    /// Removes sites that only exist as history (no overrides).
    pub fn forget_history(&mut self) {
        self.sites.retain(|_, e| e.is_customized());
    }

    /// Autocomplete: prefix matches first, then substring matches, each group
    /// most-recently-used first.
    pub fn suggestions(&self, query: &str, limit: usize) -> Vec<String> {
        let q = query.trim().to_lowercase();
        let mut hits: Vec<(u8, u64, &String)> = self
            .sites
            .iter()
            .filter_map(|(site, e)| {
                let rank = if q.is_empty() || site.starts_with(&q) {
                    0
                } else if site.contains(&q) {
                    1
                } else {
                    return None;
                };
                Some((rank, e.last_used, site))
            })
            .collect();
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(b.2)));
        hits.into_iter().take(limit).map(|(_, _, s)| s.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("ep-test-{}", std::process::id()));
        let path = dir.join("settings.toml");
        let _ = fs::remove_file(&path);
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());

        let mut s = Settings::default();
        s.touch("https://GitHub.com/x", 5);
        s.save(&path).unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded, s);
        assert!(loaded.sites.contains_key("github.com"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_file_uses_defaults() {
        let s: Settings = toml::from_str("output = \"copy\"\n[defaults]\nlength = 12\n").unwrap();
        assert_eq!(s.output, OutputMode::Copy);
        assert_eq!(s.defaults.length, 12);
        assert!(s.defaults.digits);
        assert_eq!(s.shortcut, Settings::default().shortcut);
    }

    #[test]
    fn overrides_are_stored_as_diffs() {
        let mut s = Settings::default();
        let mut p = s.params_for("bank.com");
        p.rules.length = 12;
        p.rules.symbols = String::new();
        p.counter = 2;
        s.set_params(&p, 10);
        let e = &s.sites["bank.com"];
        assert_eq!(e.length, Some(12));
        assert_eq!(e.symbols.as_deref(), Some(""));
        assert_eq!(e.uppercase, None);
        assert_eq!(s.params_for("BANK.com"), p);

        // A later change of defaults still applies to non-overridden fields.
        s.defaults.uppercase = false;
        assert!(!s.params_for("bank.com").rules.uppercase);
    }

    #[test]
    fn history_and_suggestions() {
        let mut s = Settings::default();
        s.touch("github.com", 1);
        s.touch("gitlab.com", 3);
        s.touch("mygit.io", 9);
        s.touch("bank.com", 2);
        assert_eq!(s.suggestions("git", 10), vec!["gitlab.com", "github.com", "mygit.io"]);
        assert_eq!(s.suggestions("", 2), vec!["mygit.io", "gitlab.com"]);
        assert!(s.suggestions("zzz", 10).is_empty());

        let mut p = s.params_for("bank.com");
        p.counter = 3;
        s.set_params(&p, 4);
        s.forget_history();
        assert_eq!(s.sites.keys().collect::<Vec<_>>(), vec!["bank.com"]);

        s.remember_sites = false;
        assert!(!s.touch("new.com", 5));
        assert!(!s.sites.contains_key("new.com"));
    }
}
