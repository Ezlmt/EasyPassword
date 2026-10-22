# EasyPassword 2

按一个全局快捷键 → 输入站点名 → 回车，EasyPassword 就把一个**强壮、可重现**的密码输入到你刚才所在的窗口。
密码由主密钥实时推导，**不保存、不同步、不联网**。

*Press a global shortcut, type a site, hit Enter — EasyPassword types a strong, reproducible password derived from your master key into the window you came from. Nothing is stored or synced.*

> [!IMPORTANT]
> 2.x 的新算法与 0.x 的 `;;site`（Argon2）密码**不兼容**；0.x 的 `!!site` **拼接密码完全兼容**，见下文。

## 使用

| 操作 | 说明 |
|---|---|
| <kbd>⌘/Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>P</kbd> | 打开生成器（可在设置中修改） |
| 输入站点名 | 可以直接粘贴网址，`https://www.github.com/login` 会被识别为 `github.com`；支持历史补全（<kbd>↑</kbd><kbd>↓</kbd>/<kbd>Tab</kbd>） |
| <kbd>Enter</kbd> | 把密码输入到原窗口（默认） |
| <kbd>⌘/Ctrl</kbd>+<kbd>Enter</kbd> | 复制到剪贴板，20 秒后自动清除（不会进入剪贴板历史/云同步） |
| 选项 ▾ | 为该站点单独设置长度、字符类型、符号集、版本号（换密码时 +1）、用户名备注 |
| <kbd>Esc</kbd> | 关闭 |

### 继续使用旧版拼接密码（`!!site`）

旧版的拼接密码 = `master_key` + 站点名（原样，区分大小写）。2.x 原样支持：

1. 首次设置时，在「旧版拼接前缀」一栏填入旧 `config.toml` 里的 `master_key`（之后也可在 设置 → 旧版拼接密码 中修改）。填写后新站点**默认用拼接**，与旧版输出逐字符一致。
2. 想把某个站点换成更安全的新算法：在弹窗「选项」里切到「新算法」，再去网站上改密码。
3. 弹窗里输入 `!!github.com` 也会强制按拼接生成（并记住该站点用拼接）。
4. 每个站点第一次使用时就会固定它的方式，之后修改默认方式不会改变已有站点的密码。

前缀保存在系统钥匙串，不再明文写在配置文件里；也不再监听键盘，聊天时打 `!!` 不会误触发。

> [!WARNING]
> 拼接密码本身很弱：任何一个网站泄露，其他网站的密码都能被猜出来。建议逐步把重要网站切到新算法。

首次启动会要求设置主密钥，并显示一个 5 位**校验码**。在另一台设备上输入同一个主密钥（和身份）应得到相同的校验码——这样就能确认没有打错。

同一网站多个账号：用 `work@github.com`、`personal@github.com` 这样的站点名即可。

## 安全设计

- **不监听键盘**。只注册一个全局快捷键，不需要「输入监控」权限，不会误触发，也不受输入法影响。
- **主密钥不落盘**。输入后用 Argon2id（64 MiB, t=3）推导出 32 字节种子，种子存进 **macOS 钥匙串 / Windows 凭据管理器**；配置文件里没有任何秘密。
- 每个站点的密码 = `HKDF-SHA256(种子, 站点 ‖ 版本号 ‖ 规则)`，按规则做无偏的进制转换，保证每种启用的字符类型至少出现一次。
- 内存中的密码、种子在用完后清零；日志永远不会记录输入的内容。
- 单实例运行；剪贴板在超时后、且内容仍是该密码时才清除。

算法细节见 [core/src/algorithm.rs](core/src/algorithm.rs)，另有一份独立的 Python 实现 [scripts/reference.py](scripts/reference.py)，CI 会核对两者输出完全一致。

## 平台说明

### macOS
- 需要「**辅助功能**」权限才能把密码输入到其他窗口：系统设置 → 隐私与安全性 → 辅助功能 → 允许 EasyPassword。未授权时会自动改为复制到剪贴板并提示。
- 当前输入法是中文/日文等非 ASCII 输入法时，会自动改用「粘贴」方式（随后恢复原剪贴板内容），避免被输入法吞字。
- 浏览器密码框开启的 Secure Input 不影响使用（不需要监听键盘）。
- 安装包是 ad-hoc 签名、未公证：首次打开请右键 → 打开。每次更新后系统可能要求重新授权辅助功能 / 钥匙串访问。

### Windows
- 无需额外权限。无法向「以管理员身份运行」的窗口输入（Windows 的 UIPI 限制），这种情况请用 <kbd>Ctrl</kbd>+<kbd>Enter</kbd> 复制。
- 安装包未签名，SmartScreen 可能提示，选择「仍要运行」。

Linux 暂不支持（核心库可在 Linux 编译和测试）。

## 构建

```bash
# 运行（开发）
cargo run -p easypassword

# 打包（.dmg / .exe 安装包）
cd app && npx @tauri-apps/cli@^2 build

# 测试核心算法
cargo test -p easypassword-core
```

项目结构：

```
core/      纯 Rust 核心：算法、站点名规范化、设置（无 GUI 依赖，全部单元测试）
app/       Tauri 2 桌面应用：快捷键、托盘、钥匙串、输入/剪贴板
ui/        弹窗与设置页（原生 HTML/JS，无构建步骤）
scripts/   Python 参考实现、图标生成脚本
```

## License

MIT
