use std::env;
use std::path::PathBuf;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::path::Path;

use anyhow::Context;

const APP_NAME: &str = "EasyPassword";

pub fn set_enabled(enabled: bool) -> anyhow::Result<()> {
    if enabled {
        enable()
    } else {
        disable()
    }
}

fn current_exe() -> anyhow::Result<PathBuf> {
    env::current_exe().context("failed to resolve current executable path")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn atomic_write(path: &Path, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create dir: {}", parent.display()))?;
    }

    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, contents)
        .with_context(|| format!("failed to write temp file: {}", tmp_path.display()))?;

    match fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(path);
            fs::rename(&tmp_path, path)
                .with_context(|| format!("failed to replace file: {}", path.display()))
        }
        Err(e) => Err(e).with_context(|| format!("failed to replace file: {}", path.display())),
    }?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn enable() -> anyhow::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let exe = current_exe()?;
    let exe_str = exe
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("executable path is not valid UTF-8"))?;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .context("failed to open HKCU Run key")?;

    // Quote the path to handle spaces.
    let value = format!("\"{}\"", exe_str);
    run_key
        .set_value(APP_NAME, &value)
        .context("failed to write HKCU Run value")?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn disable() -> anyhow::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = match hkcu.open_subkey_with_flags(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        winreg::enums::KEY_WRITE,
    ) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).context("failed to open HKCU Run key"),
    };

    match run_key.delete_value(APP_NAME) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("failed to delete HKCU Run value"),
    }
}

#[cfg(target_os = "macos")]
fn enable() -> anyhow::Result<()> {
    let plist_path = macos_plist_path()?;
    let args = macos_program_arguments()?;
    let contents = macos_launch_agent_plist(&args);
    atomic_write(&plist_path, &contents)?;

    let _ = std::process::Command::new("launchctl")
        .arg("load")
        .arg(&plist_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    Ok(())
}

#[cfg(target_os = "macos")]
fn disable() -> anyhow::Result<()> {
    let plist_path = macos_plist_path()?;

    let _ = std::process::Command::new("launchctl")
        .arg("unload")
        .arg(&plist_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match fs::remove_file(&plist_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("failed to remove {}", plist_path.display())),
    }
}

#[cfg(target_os = "macos")]
fn macos_plist_path() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join("com.easypassword.EasyPassword.plist"))
}

#[cfg(target_os = "macos")]
fn macos_program_arguments() -> anyhow::Result<Vec<String>> {
    let exe = current_exe()?;
    if let Some(app_bundle) = macos_app_bundle_path(&exe) {
        return Ok(vec![
            "/usr/bin/open".to_string(),
            "-a".to_string(),
            app_bundle.to_string_lossy().to_string(),
        ]);
    }

    Ok(vec![exe.to_string_lossy().to_string()])
}

#[cfg(target_os = "macos")]
fn macos_app_bundle_path(exe: &Path) -> Option<PathBuf> {
    // If running from an .app bundle, the executable path looks like:
    //   .../EasyPassword.app/Contents/MacOS/EasyPassword
    let mut cur = exe;
    while let Some(parent) = cur.parent() {
        if let Some(name) = parent.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".app") {
                return Some(parent.to_path_buf());
            }
        }
        cur = parent;
    }
    None
}

#[cfg(target_os = "macos")]
fn macos_launch_agent_plist(program_arguments: &[String]) -> String {
    let mut plist = String::new();
    plist.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    plist.push_str(
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
    );
    plist.push_str("<plist version=\"1.0\">\n");
    plist.push_str("<dict>\n");
    plist.push_str("  <key>Label</key>\n");
    plist.push_str("  <string>com.easypassword.EasyPassword</string>\n");
    plist.push_str("  <key>RunAtLoad</key>\n");
    plist.push_str("  <true/>\n");
    plist.push_str("  <key>ProgramArguments</key>\n");
    plist.push_str("  <array>\n");
    for arg in program_arguments {
        plist.push_str("    <string>");
        plist.push_str(&xml_escape(arg));
        plist.push_str("</string>\n");
    }
    plist.push_str("  </array>\n");
    plist.push_str("</dict>\n");
    plist.push_str("</plist>\n");
    plist
}

#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "linux")]
fn enable() -> anyhow::Result<()> {
    let desktop_path = linux_desktop_path()?;
    let exe = current_exe()?;
    let contents = linux_desktop_entry(&exe)?;
    atomic_write(&desktop_path, &contents)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn disable() -> anyhow::Result<()> {
    let desktop_path = linux_desktop_path()?;
    match fs::remove_file(&desktop_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("failed to remove {}", desktop_path.display())),
    }
}

#[cfg(target_os = "linux")]
fn linux_desktop_path() -> anyhow::Result<PathBuf> {
    let config_dir = dirs::config_dir().ok_or_else(|| anyhow::anyhow!("cannot find config dir"))?;
    Ok(config_dir.join("autostart").join("easypassword.desktop"))
}

#[cfg(target_os = "linux")]
fn linux_escape_exec(path: &Path) -> anyhow::Result<String> {
    let s = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("executable path is not valid UTF-8"))?;
    Ok(s.replace(' ', "\\ "))
}

#[cfg(target_os = "linux")]
fn linux_desktop_entry(exe: &Path) -> anyhow::Result<String> {
    let exec = linux_escape_exec(exe)?;
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName={APP_NAME}\nExec={exec}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    ))
}

#[cfg(all(
    not(target_os = "windows"),
    not(target_os = "macos"),
    not(target_os = "linux")
))]
fn enable() -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "autostart is not supported on this platform"
    ))
}

#[cfg(all(
    not(target_os = "windows"),
    not(target_os = "macos"),
    not(target_os = "linux")
))]
fn disable() -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "autostart is not supported on this platform"
    ))
}
