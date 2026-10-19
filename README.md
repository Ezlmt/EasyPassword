# EasyPassword

EasyPassword is a cross-platform, local-only, deterministic password generator that securely derives and injects site-specific passwords using Argon2id via global keyboard triggers.

Type `;;github.com` anywhere → get a unique, reproducible password instantly.

## Features

- **Deterministic**: Same inputs always produce the same password
- **Global**: Works in any application (browsers, editors, terminals)
- **Dual Modes**: Switch instantly between Secure (Argon2id) and Simple (Concatenation) modes via different prefixes
- **Local-only**: No network access, no cloud sync, no data leaves your machine
- **Secure**: Argon2id key derivation (OWASP 2025 recommended)
- **Fast**: Instant password generation and injection
- **Cross-platform**: Windows, macOS, and Linux support
- **Start on Login**: Automatically launch the app when you log in (configurable via tray menu or config)

## Installation

### From Source

```bash
git clone https://github.com/Ezlmt/EasyPassword.git
cd EasyPassword
cargo build --release
```

### Binary Location

After building: `./target/release/easypassword` (or `easypassword.exe` on Windows)

## Quick Start

### 1. Create Configuration

Create `config.toml` at:
- **Windows**: `%APPDATA%\easypassword\config.toml`
- **macOS**: `~/Library/Application Support/easypassword/config.toml`
- **Linux**: `~/.config/easypassword/config.toml`

```toml
[default]
master_key = "your-secret-master-key"
algorithm = "v2"
```

