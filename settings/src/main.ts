import { invoke } from "@tauri-apps/api/core";
import { createIcons, Download, Eye, EyeOff, ExternalLink, RefreshCw, RotateCcw } from "lucide";
import "./styles.css";

type Model = "nano-banana2" | "nano-banana-2.1" | "nano-banana-pro" | "gpt-image2" | "gpt-image2-all" | "gpt-image-2.5-flare" | "gpt-image-2.5-sunburst";
type Resolution = "AUTO" | "ONEK" | "TWOK" | "FOURK";
type Quality = "AUTO" | "LOW" | "MEDIUM" | "HIGH";

interface Preferences {
  model: Model;
  resolution: Resolution;
  aspectRatio: string;
  quality: Quality;
  theme: "system" | "light" | "dark";
  language: "system" | "zh-CN" | "en";
}

interface Snapshot {
  preferences: Preferences;
  hasApiKey: boolean;
  maskedApiKey?: string;
  apiBase: string;
  version: string;
}

interface PluginInstallStatus {
  codexInstalled: boolean;
  pluginInstalled: boolean;
  integrationMode: "plugin" | "mcp" | "none";
  message: string;
}

interface UpdateStatus {
  updateAvailable: boolean;
  currentVersion: string;
  latestVersion: string;
  downloadPage?: string;
  notes?: string;
}

