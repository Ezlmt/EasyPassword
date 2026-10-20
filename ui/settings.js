"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);

let view = null; // SettingsView from Rust

/** In-page replacement for window.confirm (not reliable in every webview). */
function ask(text) {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `<div class="dialog"><p></p><div class="row" style="justify-content:flex-end">
      <button data-v="0">取消</button><button class="primary" data-v="1">确定</button></div></div>`;
    overlay.querySelector("p").textContent = text;
    const done = (v) => {
      overlay.remove();
      document.removeEventListener("keydown", onKey, true);
      resolve(v);
    };
    const onKey = (e) => {
      if (e.key === "Escape") { e.preventDefault(); done(false); }
      if (e.key === "Enter") { e.preventDefault(); done(true); }
    };
    overlay.querySelectorAll("button").forEach((b) => b.addEventListener("click", () => done(b.dataset.v === "1")));
    document.addEventListener("keydown", onKey, true);
    document.body.appendChild(overlay);
    overlay.querySelector("button.primary").focus();
  });
}
let recording = false;

function msg(text, kind = "ok") {
  const m = $("message");
  if (!text) return m.classList.add("hidden");
  m.textContent = text;
  m.className = `msg grow ${kind}`;
  if (kind === "ok") setTimeout(() => m.classList.add("hidden"), 2500);
}
const err = (e) => msg(typeof e === "string" ? e : String(e), "error");

async function load() {
  try {
    view = await invoke("get_settings");
  } catch (e) {
    return err(e);
  }
  const s = view.settings;
  const isMac = view.platform === "macos";
  document.querySelectorAll(".mod").forEach((n) => (n.textContent = isMac ? "⌘" : "Ctrl"));

  $("key-state").innerHTML = view.has_seed
    ? `已设置 · 校验码 <span class="chip">${view.fingerprint}</span>` +
      (s.identity ? ` · 身份 ${escapeHtml(s.identity)}` : "")
    : `<span style="color:var(--danger)">未设置</span>`;
  $("forget-key").disabled = !view.has_seed;
  $("identity").value = s.identity;

  $("shortcut").value = s.shortcut;
  document.querySelectorAll('input[name="output"]').forEach((r) => (r.checked = r.value === s.output));
  $("clear-secs").value = s.clipboard_clear_seconds;
  $("autostart").checked = view.autostart;

  $("d-length").value = s.defaults.length;
  $("d-symbols").value = s.defaults.symbols;
  $("d-lower").checked = s.defaults.lowercase;
  $("d-upper").checked = s.defaults.uppercase;
  $("d-digits").checked = s.defaults.digits;
  $("remember").checked = s.remember_sites;

  renderSites();

  if (isMac) {
    const ok = await invoke("check_input_permission");
    $("perm").classList.toggle("hidden", ok);
  }
}

function escapeHtml(t) {
  return t.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}

function renderSites() {
  const tbody = $("sites");
  tbody.innerHTML = "";
  const entries = Object.entries(view.settings.sites).sort((a, b) => b[1].last_used - a[1].last_used);
  if (entries.length === 0) {
    tbody.innerHTML = `<tr><td colspan="6" class="muted">还没有站点</td></tr>`;
    return;
  }
  for (const [site, e] of entries) {
    const tr = document.createElement("tr");
    const when = e.last_used ? new Date(e.last_used * 1000).toLocaleDateString() : "";
    tr.innerHTML = `
      <td>${escapeHtml(site)}</td>
      <td><input type="number" min="1" value="${e.counter}" data-k="counter"></td>
      <td><input type="number" min="4" max="128" value="${e.length ?? ""}" placeholder="默认" data-k="length"></td>
      <td><input type="text" value="${escapeHtml(e.username || "")}" data-k="username"></td>
      <td class="muted">${when}</td>
      <td><button class="link danger" title="删除">删除</button></td>`;
    tr.querySelectorAll("input").forEach((input) =>
      input.addEventListener("change", () => {
        const k = input.dataset.k;
        if (k === "counter") e.counter = Math.max(1, parseInt(input.value, 10) || 1);
        else if (k === "length") e.length = input.value === "" ? null : parseInt(input.value, 10);
        else e.username = input.value.trim();
      })
    );
    tr.querySelector("button").addEventListener("click", async () => {
      if (!await ask(`删除 ${site} 的记录？（自定义设置会丢失，密码将按默认规则生成）`)) return;
      try {
        await invoke("delete_site", { site });
        delete view.settings.sites[site];
        renderSites();
      } catch (ex) {
        err(ex);
      }
    });
    tbody.appendChild(tr);
  }
}

