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
```

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
| Terminator | Space/Enter/Tab | Triggers password generation |

### Examples

| Input | Action | Mode |
|-------|--------|------|
| `;;github.com ` | Generate secure password for github.com | **Argon2id** |
| `;;bank.example.com ` | Generate secure password for bank | **Argon2id** |
| `!!github.com ` | Generate simple password (`mastergithub.com`) | **Concatenation** |
| `!!local-dev ` | Generate simple password (`masterlocal-dev`) | **Concatenation** |

> **Note**: Concatenation mode does not add an implicit separator. If you want one (e.g. `master!github.com`), include it in your `master_key` (e.g. `master_key = "master!"`).

> **Note**: Site names are case-insensitive (`GitHub.com` = `github.com`)

## Configuration

### Full Example

```toml
[default]
master_key = "your-secret-master-key"
trigger_prefix = ";;"        # Triggers Argon2id mode
concat_trigger_prefix = "!!" # Triggers Concatenation mode
length = 16
lowercase = true
uppercase = true
digits = true
symbols = true
autostart = false            # Start on system login

# Per-site overrides
[sites.github]
length = 20
symbols = false

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

## How It Works

### Password Generation

```
password = render(Argon2id(master_key, SHA256(site || counter)))
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
- **Accessibility**: Required for text injection (System Settings → Privacy & Security → Accessibility).
- **Input Monitoring**: Required for global keyboard monitoring (System Settings → Privacy & Security → Input Monitoring).
- **Start on Login**: Implemented via LaunchAgent (`~/Library/LaunchAgents/com.easypassword.EasyPassword.plist`).

### Linux
- **Start on Login**: Implemented via XDG Autostart (`~/.config/autostart/easypassword.desktop`).

## Troubleshooting

### Program exits immediately

Check error message. Most common cause: `master_key` not set in config.

### Trigger not detected

1. Run with verbose flag: `./easypassword -v`
2. Check console for key events
3. Ensure terminator (Space/Enter/Tab) is pressed after trigger

### Password not injected

1. Some applications block simulated input
2. Try a different application (Notepad, browser URL bar)
3. Check console for injection errors

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

- **Master key**: Store config file securely; it contains your master key
- **Memory**: Master key is held in memory while running
- **Local only**: No network requests, no telemetry, no cloud
- **Open source**: Audit the code yourself

## License

MIT License

## Acknowledgments

- [Argon2](https://github.com/P-H-C/phc-winner-argon2) - Password hashing algorithm
- [rdev](https://github.com/Narsil/rdev) - Cross-platform input monitoring
- [enigo](https://github.com/enigo-rs/enigo) - Cross-platform input simulation
