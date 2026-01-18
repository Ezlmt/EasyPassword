pub mod config;
pub mod core;
pub mod detect;
pub mod error;
pub mod inject;
pub mod master_key;

pub use config::Config;
pub use core::{generate_password, PasswordConfig};
pub use detect::{start_keyboard_listener, TriggerEvent};
pub use error::{EasyPasswordError, Result};
pub use inject::TextInjector;
pub use master_key::MasterKeyCache;