(If the file doesn't exist, the app creates one on first launch with `algorithm = "v2"`; you only need to fill in `master_key`.)

### 2. Run

```bash
./easypassword
```

### 3. Generate Passwords

In any text field, type:

```
;;github.com<SPACE>
```

The trigger text will be replaced with your generated password.

## Usage

### Trigger Format

```
[prefix][site][terminator]
```

| Component | Default | Description |
|-----------|---------|-------------|
| Prefix | `;;` | Configurable trigger prefix |
| Site | - | Domain or identifier (e.g., `github.com`) |
| Terminator | Space | Triggers password generation (configurable via `terminators`) |

### Examples

| Input | Action | Mode |
|-------|--------|------|
| `;;github.com ` | Generate secure password for github.com | **Argon2id** |
| `;;bank.example.com ` | Generate secure password for bank | **Argon2id** |
| `!!github.com ` | Generate simple password (`mastergithub.com`) | **Concatenation** |
| `!!local-dev ` | Generate simple password (`masterlocal-dev`) | **Concatenation** |

> **Note**: Concatenation mode does not add an implicit separator. If you want one (e.g. `master!github.com`), include it in your `master_key` (e.g. `master_key = "master!"`).

> **Note**: Site names are case-insensitive (`GitHub.com` = `github.com`)

> **Note**: Moving the caret (arrow keys, Esc, mouse click, Cmd/Ctrl shortcuts) cancels a partially typed trigger, so the password is never injected into the wrong place.

## Configuration

### Full Example

```toml
[default]
master_key = "your-secret-master-key"
algorithm = "v2"             # See "Algorithm versions" below
terminators = ["space"]      # Any of "space", "enter", "tab"
trigger_prefix = ";;"        # Triggers Argon2id mode
concat_trigger_prefix = "!!" # Triggers Concatenation mode
length = 16
lowercase = true
uppercase = true
digits = true
symbols = true
autostart = false            # Start on system login

# Per-site overrides. With algorithm = "v2", [sites.github] matches
# github.com and www.github.com too (an exact [sites."github.com"] wins).
[sites.github]
length = 20
symbols = false

# Keep an existing account on the legacy algorithm
[sites.old-bank]
algorithm = "v1"

# You can also force a specific mode for a site regardless of trigger
[sites.legacy-app]
mode = "concatenation" 
```

### Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `master_key` | string | (required) | Your secret master key |
| `trigger_prefix` | string | `;;` | Trigger for Argon2id mode |
| `concat_trigger_prefix` | string | `!!` | Trigger for Concatenation mode |
| `length` | integer | 16 | Generated password length |
| `mode` | string | "argon2id" | Default generation mode |
| `lowercase` | boolean | true | Include lowercase letters (a-z) |
| `uppercase` | boolean | true | Include uppercase letters (A-Z) |
| `digits` | boolean | true | Include digits (0-9) |
| `symbols` | boolean | true | Include symbols (!@#$%...) |
| `counter` | integer | 1 | Password version (increment to rotate) |
| `autostart` | boolean | false | Automatically start on system login |
| `algorithm` | string | `"v1"` if missing, `"v2"` in newly created configs | Derivation algorithm (also per-site) |
| `terminators` | list | `["space"]` | Keys that end a trigger: `space`, `enter`, `tab` |

> **Why only Space by default?** The terminator key is still delivered to the focused app. Enter would submit the form with the trigger text, and Tab would move focus so the backspaces land in the wrong field. Enable them only if you know your workflow tolerates it.

### Algorithm versions

| Version | Status | Notes |
|---------|--------|-------|
| `v1` | Legacy | Uses only 128 bits of the Argon2 output. Passwords of length ≥ 16 have biased character positions, and long ones degenerate (length 32 always starts with `!0Aa` and ends with `aaaaaaaa`). |
| `v2` | Recommended | Uses the full Argon2 output (≥ 64 bytes, scaled with length) and a domain-separated salt bound to the length. |

> [!IMPORTANT]
> Switching `algorithm` changes **every** generated password. To migrate, set `algorithm = "v2"` under `[default]` and add `algorithm = "v1"` to `[sites.<name>]` for accounts you have not rotated yet. Configs without an `algorithm` key keep using `v1`, so upgrading the app never changes your passwords silently.

## How It Works

### Password Generation

```
v1: password = render(Argon2id(master_key, SHA256(site || counter)))
v2: password = render(Argon2id(master_key, SHA256("easypassword/v2\0" || site || "\0" || counter || length)))
```

1. Combine site name and counter into a salt
2. Derive key material using Argon2id (memory-hard KDF)
3. Map derived bytes to configured character set
4. Ensure at least one character from each enabled class

### Security Properties

- **Deterministic**: Same `(master_key, site, counter)` → same password
- **Isolated**: Different sites produce completely different passwords
- **Irreversible**: Cannot derive master key from generated passwords
- **Memory-hard**: Argon2id resists GPU/ASIC attacks

### Argon2id Parameters

Following OWASP 2025 recommendations:
- Memory: 19 MiB (`m=19456`)
- Iterations: 2 (`t=2`)
- Parallelism: 1 (`p=1`)

## Platform Notes

The app includes a **Start on Login** feature that can be toggled via the system tray menu or the `autostart` config option.

### Windows
- **Start on Login**: Implemented via Registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (value: `EasyPassword`).
- No special permissions required.

### macOS

**Required Permissions** (both are mandatory):

1. **Input Monitoring**: Required for global keyboard monitoring
   - System Settings → Privacy & Security → Input Monitoring
   
2. **Accessibility**: Required for text injection
   - System Settings → Privacy & Security → Accessibility

**Important**: Add the correct application to both permission lists:
- If running from **Terminal/Kitty/iTerm**: Add your terminal app (e.g., `Kitty.app`, `Terminal.app`)
- If running the **`.app` bundle**: Add `EasyPassword.app`

After granting permissions, **restart the app** (or terminal) for changes to take effect.

**Building macOS App Bundle**:
```bash
./scripts/macos-bundle.sh
```
This creates `dist/EasyPassword.app` which can be moved to `/Applications`.

**Start on Login**: Implemented via LaunchAgent (`~/Library/LaunchAgents/com.easypassword.EasyPassword.plist`).

### Linux
- **Start on Login**: Implemented via XDG Autostart (`~/.config/autostart/easypassword.desktop`).

## Troubleshooting

### Program exits immediately

Check error message. Most common cause: `master_key` not set in config.

### Trigger not detected

1. Run with verbose flag: `./easypassword -v`
2. Check console for key events (look for `[MACOS-KEY]` or `[KEY]` logs)
3. Ensure a configured terminator (Space by default) is pressed after the trigger, without moving the caret in between
4. **macOS**: Verify Input Monitoring permission is granted to the correct app (terminal or .app bundle)

### Password not injected

1. Some applications block simulated input
2. Try a different application (Notepad, browser URL bar, TextEdit)
3. Check console for injection errors
4. **macOS**: Verify Accessibility permission is granted to the correct app

### Start on login not working
On macOS and Linux, the autostart configuration may require a logout and re-login to take effect for the first time.

## CLI Options

```
easypassword [OPTIONS]

Options:
  -v, --verbose  Enable verbose logging
  -h, --help     Print help
  -V, --version  Print version
```

## Security Considerations

- **Master key**: Store config file securely; it contains your master key (on macOS/Linux it is written with `0600` permissions)
- **Memory**: Master key is held in memory while running
- **Local only**: No network requests, no telemetry, no cloud
- **Open source**: Audit the code yourself

## License

MIT License

## Acknowledgments

- [Argon2](https://github.com/P-H-C/phc-winner-argon2) - Password hashing algorithm
- [rdev](https://github.com/Narsil/rdev) - Cross-platform input monitoring
- [enigo](https://github.com/enigo-rs/enigo) - Cross-platform input simulation