function collect() {
  const s = structuredClone(view.settings);
  s.shortcut = $("shortcut").value.trim();
  s.output = document.querySelector('input[name="output"]:checked')?.value || "type";
  s.clipboard_clear_seconds = Math.max(0, parseInt($("clear-secs").value, 10) || 0);
  s.remember_sites = $("remember").checked;
  s.defaults = {
    length: parseInt($("d-length").value, 10) || 20,
    lowercase: $("d-lower").checked,
    uppercase: $("d-upper").checked,
    digits: $("d-digits").checked,
    symbols: $("d-symbols").value,
  };
  for (const e of Object.values(s.sites)) {
    if (e.length === null || e.length === undefined) delete e.length;
  }
  return s;
}

// ---------- actions ----------

$("save").addEventListener("click", async () => {
  const s = collect();
  const d = view.settings.defaults;
  const rulesChanged = JSON.stringify(s.defaults) !== JSON.stringify(d);
  if (rulesChanged && !await ask("修改默认规则会改变所有未单独设置过的站点的密码，确定吗？")) return;
  try {
    await invoke("save_settings", { settings: s });
    msg("已保存");
    await load();
  } catch (e) {
    err(e);
  }
});

$("autostart").addEventListener("change", async (ev) => {
  try {
    const actual = await invoke("set_autostart", { enabled: ev.target.checked });
    ev.target.checked = actual;
  } catch (e) {
    ev.target.checked = !ev.target.checked;
    err(e);
  }
});

$("forget-history").addEventListener("click", async () => {
  if (!await ask("清除所有没有自定义设置的站点记录？")) return;
  try {
    await invoke("forget_history");
    await load();
    msg("已清除");
  } catch (e) {
    err(e);
  }
});

$("change-key").addEventListener("click", () => {
  $("change-form").classList.toggle("hidden");
  $("mk1").focus();
});

$("change-save").addEventListener("click", async () => {
  const a = $("mk1").value;
  if (!a) return msg("请输入主密钥", "error");
  if (a !== $("mk2").value) return msg("两次输入的主密钥不一致", "error");
  if (a.length < 8) return msg("主密钥太短（至少 8 位）", "error");
  $("change-save").disabled = true;
  $("change-hint").textContent = "正在计算…";
  try {
    const fp = await invoke("setup_master_key", { masterKey: a, identity: $("identity").value });
    $("mk1").value = $("mk2").value = "";
    $("change-form").classList.add("hidden");
    await load();
    msg(`主密钥已更新，校验码 ${fp}`);
  } catch (e) {
    err(e);
  } finally {
    $("change-save").disabled = false;
    $("change-hint").textContent = "";
  }
});

$("forget-key").addEventListener("click", async () => {
  if (!await ask("从本机钥匙串移除主密钥？之后需要重新输入同一个主密钥才能生成密码。")) return;
  try {
    await invoke("forget_master_key");
    await load();
    msg("已移除");
  } catch (e) {
    err(e);
  }
});

// ---------- shortcut recorder ----------

function codeToKey(code) {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return code; // Semicolon, Space, F5, Slash, ...
}

$("record").addEventListener("click", () => {
  recording = true;
  $("record").textContent = "请按下组合键…";
  $("shortcut").focus();
});

document.addEventListener("keydown", (e) => {
  if (!recording) return;
  if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return;
  e.preventDefault();
  recording = false;
  $("record").textContent = "录制";
  if (e.key === "Escape") return;
  const isMac = view && view.platform === "macos";
  const parts = [];
  if (isMac ? e.metaKey : e.ctrlKey) parts.push("CommandOrControl");
  if (isMac && e.ctrlKey) parts.push("Control");
  if (!isMac && e.metaKey) parts.push("Super");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (parts.length === 0) {
    msg("快捷键需要至少一个修饰键（Ctrl/⌘/Alt/Shift）", "error");
    return;
  }
  parts.push(codeToKey(e.code));
  $("shortcut").value = parts.join("+");
});

listen("settings-shown", load);
load();
