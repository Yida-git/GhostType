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

const ASR_TYPE_OPTIONS = [
  { value: "websocket", label: "自建服务端 (WebSocket) - 已支持", supported: true },
  { value: "native", label: "系统原生 (未实现)", supported: false },
  { value: "cloud", label: "云端 API (未实现)", supported: false },
];

const CLOUD_PROVIDER_OPTIONS = [
  { value: "deepgram", label: "Deepgram" },
  { value: "xfyun", label: "讯飞" },
  { value: "aliyun", label: "阿里云" },
];

const LLM_TYPE_OPTIONS = [
  { value: "disabled", label: "禁用（仅输出 ASR）" },
  { value: "openai_compat", label: "OpenAI 兼容接口（通义/DeepSeek/OpenAI…）" },
  { value: "ollama", label: "本地 Ollama" },
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
          <p class="sub">按住热键 → 说话 → 松开 → 即时出字 → 500ms 后智能替换</p>
        </div>
      </header>

      <section id="permissionGuide" class="card cardWarn hidden">
        <h2>⚠️ 需要权限</h2>
        <div class="hint">GhostType 需要以下权限才能正常工作：</div>

        <div class="permGrid">
          <div id="permAccItem" class="permItem">
            <div class="permTitle">
              <span>🔐 辅助功能权限</span>
              <span id="permAccBadge" class="badge"></span>
            </div>
            <div class="hint">用于全局热键监听和文本注入</div>
            <div class="actions">
              <button id="permAccOpen" type="button" class="secondary">打开系统设置</button>
            </div>
          </div>

          <div id="permMicItem" class="permItem">
            <div class="permTitle">
              <span>🎤 麦克风权限 / 设备</span>
              <span id="permMicBadge" class="badge"></span>
            </div>
            <div class="hint">用于语音录制</div>
            <ul id="permDevices" class="deviceList"></ul>
            <div class="actions">
              <button id="permMicOpen" type="button" class="secondary">打开系统设置</button>
            </div>
          </div>
        </div>

        <div class="actions">
          <button id="permRefresh" type="button">刷新状态</button>
          <button id="permSkip" type="button" class="secondary">跳过，继续使用</button>
          <span id="permHint" class="status"></span>
        </div>
      </section>

      <section class="card">
        <h2>状态</h2>
        <div class="statusGrid">
          <div class="statusRow">
            <span>ASR（服务端）</span>
            <span id="summaryServer" class="badge"></span>
          </div>
          <div class="statusRow">
            <span>LLM 校正</span>
            <span id="summaryLlm" class="badge"></span>
          </div>
          <div class="statusRow">
            <span>热键</span>
            <span id="summaryHotkey" class="mono"></span>
          </div>
          <div class="statusRow">
            <span>音频输入</span>
            <span id="summaryAudio" class="mono"></span>
          </div>
          <div id="runtimeInfo" class="hint"></div>
          <div class="hint">提示：按住热键说话，松开后文字自动输入。</div>
        </div>
      </section>

      <section class="card">
        <h2>设置</h2>

        <div class="field">
          <label for="asrType">ASR 引擎</label>
          <select id="asrType"></select>
          <div id="asrTypeHint" class="hint">当前版本仅支持「自建服务端 (WebSocket)」。</div>
        </div>

        <div id="asrWebsocketFields">
          <div class="field">
            <label for="asrEndpoint">WebSocket 地址</label>
            <input id="asrEndpoint" type="text" placeholder="ws://127.0.0.1:8000/ws" spellcheck="false" />
            <div class="hint">保存后重启客户端生效（默认：ws://127.0.0.1:8000/ws）</div>
          </div>
        </div>

        <div id="asrCloudFields" class="hidden">
          <div class="field">
            <label for="asrCloudProvider">云端厂商</label>
            <select id="asrCloudProvider"></select>
          </div>
          <div class="field">
            <label for="asrCloudApiKey">API Key</label>
            <input id="asrCloudApiKey" type="password" placeholder="请输入 API Key" spellcheck="false" />
          </div>
          <div class="field">
            <label for="asrCloudRegion">Region（可选）</label>
            <input id="asrCloudRegion" type="text" placeholder="例如：cn / us / ap-shanghai" spellcheck="false" />
          </div>
          <div class="hint">提示：云端 ASR 目前尚未实现，此处仅做配置预留。</div>
        </div>

        <div id="asrNativeFields" class="hidden">
          <div class="hint">提示：系统原生 ASR（macOS/Windows）尚未实现，请先使用「自建服务端 (WebSocket)」。</div>
        </div>

        <div class="divider"></div>

        <div class="field">
          <label for="llmType">LLM 校正</label>
          <select id="llmType"></select>
          <div class="hint">两阶段管道：先输出 ASR，再延迟 500ms 用 LLM 校正并替换（可禁用）。</div>
        </div>

        <div id="llmOpenaiFields" class="hidden">
          <div class="field">
            <label for="llmOpenaiEndpoint">OpenAI 兼容端点</label>
            <input id="llmOpenaiEndpoint" type="text" placeholder="https://api.openai.com/v1" spellcheck="false" />
            <div class="hint">示例：通义 https://dashscope.aliyuncs.com/compatible-mode/v1；DeepSeek https://api.deepseek.com/v1</div>
          </div>
          <div class="field">
            <label for="llmOpenaiApiKey">API Key</label>
            <input id="llmOpenaiApiKey" type="password" placeholder="sk-..." spellcheck="false" />
          </div>
          <div class="field">
            <label for="llmOpenaiModel">模型</label>
            <input id="llmOpenaiModel" type="text" placeholder="gpt-4o-mini / qwen-turbo / deepseek-chat" spellcheck="false" />
          </div>
          <div class="field">
            <label for="llmOpenaiTimeout">超时（毫秒）</label>
            <input id="llmOpenaiTimeout" type="number" min="200" step="100" placeholder="3000" />
          </div>
          <div class="actions">
            <button id="testLlmOpenai" type="button" class="secondary">测试 LLM</button>
            <span id="llmOpenaiStatus" class="status"></span>
          </div>
        </div>

        <div id="llmOllamaFields" class="hidden">
          <div class="field">
            <label for="llmOllamaEndpoint">Ollama 地址</label>
            <input id="llmOllamaEndpoint" type="text" placeholder="http://localhost:11434" spellcheck="false" />
          </div>
          <div class="field">
            <label for="llmOllamaModel">模型</label>
            <input id="llmOllamaModel" type="text" placeholder="qwen2.5:1.5b / llama3.2" spellcheck="false" />
          </div>
          <div class="field">
            <label for="llmOllamaTimeout">超时（毫秒）</label>
            <input id="llmOllamaTimeout" type="number" min="200" step="100" placeholder="3000" />
          </div>
          <div class="actions">
            <button id="testLlmOllama" type="button" class="secondary">测试 LLM</button>
            <span id="llmOllamaStatus" class="status"></span>
          </div>
        </div>

        <div class="divider"></div>

        <div class="field">
          <label for="hotkeySelect">热键</label>
          <div class="hotkeyRow">
            <select id="hotkeySelect"></select>
            <input id="hotkeyCustom" type="text" placeholder="例如：f12 / capslock / right_shift" spellcheck="false" />
          </div>
          <div class="hint">仅支持单键（不支持组合键）；未知热键会回退到平台默认。</div>
        </div>

        <div class="field">
          <label for="audioDeviceSelect">音频输入设备</label>
          <select id="audioDeviceSelect"></select>
          <div class="hint">默认使用系统默认输入设备；如录音失败可手动指定（保存后重启生效）。</div>
        </div>

        <div class="field">
          <label for="configPath">配置文件</label>
          <input id="configPath" type="text" readonly />
        </div>

        <div class="actions">
          <button id="testConn" type="button" class="secondary">测试 ASR 连接</button>
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

function setBadge(id, message, kind = "info") {
  const node = el(id);
  node.textContent = message;
  node.dataset.kind = kind;
}

function setMono(id, message) {
  el(id).textContent = message;
}

function fillHotkeySelect() {
  const select = el("hotkeySelect");
  select.innerHTML = HOTKEY_PRESETS.map(
    (opt) => `<option value="${opt.value}">${opt.label}</option>`,
  ).join("");
}

function fillAsrTypeSelect() {
  const select = el("asrType");
  select.innerHTML = ASR_TYPE_OPTIONS.map((opt) => {
    const disabled = opt.supported ? "" : " disabled";
    return `<option value="${opt.value}"${disabled}>${opt.label}</option>`;
  }).join("");
}

function fillCloudProviderSelect() {
  const select = el("asrCloudProvider");
  select.innerHTML = CLOUD_PROVIDER_OPTIONS.map(
    (opt) => `<option value="${opt.value}">${opt.label}</option>`,
  ).join("");
}

function fillLlmTypeSelect() {
  const select = el("llmType");
  select.innerHTML = LLM_TYPE_OPTIONS.map(
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

function isValidUrlWithProtocols(raw, protocols) {
  try {
    const url = new URL(raw);
    if (!protocols.includes(url.protocol)) return false;
    return Boolean(url.hostname);
  } catch {
    return false;
  }
}

function isValidWsEndpoint(endpoint) {
  return isValidUrlWithProtocols(endpoint, ["ws:", "wss:"]);
}

function isValidHttpEndpoint(endpoint) {
  return isValidUrlWithProtocols(endpoint, ["http:", "https:"]);
}

function normalizeHotkey(selectValue, customValue) {
  if (selectValue === "__custom__") return (customValue || "").trim();
  return (selectValue || "").trim();
}

function normalizeAsrType(type) {
  const v = (type || "").trim();
  if (v === "web_socket") return "websocket";
  return v || "websocket";
}

function normalizeLlmType(type) {
  const v = (type || "").trim();
  if (v === "open_ai_compat") return "openai_compat";
  return v || "disabled";
}

function getAsrConfigFromUi() {
  const type = normalizeAsrType(el("asrType").value);
  if (type === "websocket") {
    const endpoint = normalizeEndpoint(el("asrEndpoint").value);
    return { type: "websocket", endpoint };
  }

  if (type === "native") {
    return { type: "native" };
  }

  if (type === "cloud") {
    const provider = (el("asrCloudProvider").value || "").trim() || "deepgram";
    const api_key = (el("asrCloudApiKey").value || "").trim();
    const regionValue = (el("asrCloudRegion").value || "").trim();
    const region = regionValue ? regionValue : null;
    return { type: "cloud", provider, api_key, region };
  }

  return { type: "websocket", endpoint: "" };
}

function getLlmConfigFromUi() {
  const type = normalizeLlmType(el("llmType").value);
  if (type === "disabled") return { type: "disabled" };

  if (type === "openai_compat") {
    const endpoint = (el("llmOpenaiEndpoint").value || "").trim();
    const api_key = (el("llmOpenaiApiKey").value || "").trim();
    const model = (el("llmOpenaiModel").value || "").trim();
    const timeout_ms = Number.parseInt(el("llmOpenaiTimeout").value || "3000", 10) || 3000;
    return { type: "openai_compat", endpoint, api_key, model, timeout_ms };
  }

  if (type === "ollama") {
    const endpoint = (el("llmOllamaEndpoint").value || "").trim();
    const model = (el("llmOllamaModel").value || "").trim();
    const timeout_ms = Number.parseInt(el("llmOllamaTimeout").value || "3000", 10) || 3000;
    return { type: "ollama", endpoint, model, timeout_ms };
  }

  return { type: "disabled" };
}

function applyAsrUi(asr) {
  const type = normalizeAsrType(asr && asr.type);
  el("asrType").value = type;

  el("asrWebsocketFields").classList.toggle("hidden", type !== "websocket");
  el("asrCloudFields").classList.toggle("hidden", type !== "cloud");
  el("asrNativeFields").classList.toggle("hidden", type !== "native");

  el("asrTypeHint").textContent =
    type === "websocket"
      ? "当前版本仅支持「自建服务端 (WebSocket)」。"
      : "该 ASR 类型当前尚未实现，请先使用「自建服务端 (WebSocket)」。";

  if (type === "websocket") {
    el("asrEndpoint").value = (asr && asr.endpoint) || "";
  } else if (type === "cloud") {
    el("asrCloudProvider").value = (asr && asr.provider) || "deepgram";
    el("asrCloudApiKey").value = (asr && asr.api_key) || "";
    el("asrCloudRegion").value = (asr && asr.region) || "";
  }
}

function applyLlmUi(llm) {
  const type = normalizeLlmType(llm && llm.type);
  el("llmType").value = type;

  el("llmOpenaiFields").classList.toggle("hidden", type !== "openai_compat");
  el("llmOllamaFields").classList.toggle("hidden", type !== "ollama");

  if (type === "openai_compat") {
    el("llmOpenaiEndpoint").value = (llm && llm.endpoint) || "";
    el("llmOpenaiApiKey").value = (llm && llm.api_key) || "";
    el("llmOpenaiModel").value = (llm && llm.model) || "";
    el("llmOpenaiTimeout").value = String((llm && llm.timeout_ms) || 3000);
  } else if (type === "ollama") {
    el("llmOllamaEndpoint").value = (llm && llm.endpoint) || "http://localhost:11434";
    el("llmOllamaModel").value = (llm && llm.model) || "";
    el("llmOllamaTimeout").value = String((llm && llm.timeout_ms) || 3000);
  }
}

function syncAsrVisibility() {
  const type = normalizeAsrType(el("asrType").value);
  el("asrWebsocketFields").classList.toggle("hidden", type !== "websocket");
  el("asrCloudFields").classList.toggle("hidden", type !== "cloud");
  el("asrNativeFields").classList.toggle("hidden", type !== "native");
  el("asrTypeHint").textContent =
    type === "websocket"
      ? "当前版本仅支持「自建服务端 (WebSocket)」。"
      : "该 ASR 类型当前尚未实现，请先使用「自建服务端 (WebSocket)」。";
}

function syncLlmVisibility() {
  const type = normalizeLlmType(el("llmType").value);
  el("llmOpenaiFields").classList.toggle("hidden", type !== "openai_compat");
  el("llmOllamaFields").classList.toggle("hidden", type !== "ollama");
}

async function loadConfig() {
  const resp = await invoke("load_client_config");
  return resp;
}

async function saveConfig(config) {
  const resp = await invoke("save_client_config", { config });
  return resp;
}

async function getRuntimeInfo() {
  return await invoke("get_runtime_info");
}

async function listAudioDevices() {
  return await invoke("list_audio_devices");
}

async function testServerConnection(endpoint) {
  return await invoke("test_server_connection", { endpoint });
}

async function checkPermissions() {
  return await invoke("check_permissions");
}

async function openAccessibilitySettings() {
  return await invoke("open_accessibility_settings");
}

async function openMicrophoneSettings() {
  return await invoke("open_microphone_settings");
}

async function testLlmHealth(llmConfig) {
  return await invoke("test_llm_health", { llm_config: llmConfig });
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
  fillAsrTypeSelect();
  fillCloudProviderSelect();
  fillLlmTypeSelect();
  const { applyHotkeyUi } = bindUi();

  let runtime = null;
  let permissionGuideDismissed = false;

  setStatus("正在加载配置…", "info");

  let currentConfig = null;
  let currentDevices = [];
  try {
    runtime = await getRuntimeInfo();
    el("runtimeInfo").textContent = `运行环境: ${runtime.os} / ${runtime.arch}`;

    currentDevices = await listAudioDevices();
    const select = el("audioDeviceSelect");
    select.innerHTML = [
      `<option value="__default__">(默认设备)</option>`,
      ...currentDevices.map((d) => {
        const suffix = d.is_default ? " (默认)" : "";
        return `<option value="${d.name}">${d.name}${suffix}</option>`;
      }),
    ].join("");

    const { config, path } = await loadConfig();
    currentConfig = config;

    applyAsrUi(config.asr || { type: "websocket", endpoint: "" });
    applyLlmUi(config.llm || { type: "disabled" });
    applyHotkeyUi(config.hotkey || "");
    const audioValue = config.audio_device || "__default__";
    el("audioDeviceSelect").value = audioValue;
    el("configPath").value = path || "(default / auto)";

    setStatus("配置已加载。", "ok");
  } catch (err) {
    setStatus(`配置加载失败：${err}`, "error");
  }

  async function refreshConnectionStatus() {
    const asr = getAsrConfigFromUi();
    if (asr.type !== "websocket") {
      setBadge("summaryServer", "未实现", "error");
      return;
    }

    const endpoint = normalizeEndpoint(asr.endpoint);
    if (!endpoint) {
      setBadge("summaryServer", "未设置", "error");
      return;
    }
    if (!isValidWsEndpoint(endpoint)) {
      setBadge("summaryServer", "无效地址", "error");
      return;
    }
    setBadge("summaryServer", "检测中…", "info");
    try {
      const ok = await testServerConnection(endpoint);
      setBadge("summaryServer", ok ? "● 已连接" : "● 未连接", ok ? "ok" : "error");
    } catch (err) {
      setBadge("summaryServer", "● 未连接", "error");
      setStatus(`连接测试失败：${err}`, "error");
    }
  }

  function updateSummary() {
    const asr = getAsrConfigFromUi();
    const llm = getLlmConfigFromUi();
    const endpoint = normalizeEndpoint(asr.type === "websocket" ? asr.endpoint : "");
    const hotkey = normalizeHotkey(el("hotkeySelect").value, el("hotkeyCustom").value) || "-";
    const audio = el("audioDeviceSelect").value;
    setMono("summaryHotkey", hotkey || "-");
    setMono("summaryAudio", audio === "__default__" ? "(默认设备)" : audio);
    if (!endpoint) {
      setBadge("summaryServer", "未设置", "error");
    } else if (!isValidWsEndpoint(endpoint)) {
      setBadge("summaryServer", "无效地址", "error");
    }

    if (llm.type === "disabled") {
      setBadge("summaryLlm", "已禁用", "info");
    } else if (llm.type === "openai_compat") {
      setBadge("summaryLlm", "OpenAI 兼容", "ok");
    } else if (llm.type === "ollama") {
      setBadge("summaryLlm", "Ollama", "ok");
    } else {
      setBadge("summaryLlm", "未知", "error");
    }
  }

  async function refreshPermissions() {
    if (permissionGuideDismissed) return;
    try {
      const status = await checkPermissions();

      if (runtime && runtime.os !== "macos") {
        el("permAccItem").classList.add("hidden");
        el("permAccBadge").textContent = "";
      }

      setBadge("permAccBadge", status.accessibility ? "✅ 已授权" : "❌ 未授权", status.accessibility ? "ok" : "error");
      setBadge("permMicBadge", status.microphone ? "✅ 可用" : "❌ 不可用", status.microphone ? "ok" : "error");

      const list = el("permDevices");
      list.innerHTML = currentDevices
        .slice(0, 8)
        .map((d) => `<li>${d.is_default ? "●" : "○"} ${d.name}</li>`)
        .join("");

      const needsGuide = !status.accessibility || !status.microphone;
      el("permissionGuide").classList.toggle("hidden", !needsGuide);
      el("permHint").textContent = needsGuide
        ? "请按提示授予权限/检查设备后点击“刷新状态”。"
        : "";
      el("permHint").dataset.kind = needsGuide ? "info" : "ok";
    } catch (err) {
      el("permissionGuide").classList.remove("hidden");
      el("permHint").textContent = `权限检测失败：${err}`;
      el("permHint").dataset.kind = "error";
    }
  }

  updateSummary();
  await refreshConnectionStatus();
  await refreshPermissions();

  el("asrType").addEventListener("change", () => {
    syncAsrVisibility();
    updateSummary();
  });

  el("llmType").addEventListener("change", () => {
    syncLlmVisibility();
    updateSummary();
  });

  el("testConn").addEventListener("click", async () => {
    setStatus("正在测试连接…", "info");
    await refreshConnectionStatus();
    setStatus("连接测试完成。", "ok");
  });

  el("permRefresh").addEventListener("click", async () => {
    el("permHint").textContent = "正在刷新…";
    el("permHint").dataset.kind = "info";
    await refreshPermissions();
  });

  el("permSkip").addEventListener("click", () => {
    permissionGuideDismissed = true;
    el("permissionGuide").classList.add("hidden");
  });

  el("permAccOpen").addEventListener("click", async () => {
    try {
      await openAccessibilitySettings();
    } catch (err) {
      el("permHint").textContent = `打开系统设置失败：${err}`;
      el("permHint").dataset.kind = "error";
    }
  });

  el("permMicOpen").addEventListener("click", async () => {
    try {
      await openMicrophoneSettings();
    } catch (err) {
      el("permHint").textContent = `打开系统设置失败：${err}`;
      el("permHint").dataset.kind = "error";
    }
  });

  el("testLlmOpenai").addEventListener("click", async () => {
    const llm = getLlmConfigFromUi();
    setBadge("summaryLlm", "检测中…", "info");
    el("llmOpenaiStatus").textContent = "检测中…";
    el("llmOpenaiStatus").dataset.kind = "info";
    try {
      const ok = await testLlmHealth(llm);
      el("llmOpenaiStatus").textContent = ok ? "✅ 可用" : "❌ 不可用";
      el("llmOpenaiStatus").dataset.kind = ok ? "ok" : "error";
      setBadge("summaryLlm", "OpenAI 兼容", ok ? "ok" : "error");
    } catch (err) {
      el("llmOpenaiStatus").textContent = `检测失败：${err}`;
      el("llmOpenaiStatus").dataset.kind = "error";
      setBadge("summaryLlm", "OpenAI 兼容", "error");
    }
  });

  el("testLlmOllama").addEventListener("click", async () => {
    const llm = getLlmConfigFromUi();
    setBadge("summaryLlm", "检测中…", "info");
    el("llmOllamaStatus").textContent = "检测中…";
    el("llmOllamaStatus").dataset.kind = "info";
    try {
      const ok = await testLlmHealth(llm);
      el("llmOllamaStatus").textContent = ok ? "✅ 可用" : "❌ 不可用";
      el("llmOllamaStatus").dataset.kind = ok ? "ok" : "error";
      setBadge("summaryLlm", "Ollama", ok ? "ok" : "error");
    } catch (err) {
      el("llmOllamaStatus").textContent = `检测失败：${err}`;
      el("llmOllamaStatus").dataset.kind = "error";
      setBadge("summaryLlm", "Ollama", "error");
    }
  });

  el("save").addEventListener("click", async () => {
    const asr = getAsrConfigFromUi();
    if (asr.type !== "websocket") {
      setStatus("当前版本仅支持「自建服务端 (WebSocket)」ASR，请先切换。", "error");
      return;
    }

    const endpoint = normalizeEndpoint(asr.endpoint);
    if (!endpoint) {
      setStatus("请输入 WebSocket 地址（例如 ws://127.0.0.1:8000/ws）", "error");
      return;
    }
    if (!isValidWsEndpoint(endpoint)) {
      setStatus("WebSocket 地址无效，请输入 ws:// 或 wss:// 开头的完整地址", "error");
      return;
    }

    const hotkey = normalizeHotkey(el("hotkeySelect").value, el("hotkeyCustom").value);
    if (!hotkey) {
      setStatus("请输入热键（或选择一个预设）", "error");
      return;
    }

    const audioDevice = el("audioDeviceSelect").value;
    const audio_device = audioDevice === "__default__" ? null : audioDevice;

    const llm = getLlmConfigFromUi();
    if (llm.type === "openai_compat") {
      if (!llm.endpoint) {
        setStatus("请输入 OpenAI 兼容端点（例如 https://api.openai.com/v1）", "error");
        return;
      }
      if (!isValidHttpEndpoint(llm.endpoint)) {
        setStatus("LLM 端点无效，请输入 http:// 或 https:// 开头的完整地址", "error");
        return;
      }
      if (!llm.api_key) {
        setStatus("请输入 OpenAI 兼容 API Key", "error");
        return;
      }
      if (!llm.model) {
        setStatus("请输入模型名（例如 gpt-4o-mini / qwen-turbo）", "error");
        return;
      }
    }
    if (llm.type === "ollama") {
      if (!llm.endpoint) {
        setStatus("请输入 Ollama 地址（例如 http://localhost:11434）", "error");
        return;
      }
      if (!isValidHttpEndpoint(llm.endpoint)) {
        setStatus("Ollama 地址无效，请输入 http:// 或 https:// 开头的完整地址", "error");
        return;
      }
      if (!llm.model) {
        setStatus("请输入 Ollama 模型名（例如 qwen2.5:1.5b）", "error");
        return;
      }
    }

    const next = {
      hotkey,
      audio_device,
      asr: { type: "websocket", endpoint },
      llm,
    };

    setStatus("正在保存…", "info");
    try {
      const { path } = await saveConfig(next);
      el("configPath").value = path || "(default / auto)";
      currentConfig = next;
      setStatus("已保存（重启客户端后生效）。", "ok");
      updateSummary();
      await refreshConnectionStatus();
      await refreshPermissions();
    } catch (err) {
      setStatus(`保存失败：${err}`, "error");
    }
  });
}

main();
