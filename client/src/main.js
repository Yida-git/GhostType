import "./style.css";
import { invoke } from "@tauri-apps/api/core";

const HOTKEY_PRESETS = [
  { value: "capslock", label: "CapsLock (推荐 - Windows/Linux)" },
  { value: "f8", label: "F8 (推荐 - macOS)" },
  { value: "f5", label: "F5 (macOS 系统听写同款)" },
  { value: "f6", label: "F6 (讯飞输入法同款)" },
  { value: "f7", label: "F7 (Linux/通用)" },
  { value: "right_shift", label: "Right Shift (CapsWriter macOS)" },
  { value: "__custom__", label: "自定义..." },
];

function el(id) {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing element: #${id}`);
  return node;
}

function render() {
  document.querySelector("#app").innerHTML = `
    <div class="container">
      <header class="header">
        <div>
          <h1>GhostType</h1>
          <p class="sub">Push-to-talk 语音转文字（托盘优先 MVP）</p>
        </div>
      </header>

      <section class="card">
        <h2>设置</h2>

        <div class="field">
          <label for="server">服务器地址</label>
          <input id="server" type="text" placeholder="ws://127.0.0.1:8000/ws" spellcheck="false" />
          <div class="hint">WebSocket 地址（保存后重启客户端生效）</div>
        </div>

        <div class="field">
          <label for="hotkeySelect">热键</label>
          <div class="hotkeyRow">
            <select id="hotkeySelect"></select>
            <input id="hotkeyCustom" type="text" placeholder="例如：f12 / capslock / right_shift" spellcheck="false" />
          </div>
          <div class="hint">仅支持单键（不支持组合键）；未知热键会回退到平台默认。</div>
        </div>

        <div class="field">
          <label for="configPath">配置文件</label>
          <input id="configPath" type="text" readonly />
        </div>

        <div class="actions">
          <button id="save" type="button">保存配置</button>
          <span id="status" class="status"></span>
        </div>
      </section>
    </div>
  `;
}

function setStatus(message, kind = "info") {
  const node = el("status");
  node.textContent = message;
  node.dataset.kind = kind;
}

function fillHotkeySelect() {
  const select = el("hotkeySelect");
  select.innerHTML = HOTKEY_PRESETS.map(
    (opt) => `<option value="${opt.value}">${opt.label}</option>`,
  ).join("");
}

function isPresetHotkey(value) {
  return HOTKEY_PRESETS.some((opt) => opt.value === value && opt.value !== "__custom__");
}

function normalizeEndpoint(raw) {
  const endpoint = (raw || "").trim();
  if (!endpoint) return "";
  return endpoint;
}

function normalizeHotkey(selectValue, customValue) {
  if (selectValue === "__custom__") return (customValue || "").trim();
  return (selectValue || "").trim();
}

async function loadConfig() {
  const resp = await invoke("load_client_config");
  return resp;
}

async function saveConfig(config) {
  const resp = await invoke("save_client_config", { config });
  return resp;
}

function bindUi() {
  const select = el("hotkeySelect");
  const custom = el("hotkeyCustom");

  function applyHotkeyUi(value) {
    if (isPresetHotkey(value)) {
      select.value = value;
      custom.value = "";
      custom.classList.add("hidden");
    } else {
      select.value = "__custom__";
      custom.value = value || "";
      custom.classList.remove("hidden");
    }
  }

  select.addEventListener("change", () => {
    if (select.value === "__custom__") {
      custom.classList.remove("hidden");
      custom.focus();
    } else {
      custom.classList.add("hidden");
      custom.value = "";
    }
  });

  return { applyHotkeyUi };
}

async function main() {
  render();
  fillHotkeySelect();
  const { applyHotkeyUi } = bindUi();

  setStatus("正在加载配置…", "info");

  let currentConfig = null;
  try {
    const { config, path } = await loadConfig();
    currentConfig = config;

    const endpoint = (config.server_endpoints && config.server_endpoints[0]) || "";
    el("server").value = endpoint;
    applyHotkeyUi(config.hotkey || "");
    el("configPath").value = path || "(default / auto)";

    setStatus("配置已加载。", "ok");
  } catch (err) {
    setStatus(`配置加载失败：${err}`, "error");
  }

  el("save").addEventListener("click", async () => {
    const endpoint = normalizeEndpoint(el("server").value);
    if (!endpoint) {
      setStatus("请输入服务器地址（例如 ws://127.0.0.1:8000/ws）", "error");
      return;
    }

    const hotkey = normalizeHotkey(el("hotkeySelect").value, el("hotkeyCustom").value);
    if (!hotkey) {
      setStatus("请输入热键（或选择一个预设）", "error");
      return;
    }

    const next = {
      server_endpoints: [endpoint],
      use_cloud_api: Boolean(currentConfig && currentConfig.use_cloud_api),
      hotkey,
    };

    setStatus("正在保存…", "info");
    try {
      const { path } = await saveConfig(next);
      el("configPath").value = path || "(default / auto)";
      currentConfig = next;
      setStatus("已保存（重启客户端后生效）。", "ok");
    } catch (err) {
      setStatus(`保存失败：${err}`, "error");
    }
  });
}

main();

