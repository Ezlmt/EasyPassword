//! EasyPassword core: deterministic, offline password derivation.
//!
//! This crate has no GUI or OS dependencies so it can be tested everywhere.

pub mod algorithm;
pub mod config;
pub mod site;

pub use algorithm::{concat, generate, MasterSeed, Rules};
pub use config::{Mode, OutputMode, Settings, SiteEntry, SiteParams};
pub use site::normalize_site;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("master key must not be empty")]
    EmptyMasterKey,
    #[error("concatenation prefix is not set")]
    EmptyConcatPrefix,
    #[error("site must not be empty")]
    EmptySite,
    #[error("enable at least one character type")]
    NoCharacterClasses,
    #[error("length {0} is out of range (4-128, and at least one per enabled character type)")]
    InvalidLength(usize),
    #[error("'{0}' is not allowed as a symbol (use printable ASCII punctuation)")]
    InvalidSymbol(char),
    #[error("stored master seed is invalid")]
    InvalidSeed,
    #[error("key derivation failed: {0}")]
    Kdf(String),
    #[error("settings file error: {0}")]
    Config(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
