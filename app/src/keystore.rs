//! Master seed storage in the OS credential store
//! (macOS Keychain / Windows Credential Manager).

use easypassword_core::MasterSeed;
use keyring::{Entry, Error as KeyringError};
use zeroize::Zeroize;

const SERVICE: &str = "EasyPassword";
const ACCOUNT: &str = "master-seed-v2";

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("无法访问系统钥匙串: {e}"))
}

pub fn load() -> Result<Option<MasterSeed>, String> {
    match entry()?.get_secret() {
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
    entry()?
        .set_secret(seed.as_bytes())
        .map_err(|e| format!("写入系统钥匙串失败: {e}"))
}

pub fn delete() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除钥匙串条目失败: {e}")),
    }
}
