use thiserror::Error;

#[derive(Error, Debug)]
pub enum EasyPasswordError {
    #[error("Master key not set. Use ;;!setkey<space>your_master_key to set it.")]
    MasterKeyNotSet,

    #[error("Failed to generate password: {0}")]
    PasswordGeneration(String),

    #[error("Keyboard monitoring error: {0}")]
    KeyboardMonitor(String),

    #[error("Text injection error: {0}")]
    TextInjection(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
}

pub type Result<T> = std::result::Result<T, EasyPasswordError>;
