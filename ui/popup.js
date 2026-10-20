"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const el = {
  message: $("message"), main: $("main"), setup: $("setup"), hints: $("hints"),
  site: $("site"), suggestions: $("suggestions"), siteInfo: $("site-info"),
  advanced: $("advanced"), toggleAdvanced: $("toggle-advanced"),
  length: $("adv-length"), counter: $("adv-counter"), lower: $("adv-lower"),
  upper: $("adv-upper"), digits: $("adv-digits"), symbols: $("adv-symbols"),
  username: $("adv-username"), fingerprint: $("fingerprint"),
  mk1: $("mk1"), mk2: $("mk2"), identity: $("identity"),
  setupSave: $("setup-save"), setupHint: $("setup-hint"),
};

let status = null;
let suggestions = [];
let active = -1;
let loadedParams = null; // params for the site currently shown in "advanced"
let loadedFor = null; // raw input value loadedParams belongs to
let busy = false;

function showMessage(text, kind = "error") {
  if (!text) {
    el.message.classList.add("hidden");
    return;
  }
  el.message.textContent = text;
  el.message.className = `msg ${kind}`;
}

function errorText(e) {
  return typeof e === "string" ? e : (e && e.message) || String(e);
}

// ---------- status / views ----------

async function refresh() {
  showMessage(null);
  try {
    status = await invoke("get_status");
  } catch (e) {
    showMessage(errorText(e));
    return;
  }
  const isMac = status.platform === "macos";
  $("mod-key").textContent = isMac ? "⌘" : "Ctrl";
  const typeFirst = status.output === "type";
  $("hint-primary").textContent = typeFirst ? "输入到原窗口" : "复制";
  $("hint-alt").textContent = typeFirst ? "复制" : "输入到原窗口";

  if (status.has_seed) {
    el.fingerprint.textContent = status.fingerprint;
    el.fingerprint.classList.remove("hidden");
    el.setup.classList.add("hidden");
    el.main.classList.remove("hidden");
    el.hints.classList.remove("hidden");
    resetMain();
  } else {
    el.fingerprint.classList.add("hidden");
    el.main.classList.add("hidden");
    el.hints.classList.add("hidden");
    el.setup.classList.remove("hidden");
    el.identity.value = status.identity || "";
    el.mk1.value = el.mk2.value = "";
    el.setupHint.textContent = "";
    el.mk1.focus();
  }
}

function resetMain() {
  el.site.value = "";
  el.siteInfo.textContent = "";
  loadedParams = null;
  loadedFor = null;
  el.advanced.classList.add("hidden");
  el.toggleAdvanced.textContent = "选项 ▾";
  hideSuggestions();
  el.site.focus();
  updateSuggestions();
}

// ---------- suggestions ----------

function hideSuggestions() {
  el.suggestions.classList.add("hidden");
  active = -1;
}

function renderSuggestions() {
  el.suggestions.innerHTML = "";
  if (suggestions.length === 0) {
    hideSuggestions();
    return;
  }
  suggestions.forEach((s, i) => {
    const li = document.createElement("li");
    li.textContent = s;
    if (i === active) li.classList.add("active");
    li.addEventListener("mousedown", (ev) => {
      ev.preventDefault();
      pick(s);
    });
    el.suggestions.appendChild(li);
  });
  el.suggestions.classList.remove("hidden");
}

async function updateSuggestions() {
  const q = el.site.value;
  try {
    suggestions = await invoke("suggest", { query: q });
  } catch {
    suggestions = [];
  }
  // Don't suggest exactly what is typed.
  suggestions = suggestions.filter((s) => s !== q.trim().toLowerCase());
  active = -1;
  if (q.trim() === "" && document.activeElement !== el.site) {
    hideSuggestions();
    return;
  }
  renderSuggestions();
}

function pick(site) {
  el.site.value = site;
  hideSuggestions();
  loadParams();
  el.site.focus();
}

// ---------- per-site params ----------

let paramsTimer = null;
function scheduleLoadParams() {
  clearTimeout(paramsTimer);
  paramsTimer = setTimeout(loadParams, 150);
}

