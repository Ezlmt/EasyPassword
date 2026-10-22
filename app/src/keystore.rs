//! Secret storage in the OS credential store
//! (macOS Keychain / Windows Credential Manager).
//!
//! Two entries: the 32-byte master seed, and (optionally) the legacy concat
//! prefix used by EasyPassword 0.x "!!" passwords.

use easypassword_core::MasterSeed;
use keyring::{Entry, Error as KeyringError};
use zeroize::{Zeroize, Zeroizing};

const SERVICE: &str = "EasyPassword";
const SEED_ACCOUNT: &str = "master-seed-v2";
const PREFIX_ACCOUNT: &str = "concat-prefix";

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| format!("无法访问系统钥匙串: {e}"))
}

fn delete_entry(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除钥匙串条目失败: {e}")),
    }
}

pub fn load() -> Result<Option<MasterSeed>, String> {
    match entry(SEED_ACCOUNT)?.get_secret() {
        Ok(mut bytes) => {
            let seed = MasterSeed::from_bytes(&bytes).map_err(|e| e.to_string());
            bytes.zeroize();
            seed.map(Some)
        }
        Err(KeyringError::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取系统钥匙串失败: {e}")),
    }
}

pub fn store(seed: &MasterSeed) -> Result<(), String> {
    entry(SEED_ACCOUNT)?
        .set_secret(seed.as_bytes())
        .map_err(|e| format!("写入系统钥匙串失败: {e}"))
}

pub fn delete() -> Result<(), String> {
    delete_entry(SEED_ACCOUNT)
}

pub fn load_prefix() -> Result<Option<Zeroizing<String>>, String> {
    match entry(PREFIX_ACCOUNT)?.get_password() {
        Ok(p) => Ok(Some(Zeroizing::new(p))),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取系统钥匙串失败: {e}")),
    }
}

pub fn store_prefix(prefix: &str) -> Result<(), String> {
    entry(PREFIX_ACCOUNT)?
        .set_password(prefix)
        .map_err(|e| format!("写入系统钥匙串失败: {e}"))
}

pub fn delete_prefix() -> Result<(), String> {
    delete_entry(PREFIX_ACCOUNT)
}