const ratios: Record<Model, string[]> = {
  "nano-banana2": ["auto", "1:1", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16", "5:4", "4:5", "21:9", "4:1", "1:4", "8:1", "1:8"],
  "nano-banana-2.1": ["auto", "1:1", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16", "5:4", "4:5", "21:9", "4:1", "1:4", "8:1", "1:8"],
  "nano-banana-pro": ["auto", "1:1", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16", "5:4", "4:5", "21:9"],
  "gpt-image2": ["auto", "1:1", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16", "5:4", "4:5", "2:1", "1:2", "21:9", "9:21", "3:1", "1:3", "custom"],
  "gpt-image2-all": ["auto", "1:1", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16", "5:4", "4:5", "2:1", "1:2", "21:9", "9:21", "3:1", "1:3", "custom"],
  "gpt-image-2.5-flare": ["auto", "1:1", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16", "5:4", "4:5", "2:1", "1:2", "21:9", "9:21", "3:1", "1:3", "custom"],
  "gpt-image-2.5-sunburst": ["auto", "1:1", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16", "5:4", "4:5", "2:1", "1:2", "21:9", "9:21", "3:1", "1:3", "custom"],
};

const gptImageModels: readonly Model[] = ["gpt-image2", "gpt-image2-all", "gpt-image-2.5-flare", "gpt-image-2.5-sunburst"];

const translations = {
  "zh-CN": {
    openSource: "MIT 开源", website: "Smile AI 主站",
    subtitle: "Codex 图片模型设置", language: "语言", theme: "主题", account: "账户", accountHint: "API Key 安全保存在本机系统凭据库",
    testSave: "测试并保存", clearKey: "清除", balance: "账户余额", lowBalance: "余额较低", refresh: "刷新", relay: "进入中转站",
    defaults: "默认生成参数", defaultsHint: "Codex 下一次生成时立即使用", model: "模型", resolution: "分辨率", ratio: "图片比例", quality: "图片质量",
    qualityAuto: "自动", qualityLow: "低", qualityMedium: "中", qualityHigh: "高",
    customRatio: "自定义比例", customHint: "GPT Image 2 / 2.5 支持 1:3 至 3:1", codexOnly: "图片生成和编辑请在 Codex 对话中完成。",
    restore: "恢复默认设置", openCodex: "打开 Codex", configured: "已配置", unconfigured: "未配置", synced: "已同步到 Codex", restored: "已恢复默认设置", retry: "重试",
    pluginReady: "Codex 插件已安装；请开启新对话", mcpReady: "Codex 接入已配置；请重启并开启新对话",
    updateAvailable: "发现新版本", downloadUpdate: "下载更新", checkUpdate: "检查更新", latestVersion: "已是最新版本", updateFailed: "检查失败", auto: "自动", light: "浅色", dark: "深色",
  },
  en: {
    openSource: "MIT open source", website: "Smile AI website",
    subtitle: "Image model settings for Codex", language: "Language", theme: "Theme", account: "Account", accountHint: "Your API Key is stored in the system credential vault",
    testSave: "Test & Save", clearKey: "Clear", balance: "Balance", lowBalance: "Low balance", refresh: "Refresh", relay: "Open Relay",
    defaults: "Default generation", defaultsHint: "Used by the next Codex image request", model: "Model", resolution: "Resolution", ratio: "Aspect ratio", quality: "Image quality",
    qualityAuto: "Auto", qualityLow: "Low", qualityMedium: "Medium", qualityHigh: "High",
    customRatio: "Custom ratio", customHint: "GPT Image 2 / 2.5 supports ratios from 1:3 to 3:1", codexOnly: "Generate and edit images in a Codex conversation.",
    restore: "Restore Defaults", openCodex: "Open Codex", configured: "Configured", unconfigured: "Not configured", synced: "Synced to Codex", restored: "Defaults restored", retry: "Retry",
    pluginReady: "Codex plugin installed; start a new chat", mcpReady: "Codex connection configured; restart and start a new chat",
    updateAvailable: "New version available", downloadUpdate: "Download", checkUpdate: "Check for Updates", latestVersion: "Up to date", updateFailed: "Check failed", auto: "Auto", light: "Light", dark: "Dark",
  },
};

const $ = <T extends HTMLElement>(selector: string): T => {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`Missing element: ${selector}`);
  return element;
};

let current: Snapshot;
let saveTimer: number | undefined;
let updateDownloadPage: string | undefined;
const isTauri = "__TAURI_INTERNALS__" in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return invoke<T>(command, args);
  if (command === "get_settings") {
    return {
      preferences: { model: "nano-banana2", resolution: "FOURK", aspectRatio: "auto", quality: "AUTO", theme: "system", language: "system" },
      hasApiKey: false,
      apiBase: "https://api.smile-ai-studio.com",
      version: "1.0.9",
    } as T;
  }
  if (command === "check_for_updates") {
    const showPreview = new URLSearchParams(location.search).has("update-preview");
    return {
      updateAvailable: showPreview,
      currentVersion: "1.0.9",
      latestVersion: showPreview ? "1.0.10" : "1.0.9",
      downloadPage: showPreview ? "https://api.smile-ai-studio.com/smile-ai-image/download/" : undefined,
      notes: showPreview ? "支持新模型并改进生成稳定性。" : undefined,
    } as T;
  }
  if (command === "save_preferences" || command === "restore_preferences" || command === "open_url") return undefined as T;
  throw new Error(effectiveLanguage() === "en" ? "This action is available in the desktop app." : "此操作仅在桌面应用中可用。");
}

function effectiveLanguage(): "zh-CN" | "en" {
  const selected = $("#language") as HTMLSelectElement;
  if (selected.value === "en") return "en";
  if (selected.value === "zh-CN") return "zh-CN";
  return navigator.language.toLowerCase().startsWith("en") ? "en" : "zh-CN";
}

function t(key: keyof typeof translations["zh-CN"]): string { return translations[effectiveLanguage()][key]; }

function translate(): void {
  document.documentElement.lang = effectiveLanguage();
  document.querySelectorAll<HTMLElement>("[data-i18n]").forEach((element) => {
    const key = element.dataset.i18n as keyof typeof translations["zh-CN"];
    element.textContent = t(key);
  });
  renderKeyState();
}

function applyTheme(): void {
  const value = ($("#theme") as HTMLSelectElement).value;
  document.documentElement.dataset.theme = value;
}

function renderKeyState(): void {
  const chip = $("#key-status");
  chip.textContent = current?.hasApiKey ? t("configured") : t("unconfigured");
  chip.className = `status-chip ${current?.hasApiKey ? "success" : "neutral"}`;
  const input = $("#api-key") as HTMLInputElement;
  input.placeholder = current?.maskedApiKey || (effectiveLanguage() === "en" ? "Enter API Key" : "输入 API Key");
}

function renderRatios(preferred?: string): void {
  const model = ($("#model") as HTMLSelectElement).value as Model;
  const select = $("#ratio") as HTMLSelectElement;
  const requested = preferred || select.value || "auto";
  select.replaceChildren(...ratios[model].map((ratio) => {
    const option = document.createElement("option");
    option.value = ratio;
    option.textContent = ratio === "auto" ? (effectiveLanguage() === "en" ? "Auto" : "自动") : ratio === "custom" ? (effectiveLanguage() === "en" ? "Custom..." : "自定义...") : ratio;
    return option;
  }));
  select.value = ratios[model].includes(requested) ? requested : "auto";
  renderRatioNote();
}

function renderRatioNote(): void {
  const model = ($("#model") as HTMLSelectElement).value as Model;
  const ratio = ($("#ratio") as HTMLSelectElement).value;
  $("#custom-ratio-row").hidden = ratio !== "custom";
  const note = $("#ratio-note");
  if (model === "nano-banana2" || model === "nano-banana-2.1") note.textContent = effectiveLanguage() === "en" ? "Includes 1:4, 4:1, 1:8 and 8:1 extended ratios. Choose 4K for 4096px output." : "支持 1:4、4:1、1:8、8:1 超长比例；需要 4096 像素时请选择 4K。";
  else if (model === "nano-banana-pro") note.textContent = effectiveLanguage() === "en" ? "Extended 1:4 and 1:8 ratios are unavailable on Pro." : "Pro 不支持 1:4、4:1、1:8、8:1 超长比例。";
  else note.textContent = effectiveLanguage() === "en" ? "Dimensions are calculated automatically within GPT Image 2 / 2.5 limits." : "按 GPT Image 2 / 2.5 限制自动换算合法像素尺寸。";
}

function renderQualityVisibility(): void {
  const enabled = gptImageModels.includes(($("#model") as HTMLSelectElement).value as Model);
  $("#quality-field").hidden = !enabled;
  $(".control-grid").classList.toggle("has-quality", enabled);
}

function readPreferences(): Preferences {
  const ratioChoice = ($("#ratio") as HTMLSelectElement).value;
  const custom = ($("#custom-ratio") as HTMLInputElement).value.trim();
  return {
    model: ($("#model") as HTMLSelectElement).value as Model,
    resolution: ($("#resolution") as HTMLSelectElement).value as Resolution,
    aspectRatio: ratioChoice === "custom" ? custom : ratioChoice,
    quality: ($("#quality") as HTMLSelectElement).value as Quality,
    theme: ($("#theme") as HTMLSelectElement).value as Preferences["theme"],
    language: ($("#language") as HTMLSelectElement).value as Preferences["language"],
  };
}

async function savePreferences(): Promise<void> {
  try {
    const preferences = readPreferences();
    await call("save_preferences", { preferences });
    current.preferences = preferences;
    $("#sync-status").textContent = t("synced");
    window.setTimeout(() => { $("#sync-status").textContent = ""; }, 1800);
  } catch (error) { showMessage(String(error), true); }
}

function scheduleSave(): void {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void savePreferences(), 250);
}

function showMessage(message: string, isError = false): void {
  const element = $("#account-message");
  element.textContent = message;
  element.classList.toggle("error", isError);
}

async function refreshBalance(): Promise<void> {
  const button = $("#refresh-balance") as HTMLButtonElement;
  button.disabled = true;
  $("#balance-value").textContent = "...";
  try {
    const value = await call<number | null>("check_balance");
    $("#balance-value").textContent = value == null ? (effectiveLanguage() === "en" ? "Unavailable" : "暂时无法读取") : `$${value.toFixed(2)}`;
    $("#balance-warning").hidden = value == null || value >= 1;
  } catch (error) {
    $("#balance-value").textContent = effectiveLanguage() === "en" ? "Unavailable" : "暂时无法读取";
    showMessage(String(error), true);
  } finally { button.disabled = false; }
}

async function init(): Promise<void> {
  if (isTauri) await installPlugin();
  current = await call<Snapshot>("get_settings");
  ($("#model") as HTMLSelectElement).value = current.preferences.model;
  ($("#resolution") as HTMLSelectElement).value = current.preferences.resolution;
  ($("#quality") as HTMLSelectElement).value = current.preferences.quality;
  ($("#theme") as HTMLSelectElement).value = current.preferences.theme;
  ($("#language") as HTMLSelectElement).value = current.preferences.language;
  renderRatios(ratios[current.preferences.model].includes(current.preferences.aspectRatio) ? current.preferences.aspectRatio : "custom");
  renderQualityVisibility();
  if (($("#ratio") as HTMLSelectElement).value === "custom") ($("#custom-ratio") as HTMLInputElement).value = current.preferences.aspectRatio;
  applyTheme();
  translate();
  $("#version").textContent = `v${current.version}`;
  if (current.hasApiKey) void refreshBalance();
  void checkForUpdates(true);
}

function showUpdate(status: UpdateStatus): void {
  updateDownloadPage = status.downloadPage;
  $("#update-banner").hidden = !status.updateAvailable;
  $("#update-version").textContent = status.updateAvailable ? `v${status.latestVersion}` : "";
  $("#update-notes").textContent = status.notes || "";
}

async function checkForUpdates(silent = false): Promise<void> {
  const button = $("#check-update") as HTMLButtonElement;
  const label = $("#check-update-label");
  button.disabled = true;
  if (!silent) label.textContent = "...";
  try {
    const status = await call<UpdateStatus>("check_for_updates");
    showUpdate(status);
    if (!silent) label.textContent = status.updateAvailable ? t("updateAvailable") : t("latestVersion");
  } catch {
    if (!silent) label.textContent = t("updateFailed");
  } finally {
    button.disabled = false;
    if (!silent) window.setTimeout(() => { label.textContent = t("checkUpdate"); }, 2200);
  }
}

async function installPlugin(): Promise<void> {
  const status = $("#plugin-status");
  const dot = $("#plugin-dot");
  const retry = $("#retry-install") as HTMLButtonElement;
  retry.hidden = true;
  retry.disabled = true;
  status.textContent = effectiveLanguage() === "en" ? "Checking Codex plugin..." : "正在检查 Codex 插件...";
  dot.className = "plugin-dot neutral";
  try {
    const result = await call<PluginInstallStatus>("ensure_plugin_installed");
    status.textContent = result.pluginInstalled
      ? t(result.integrationMode === "mcp" ? "mcpReady" : "pluginReady")
      : result.message;
    dot.className = `plugin-dot ${result.pluginInstalled ? "success" : "error"}`;
    retry.hidden = result.pluginInstalled;
    $("#open-codex").hidden = !result.codexInstalled;
  } catch (error) {
    status.textContent = String(error);
    dot.className = "plugin-dot error";
    retry.hidden = false;
  } finally {
    retry.disabled = false;
  }
}

$("#model").addEventListener("change", () => { renderRatios(); renderQualityVisibility(); scheduleSave(); });
$("#resolution").addEventListener("change", scheduleSave);
$("#ratio").addEventListener("change", () => { renderRatioNote(); if (($("#ratio") as HTMLSelectElement).value !== "custom") scheduleSave(); });
$("#custom-ratio").addEventListener("change", scheduleSave);
$("#quality").addEventListener("change", scheduleSave);
$("#theme").addEventListener("change", () => { applyTheme(); scheduleSave(); });
$("#language").addEventListener("change", () => { translate(); renderRatios(($("#ratio") as HTMLSelectElement).value); scheduleSave(); });

$("#toggle-key").addEventListener("click", () => {
  const input = $("#api-key") as HTMLInputElement;
  input.type = input.type === "password" ? "text" : "password";
  $("#toggle-key").innerHTML = `<span data-lucide="${input.type === "password" ? "eye" : "eye-off"}"></span>`;
  createIcons({ icons: { Download, Eye, EyeOff, ExternalLink, RefreshCw, RotateCcw } });
});

$("#save-key").addEventListener("click", async () => {
  const key = ($("#api-key") as HTMLInputElement).value.trim();
  if (!key) { showMessage(effectiveLanguage() === "en" ? "Enter a new API Key first." : "请先输入新的 API Key。", true); return; }
  const button = $("#save-key") as HTMLButtonElement;
  button.disabled = true;
  try {
    const balance = await call<number | null>("test_and_save_key", { apiKey: key });
    current = await call<Snapshot>("get_settings");
    ($("#api-key") as HTMLInputElement).value = "";
    renderKeyState();
    showMessage(effectiveLanguage() === "en" ? "API Key verified and saved." : "API Key 验证成功并已保存。", false);
    $("#balance-value").textContent = balance == null ? (effectiveLanguage() === "en" ? "Unavailable" : "暂时无法读取") : `$${balance.toFixed(2)}`;
  } catch (error) { showMessage(String(error), true); }
  finally { button.disabled = false; }
});

$("#clear-key").addEventListener("click", async () => {
  if (!confirm(effectiveLanguage() === "en" ? "Clear the saved API Key?" : "确定清除已保存的 API Key？")) return;
  try { await call("clear_api_key"); current = await call<Snapshot>("get_settings"); renderKeyState(); $("#balance-value").textContent = "--"; showMessage(""); }
  catch (error) { showMessage(String(error), true); }
});

$("#refresh-balance").addEventListener("click", () => void refreshBalance());
$("#open-relay").addEventListener("click", () => void call("open_url", { url: current.apiBase }));
$("#open-source").addEventListener("click", (event) => {
  if (!isTauri) return;
  event.preventDefault();
  void call("open_repository").catch((error) => showMessage(String(error), true));
});
$("#open-website").addEventListener("click", (event) => {
  if (!isTauri) return;
  event.preventDefault();
  void call("open_url", { url: "https://api.smile-ai-studio.com/" }).catch((error) => showMessage(String(error), true));
});
$("#open-codex").addEventListener("click", () => void call("open_codex"));
$("#check-update").addEventListener("click", () => void checkForUpdates());
$("#download-update").addEventListener("click", () => {
  if (updateDownloadPage) void call("open_url", { url: updateDownloadPage });
});
$("#retry-install").addEventListener("click", () => void installPlugin());
$("#restore").addEventListener("click", async () => {
  if (!confirm(effectiveLanguage() === "en" ? "Restore default preferences? Your API Key will be kept." : "恢复默认设置？已保存的 API Key 会保留。")) return;
  const button = $("#restore") as HTMLButtonElement;
  button.disabled = true;
  try {
    await call("restore_preferences");
    await init();
    $("#sync-status").textContent = t("restored");
    window.setTimeout(() => { $("#sync-status").textContent = ""; }, 2200);
  } catch (error) {
    showMessage(String(error), true);
  } finally {
    button.disabled = false;
  }
});

createIcons({ icons: { Download, Eye, EyeOff, ExternalLink, RefreshCw, RotateCcw } });
void init().catch((error) => showMessage(String(error), true));