async function loadParams() {
  const raw = el.site.value;
  const site = raw.trim();
  if (!site) {
    el.siteInfo.textContent = "";
    loadedParams = null;
    loadedFor = null;
    return;
  }
  const p = await invoke("site_params", { site });
  loadedParams = p;
  loadedFor = raw;
  const r = p.rules;
  const kinds = [r.lowercase && "小写", r.uppercase && "大写", r.digits && "数字", r.symbols && "符号"].filter(Boolean);
  el.siteInfo.textContent =
    `${p.site} · ${r.length} 位 · ${kinds.join("/")}` +
    (p.counter > 1 ? ` · 第 ${p.counter} 版` : "") +
    (p.username ? ` · ${p.username}` : "");
  el.length.value = r.length;
  el.counter.value = p.counter;
  el.lower.checked = r.lowercase;
  el.upper.checked = r.uppercase;
  el.digits.checked = r.digits;
  el.symbols.value = r.symbols;
  el.username.value = p.username;
}

function currentParams() {
  if (!loadedParams) return null;
  if (el.advanced.classList.contains("hidden")) return loadedParams;
  return {
    site: loadedParams.site,
    counter: Math.max(1, parseInt(el.counter.value, 10) || 1),
    username: el.username.value,
    rules: {
      length: parseInt(el.length.value, 10) || loadedParams.rules.length,
      lowercase: el.lower.checked,
      uppercase: el.upper.checked,
      digits: el.digits.checked,
      symbols: el.symbols.value,
    },
  };
}

// ---------- submit ----------

async function submit(alternate) {
  if (busy) return;
  if (active >= 0 && suggestions[active]) {
    pick(suggestions[active]);
  }
  if (!el.site.value.trim()) {
    showMessage("请输入站点名", "warn");
    return;
  }
  busy = true;
  try {
    await loadParamsIfStale();
    const params = currentParams();
    const result = await invoke("submit", { params, alternate });
    if (!result.hidden && result.message) {
      showMessage(result.message, "warn");
    }
  } catch (e) {
    showMessage(errorText(e));
  } finally {
    busy = false;
  }
}

async function loadParamsIfStale() {
  clearTimeout(paramsTimer);
  if (!loadedParams || loadedFor !== el.site.value) {
    await loadParams();
  }
}

// ---------- setup ----------

async function saveSetup() {
  const a = el.mk1.value;
  const b = el.mk2.value;
  if (!a) return showMessage("请输入主密钥", "warn");
  if (a !== b) return showMessage("两次输入的主密钥不一致", "warn");
  if (a.length < 8) return showMessage("主密钥太短（至少 8 位，建议更长）", "warn");
  el.setupSave.disabled = true;
  el.setupHint.textContent = "正在计算（需要约 1 秒）…";
  try {
    const fp = await invoke("setup_master_key", { masterKey: a, identity: el.identity.value });
    el.mk1.value = el.mk2.value = "";
    await refresh();
    showMessage(`主密钥已保存。校验码 ${fp}：在其他设备上设置时应得到相同的校验码。`, "ok");
  } catch (e) {
    showMessage(errorText(e));
  } finally {
    el.setupSave.disabled = false;
    el.setupHint.textContent = "";
  }
}

// ---------- events ----------

el.site.addEventListener("input", () => {
  showMessage(null);
  updateSuggestions();
  scheduleLoadParams();
});
el.site.addEventListener("focus", updateSuggestions);
el.site.addEventListener("blur", () => setTimeout(hideSuggestions, 100));

el.toggleAdvanced.addEventListener("click", async () => {
  const opening = el.advanced.classList.contains("hidden");
  el.advanced.classList.toggle("hidden");
  el.toggleAdvanced.textContent = opening ? "选项 ▴" : "选项 ▾";
  if (opening) await loadParamsIfStale();
});

el.setupSave.addEventListener("click", saveSetup);
[el.mk1, el.mk2, el.identity].forEach((i) =>
  i.addEventListener("keydown", (e) => {
    if (e.key === "Enter") saveSetup();
  })
);

$("close").addEventListener("click", () => invoke("hide_popup"));
$("open-settings").addEventListener("click", () => invoke("open_settings"));

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    if (!el.suggestions.classList.contains("hidden")) hideSuggestions();
    else invoke("hide_popup");
    return;
  }
  if (el.main.classList.contains("hidden")) return;

  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    if (suggestions.length === 0) return;
    e.preventDefault();
    const n = suggestions.length;
    active = e.key === "ArrowDown" ? (active + 1) % n : (active - 1 + n) % n;
    renderSuggestions();
  } else if (e.key === "Tab" && active >= 0) {
    e.preventDefault();
    pick(suggestions[active]);
  } else if (e.key === "Enter" && !e.isComposing) {
    e.preventDefault();
    submit(e.ctrlKey || e.metaKey);
  }
});

listen("popup-shown", () => refresh());
listen("popup-error", (event) => showMessage(String(event.payload)));

refresh();
