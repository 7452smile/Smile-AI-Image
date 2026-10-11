use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use smile_core::{Preferences, SettingsSnapshot};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

mod codex;

const MARKETPLACE_NAME: &str = "smile-ai";
const PLUGIN_NAME: &str = "smile-ai-image";
const REPOSITORY_URL: &str = "https://github.com/7452smile/Smile-AI-Image";
const UPDATE_MANIFEST_URL: &str = "https://api.smile-ai-studio.com/smile-ai-image/latest.json";
const ALLOWED_WEB_HOST: &str = "api.smile-ai-studio.com";
const MAX_UPDATE_MANIFEST_BYTES: u64 = 64 * 1024;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginInstallStatus {
    codex_installed: bool,
    plugin_installed: bool,
    integration_mode: &'static str,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateManifest {
    version: String,
    download_page: String,
    notes: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStatus {
    update_available: bool,
    current_version: String,
    latest_version: String,
    download_page: Option<String>,
    notes: Option<String>,
}

#[tauri::command]
fn get_settings() -> Result<SettingsSnapshot, String> {
    smile_core::settings_snapshot().map_err(|error| error.to_string())
}

#[tauri::command]
fn save_preferences(preferences: Preferences) -> Result<(), String> {
    smile_core::save_preferences(&preferences).map_err(|error| error.to_string())
}

#[tauri::command]
fn restore_preferences() -> Result<(), String> {
    smile_core::save_preferences(&Preferences::default()).map_err(|error| error.to_string())
}

#[tauri::command]
async fn test_and_save_key(api_key: String) -> Result<Option<f64>, String> {
    let balance = smile_core::validate_key(&api_key)
        .await
        .map_err(|error| error.to_string())?;
    smile_core::save_api_key(&api_key).map_err(|error| error.to_string())?;
    Ok(balance)
}

#[tauri::command]
fn clear_api_key() -> Result<(), String> {
    smile_core::clear_api_key().map_err(|error| error.to_string())
}

#[tauri::command]
async fn check_balance() -> Result<Option<f64>, String> {
    smile_core::check_balance()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn check_for_updates() -> Result<UpdateStatus, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(format!("Smile-AI-Image/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(UPDATE_MANIFEST_URL)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| format!("检查更新失败：{error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_UPDATE_MANIFEST_BYTES)
    {
        return Err("版本信息响应过大".into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取版本信息失败：{error}"))?;
    if bytes.len() as u64 > MAX_UPDATE_MANIFEST_BYTES {
        return Err("版本信息响应过大".into());
    }
    let manifest: UpdateManifest =
        serde_json::from_slice(&bytes).map_err(|error| format!("版本信息格式无效：{error}"))?;
    build_update_status(env!("CARGO_PKG_VERSION"), manifest).map_err(|error| error.to_string())
}

fn build_update_status(current: &str, manifest: UpdateManifest) -> anyhow::Result<UpdateStatus> {
    let current_version = Version::parse(current)?;
    let latest_version = Version::parse(&manifest.version)?;
    let download_page = validate_smile_url(&manifest.download_page)?.to_string();
    let update_available = latest_version > current_version;
    Ok(UpdateStatus {
        update_available,
        current_version: current_version.to_string(),
        latest_version: latest_version.to_string(),
        download_page: update_available.then_some(download_page),
        notes: update_available.then_some(manifest.notes.trim().to_string()),
    })
}

fn validate_smile_url(url: &str) -> anyhow::Result<reqwest::Url> {
    let parsed = reqwest::Url::parse(url)?;
    let valid_port = parsed.port().is_none() || parsed.port() == Some(443);
    if parsed.scheme() != "https"
        || parsed.host_str() != Some(ALLOWED_WEB_HOST)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !valid_port
    {
        anyhow::bail!("不允许打开此地址");
    }
    Ok(parsed)
}

#[tauri::command]
async fn ensure_plugin_installed() -> Result<PluginInstallStatus, String> {
    tauri::async_runtime::spawn_blocking(install_plugin)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

fn install_plugin() -> anyhow::Result<PluginInstallStatus> {
    let executable = std::env::current_exe()?;
    #[cfg(target_os = "macos")]
    if executable.starts_with("/Volumes") {
        return Ok(PluginInstallStatus {
            codex_installed: true,
            plugin_installed: false,
            integration_mode: "none",
            message: "请先将 Smile AI Image 拖到“应用程序”后再打开".into(),
        });
    }

    let root = smile_core::config_dir()?.join("codex-marketplace");
    write_plugin_bundle(&root, &executable)?;
    codex::install(&root, &executable)
}

fn copy_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else {
            fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

fn write_plugin_bundle(root: &Path, executable: &Path) -> anyhow::Result<()> {
    let plugin = root.join("plugins").join(PLUGIN_NAME);
    fs::create_dir_all(plugin.join(".codex-plugin"))?;
    fs::create_dir_all(plugin.join("assets"))?;
    fs::create_dir_all(plugin.join("skills/smile-ai-image/agents"))?;
    fs::create_dir_all(plugin.join("skills/smile-ai-image/assets"))?;
    fs::create_dir_all(plugin.join("skills/smile-ai-image/references"))?;
    fs::create_dir_all(root.join(".agents/plugins"))?;

    let version = env!("CARGO_PKG_VERSION");
    let manifest = json!({
        "name": PLUGIN_NAME,
        "version": version,
        "description": "MIT-licensed open-source image generation and editing for Codex, powered by Smile AI.",
        "author": {"name": "Smile AI Studio", "url": "https://api.smile-ai-studio.com/"},
        "homepage": "https://api.smile-ai-studio.com/",
        "repository": REPOSITORY_URL,
        "license": "MIT",
            "keywords": ["image", "generation", "editing", "nano-banana", "nano-banana-2.1", "gpt-image", "gpt-image-2-all", "gpt-image-2.5-flare", "gpt-image-2.5-sunburst"],
        "skills": "./skills/",
        "interface": {
            "displayName": "Smile AI Image",
            "shortDescription": "MIT 开源，在 Codex 中生成和编辑图片。",
            "longDescription": "通过 Smile AI 中转站在 Codex 对话中调用 Nano Banana 2、Nano Banana 2.1、Nano Banana Pro、GPT Image 2、GPT Image 2 All、GPT Image 2.5 Flare 和 GPT Image 2.5 Sunburst，支持图片生成、编辑、本地保存与对话内预览。本插件基于 MIT 许可证开源。GitHub：https://github.com/7452smile/Smile-AI-Image；Smile AI 主站：https://api.smile-ai-studio.com/",
            "developerName": "Smile AI Studio",
            "category": "Productivity",
            "capabilities": ["Interactive", "Write"],
            "websiteURL": REPOSITORY_URL,
            "defaultPrompt": ["用 Smile AI Image 根据我的描述生成图片。", "用 Smile AI Image 编辑我上传的图片。", "打开 Smile AI Image 设置。"],
            "brandColor": "#C9FB24",
            "composerIcon": "./assets/icon.png",
            "logo": "./assets/logo.png"
        },
        "mcpServers": "./.mcp.json"
    });
    write_json(plugin.join(".codex-plugin/plugin.json"), &manifest)?;
    write_json(
        plugin.join("plugin.json"),
        &json!({
            "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name": PLUGIN_NAME,
            "version": version,
            "description": "MIT-licensed open-source image generation and editing for Codex, powered by Smile AI.",
            "author": {"name": "Smile AI Studio", "url": "https://api.smile-ai-studio.com/"},
            "homepage": "https://api.smile-ai-studio.com/",
            "repository": REPOSITORY_URL,
            "license": "MIT",
            "keywords": ["image", "generation", "editing", "nano-banana", "nano-banana-2.1", "gpt-image", "gpt-image-2", "gpt-image-2-all", "gpt-image-2.5-flare", "gpt-image-2.5-sunburst"],
            "extensions": {
                "com.openai": {
                    "interface": {
                        "displayName": "Smile AI Image",
                        "shortDescription": "MIT 开源，在 Codex 中生成和编辑图片。",
                        "longDescription": "通过 Smile AI 中转站在 Codex 对话中调用 Nano Banana 2、Nano Banana 2.1、Nano Banana Pro、GPT Image 2、GPT Image 2 All、GPT Image 2.5 Flare 和 GPT Image 2.5 Sunburst，支持图片生成、编辑、本地保存与对话内预览。本插件基于 MIT 许可证开源。GitHub：https://github.com/7452smile/Smile-AI-Image；Smile AI 主站：https://api.smile-ai-studio.com/",
                        "developerName": "Smile AI Studio",
                        "category": "Productivity",
                        "capabilities": ["Interactive", "Write"],
                        "websiteURL": REPOSITORY_URL,
                        "defaultPrompt": ["用 Smile AI Image 根据我的描述生成图片。", "用 Smile AI Image 编辑我上传的图片。", "打开 Smile AI Image 设置。"],
                        "brandColor": "#C9FB24",
                        "composerIcon": "./assets/icon.png",
                        "logo": "./assets/logo.png"
                    }
                }
            }
        }),
    )?;
    write_json(
        plugin.join(".mcp.json"),
        &json!({"mcpServers": {"smile_ai_image": {"command": executable, "args": ["--mcp"]}}}),
    )?;
    write_json(
        plugin.join("mcp.json"),
        &json!({
            "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers": {
                "smile_ai_image": {
                    "type": "stdio",
                    "command": executable,
                    "args": ["--mcp"]
                }
            }
        }),
    )?;
    fs::write(
        plugin.join("skills/smile-ai-image/SKILL.md"),
        include_str!("../../../skills/smile-ai-image/SKILL.md"),
    )?;
    for path in ["LICENSE", "skills/smile-ai-image/LICENSE"] {
        fs::write(plugin.join(path), include_str!("../../../LICENSE"))?;
    }
    fs::write(
        plugin.join("skills/smile-ai-image/agents/openai.yaml"),
        include_str!("../../../skills/smile-ai-image/agents/openai.yaml"),
    )?;
    fs::write(
        plugin.join("skills/smile-ai-image/references/model-capabilities.md"),
        include_str!("../../../skills/smile-ai-image/references/model-capabilities.md"),
    )?;
    for (name, bytes) in [
        (
            "icon.png",
            include_bytes!("../../../assets/icon.png").as_slice(),
        ),
        (
            "logo.png",
            include_bytes!("../../../assets/logo.png").as_slice(),
        ),
    ] {
        fs::write(plugin.join("assets").join(name), bytes)?;
        fs::write(
            plugin.join("skills/smile-ai-image/assets").join(name),
            bytes,
        )?;
    }
    write_json(
        root.join(".agents/plugins/marketplace.json"),
        &json!({
            "name": MARKETPLACE_NAME,
            "interface": {"displayName": "Smile AI"},
            "plugins": [{
                "name": PLUGIN_NAME,
                "source": {"source": "local", "path": "./plugins/smile-ai-image"},
                "policy": {"installation": "AVAILABLE", "authentication": "ON_INSTALL"},
                "category": "Creativity"
            }]
        }),
    )?;
    Ok(())
}

fn write_json(path: PathBuf, value: &serde_json::Value) -> anyhow::Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    let url = validate_smile_url(&url).map_err(|error| error.to_string())?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_repository(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(REPOSITORY_URL, None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_codex() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        for app in ["Codex", "ChatGPT"] {
            if Command::new("open")
                .args(["-a", app])
                .status()
                .is_ok_and(|status| status.success())
            {
                return Ok(());
            }
        }
        Err("未找到 Codex 或 ChatGPT 应用".into())
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer.exe")
            .arg(r"shell:AppsFolder\OpenAI.Codex_2p2nqsd0c76g0!App")
            .spawn()
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_preferences,
            restore_preferences,
            test_and_save_key,
            clear_api_key,
            check_balance,
            check_for_updates,
            ensure_plugin_installed,
            open_url,
            open_repository,
            open_codex,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Smile AI Image settings");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str, download_page: &str) -> UpdateManifest {
        UpdateManifest {
            version: version.into(),
            download_page: download_page.into(),
            notes: "更新说明".into(),
        }
    }

    #[test]
    fn newer_version_is_available() {
        let status = build_update_status(
            "1.0.6",
            manifest(
                "1.0.7",
                "https://api.smile-ai-studio.com/smile-ai-image/download/",
            ),
        )
        .unwrap();
        assert!(status.update_available);
        assert_eq!(status.latest_version, "1.0.7");
        assert!(status.download_page.is_some());
    }

    #[test]
    fn same_or_older_version_is_not_available() {
        for latest in ["1.0.6", "1.0.5"] {
            let status = build_update_status(
                "1.0.6",
                manifest(
                    latest,
                    "https://api.smile-ai-studio.com/smile-ai-image/download/",
                ),
            )
            .unwrap();
            assert!(!status.update_available);
            assert!(status.download_page.is_none());
            assert!(status.notes.is_none());
        }
    }

    #[test]
    fn invalid_versions_are_rejected() {
        assert!(
            build_update_status(
                "1.0.6",
                manifest(
                    "newest",
                    "https://api.smile-ai-studio.com/smile-ai-image/download/"
                )
            )
            .is_err()
        );
    }

    #[test]
    fn unsafe_download_urls_are_rejected() {
        for url in [
            "http://api.smile-ai-studio.com/smile-ai-image/download/",
            "https://api.smile-ai-studio.com.example.com/download/",
            "https://api.smile-ai-studio.com@evil.example/download/",
            "https://api.smile-ai-studio.com:8443/download/",
        ] {
            assert!(build_update_status("1.0.6", manifest("1.0.7", url)).is_err());
        }
    }
}
