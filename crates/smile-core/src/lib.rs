use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use directories::ProjectDirs;
use futures_util::{StreamExt, stream};
use keyring::Entry;
use reqwest::{Client, StatusCode, multipart};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const API_BASE: &str = "https://api.smile-ai-studio.com";
const KEYRING_SERVICE: &str = "com.smile-ai.image";
const KEYRING_ACCOUNT: &str = "api-key";
pub const MAX_GENERATION_COUNT: usize = 20;
pub const MAX_CONCURRENT_GENERATIONS: usize = 6;

mod image_result;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImageModel {
    #[default]
    NanoBanana2,
    #[serde(rename = "nano-banana-2.1")]
    NanoBanana21,
    NanoBananaPro,
    GptImage2,
    GptImage2All,
    #[serde(rename = "gpt-image-2.5-flare")]
    GptImage25Flare,
    #[serde(rename = "gpt-image-2.5-sunburst")]
    GptImage25Sunburst,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Resolution {
    Auto,
    OneK,
    TwoK,
    #[default]
    FourK,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ImageQuality {
    #[default]
    Auto,
    Low,
    Medium,
    High,
}

impl ImageQuality {
    fn as_api_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub model: ImageModel,
    pub resolution: Resolution,
    pub aspect_ratio: String,
    #[serde(default)]
    pub quality: ImageQuality,
    pub theme: String,
    pub language: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            model: ImageModel::NanoBanana2,
            resolution: Resolution::FourK,
            aspect_ratio: "auto".into(),
            quality: ImageQuality::Auto,
            theme: "system".into(),
            language: "system".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub preferences: Preferences,
    pub has_api_key: bool,
    pub masked_api_key: Option<String>,
    pub api_base: &'static str,
    pub version: &'static str,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    pub prompt: String,
    #[serde(default)]
    pub variant_prompts: Vec<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub model: Option<ImageModel>,
    #[serde(default)]
    pub resolution: Option<Resolution>,
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    #[serde(default)]
    pub quality: Option<ImageQuality>,
    #[serde(default = "default_count")]
    pub count: u8,
    #[serde(default)]
    pub input_images: Vec<PathBuf>,
    #[serde(default)]
    pub output_dir: Option<PathBuf>,
}

fn default_count() -> u8 {
    1
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedImage {
    pub path: PathBuf,
    pub mime_type: String,
    #[serde(skip_serializing)]
    pub base64: String,
    pub dimensions: Option<PixelDimensions>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PixelDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationParameters {
    pub model: ImageModel,
    pub api_model: String,
    pub resolution: Resolution,
    pub aspect_ratio: String,
    pub quality: Option<ImageQuality>,
    pub image_size: String,
    pub expected_dimensions: Option<PixelDimensions>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationFailure {
    pub index: u8,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationOutcome {
    pub requested: u8,
    pub parameters: GenerationParameters,
    pub images: Vec<GeneratedImage>,
    pub failures: Vec<GenerationFailure>,
}

pub fn config_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("com", "SmileAI", "Smile AI Image")
        .ok_or_else(|| anyhow!("无法定位本地配置目录"))?;
    Ok(dirs.config_dir().to_path_buf())
}

pub fn load_preferences() -> Result<Preferences> {
    let path = config_dir()?.join("settings.json");
    if !path.exists() {
        return Ok(Preferences::default());
    }
    let bytes = fs::read(&path).with_context(|| format!("读取设置失败: {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes).context("设置文件格式无效")?)
}

pub fn save_preferences(value: &Preferences) -> Result<()> {
    validate_preferences(value)?;
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    let target = dir.join("settings.json");
    let temp = dir.join("settings.json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(temp, target)?;
    Ok(())
}

pub fn validate_preferences(value: &Preferences) -> Result<()> {
    if !["system", "light", "dark"].contains(&value.theme.as_str()) {
        bail!("不支持的主题设置");
    }
    if !["system", "zh-CN", "en"].contains(&value.language.as_str()) {
        bail!("不支持的语言设置");
    }
    validate_ratio(value.model, &value.aspect_ratio)
}

fn keyring_entry() -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).context("无法访问系统凭据库")
}

pub fn read_api_key() -> Result<Option<String>> {
    match keyring_entry()?.get_password() {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(anyhow!("读取系统凭据失败: {error}")),
    }
}

pub fn save_api_key(value: &str) -> Result<()> {
    let value = value.trim();
    if value.len() < 8 {
        bail!("API Key 格式无效");
    }
    keyring_entry()?
        .set_password(value)
        .context("保存 API Key 到系统凭据失败")
}

pub fn clear_api_key() -> Result<()> {
    match keyring_entry()?.delete_credential() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(anyhow!("删除系统凭据失败: {error}")),
    }
}

pub fn mask_key(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "••••••••".into();
    }
    format!(
        "{}••••••{}",
        chars[..4].iter().collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>()
    )
}

pub fn settings_snapshot() -> Result<SettingsSnapshot> {
    let key = read_api_key()?;
    Ok(SettingsSnapshot {
        preferences: load_preferences()?,
        has_api_key: key.is_some(),
        masked_api_key: key.as_deref().map(mask_key),
        api_base: API_BASE,
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub fn supported_ratios(model: ImageModel) -> &'static [&'static str] {
    match model {
        ImageModel::NanoBanana2 | ImageModel::NanoBanana21 => &[
            "auto", "1:1", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16", "5:4", "4:5", "21:9", "4:1",
            "1:4", "8:1", "1:8",
        ],
        ImageModel::NanoBananaPro => &[
            "auto", "1:1", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16", "5:4", "4:5", "21:9",
        ],
        ImageModel::GptImage2
        | ImageModel::GptImage2All
        | ImageModel::GptImage25Flare
        | ImageModel::GptImage25Sunburst => &[
            "auto", "1:1", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16", "5:4", "4:5", "2:1", "1:2",
            "21:9", "9:21", "3:1", "1:3",
        ],
    }
}

pub fn validate_ratio(model: ImageModel, ratio: &str) -> Result<()> {
    if ratio == "auto" || supported_ratios(model).contains(&ratio) {
        return Ok(());
    }
    if is_gpt_image_model(model) {
        let (w, h) = parse_ratio(ratio)?;
        let value = w / h;
        if (1.0 / 3.0..=3.0).contains(&value) {
            return Ok(());
        }
    }
    bail!("当前模型不支持比例 {ratio}")
}

fn parse_ratio(value: &str) -> Result<(f64, f64)> {
    let (w, h) = value
        .split_once(':')
        .ok_or_else(|| anyhow!("比例格式应为 宽:高"))?;
    let w: f64 = w.parse().context("比例宽度无效")?;
    let h: f64 = h.parse().context("比例高度无效")?;
    if w <= 0.0 || h <= 0.0 {
        bail!("比例必须大于 0");
    }
    Ok((w, h))
}

pub fn gpt_size(resolution: Resolution, ratio: &str) -> Result<String> {
    if resolution == Resolution::Auto {
        return Ok("auto".into());
    }
    // A concrete resolution must never become an automatic (often 1K) size.
    let ratio = if ratio == "auto" { "1:1" } else { ratio };
    let (rw, rh) = parse_ratio(ratio)?;
    let ratio_value = rw / rh;
    if !(1.0 / 3.0..=3.0).contains(&ratio_value) {
        bail!("GPT Image 2 比例必须在 1:3 到 3:1 之间");
    }
    let pixels = match resolution {
        Resolution::OneK => 1_048_576.0,
        Resolution::TwoK => 4_194_304.0,
        Resolution::FourK => 8_294_400.0,
        Resolution::Auto => unreachable!(),
    };
    let mut width = (pixels * ratio_value).sqrt();
    let mut height = width / ratio_value;
    let edge_scale = 3840.0_f64.min(3840.0 / width.max(height));
    if edge_scale < 1.0 {
        width *= edge_scale;
        height *= edge_scale;
    }
    let align = |v: f64| -> u32 { ((v / 16.0).round() as u32).max(16) * 16 };
    let mut width = align(width);
    let mut height = align(height);
    while u64::from(width) * u64::from(height) > 8_294_400 {
        if width >= height {
            width -= 16;
        } else {
            height -= 16;
        }
    }
    Ok(format!("{width}x{height}"))
}

fn banana_model_id(model: ImageModel, resolution: Resolution) -> Result<String> {
    let base = match model {
        ImageModel::NanoBanana2 => "gemini-3.1-flash-image-preview",
        ImageModel::NanoBanana21 => "gemini-nano-banana-2.1",
        ImageModel::NanoBananaPro => "gemini-3-pro-image-preview",
        ImageModel::GptImage2
        | ImageModel::GptImage2All
        | ImageModel::GptImage25Flare
        | ImageModel::GptImage25Sunburst => bail!("GPT Image 不是 Banana 模型"),
    };
    let suffix = match resolution {
        Resolution::Auto | Resolution::OneK => "1k",
        Resolution::TwoK => "2k",
        Resolution::FourK => "4k",
    };
    Ok(format!("{base}-{suffix}"))
}

fn effective_resolution(
    model: ImageModel,
    requested: Option<Resolution>,
    saved: Resolution,
) -> Resolution {
    let resolution = requested.unwrap_or(saved);
    if !is_gpt_image_model(model) && resolution == Resolution::Auto {
        // AUTO is not a Banana output size. Preserve the user's concrete default.
        if saved == Resolution::Auto {
            Resolution::OneK
        } else {
            saved
        }
    } else {
        resolution
    }
}

fn banana_image_size(resolution: Resolution) -> &'static str {
    match resolution {
        Resolution::Auto | Resolution::OneK => "1K",
        Resolution::TwoK => "2K",
        Resolution::FourK => "4K",
    }
}

fn generation_parameters(
    model: ImageModel,
    resolution: Resolution,
    ratio: &str,
    quality: ImageQuality,
) -> Result<GenerationParameters> {
    let gpt = is_gpt_image_model(model);
    let aspect_ratio = if gpt && resolution != Resolution::Auto && ratio == "auto" {
        "1:1"
    } else {
        ratio
    };
    let image_size = if gpt {
        gpt_size(resolution, aspect_ratio)?
    } else {
        banana_image_size(resolution).into()
    };
    let expected_dimensions = if gpt {
        image_size.split_once('x').and_then(|(w, h)| {
            Some(PixelDimensions {
                width: w.parse().ok()?,
                height: h.parse().ok()?,
            })
        })
    } else {
        image_result::banana_dimensions(model, resolution, aspect_ratio)
    };
    Ok(GenerationParameters {
        model,
        api_model: if gpt {
            gpt_model_id(model)?.into()
        } else {
            banana_model_id(model, resolution)?
        },
        resolution,
        aspect_ratio: aspect_ratio.into(),
        quality: gpt.then_some(quality),
        image_size,
        expected_dimensions,
    })
}

pub async fn check_balance_with_key(key: &str) -> Result<Option<f64>> {
    let client = Client::new();
    let response = client
        .get(format!("{API_BASE}/api/usage/token"))
        .bearer_auth(key)
        .header("x-api-key", key)
        .header("token", key)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        bail!("HTTP {}: {}", status.as_u16(), text);
    }
    let value: Value = serde_json::from_str(&text).context("余额接口返回的不是有效 JSON")?;
    Ok(find_balance(&value, None))
}

pub async fn check_balance() -> Result<Option<f64>> {
    let key = read_api_key()?.ok_or_else(|| anyhow!("请先配置 API Key"))?;
    check_balance_with_key(&key).await
}

fn find_balance(value: &Value, parent: Option<&str>) -> Option<f64> {
    match value {
        Value::Number(number) => {
            let raw = number.as_f64()?;
            let key = parent.unwrap_or("").to_ascii_lowercase();
            if key.contains("usd") || key.contains("dollar") || key == "balance" || key == "amount"
            {
                Some(raw)
            } else if key.contains("token")
                || key.contains("quota")
                || key.contains("remain")
                || key == "total_available"
            {
                Some(raw / 500_000.0)
            } else {
                None
            }
        }
        Value::Object(map) => {
            if map
                .get("unlimited_quota")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return None;
            }
            for key in [
                "balance_usd",
                "balanceUsd",
                "balance",
                "amount",
                "total_available",
                "remaining_quota",
                "quota_remaining",
            ] {
                if let Some(balance) = map.get(key).and_then(|item| find_balance(item, Some(key))) {
                    return Some(balance);
                }
            }
            map.iter()
                .find_map(|(key, item)| find_balance(item, Some(key)))
        }
        Value::Array(items) => items.iter().find_map(|item| find_balance(item, parent)),
        _ => None,
    }
}

pub async fn validate_key(value: &str) -> Result<Option<f64>> {
    check_balance_with_key(value).await
}

pub async fn generate(request: GenerateRequest) -> Result<GenerationOutcome> {
    let prompts = effective_prompts(&request)?;
    let output_count = prompts.len() as u8;
    let prefs = load_preferences()?;
    let model = request.model.unwrap_or(prefs.model);
    let resolution = effective_resolution(model, request.resolution, prefs.resolution);
    let ratio = request
        .aspect_ratio
        .as_deref()
        .unwrap_or(&prefs.aspect_ratio);
    let quality = request.quality.unwrap_or(prefs.quality);
    validate_ratio(model, ratio)?;
    let parameters = generation_parameters(model, resolution, ratio, quality)?;
    let key =
        read_api_key()?.ok_or_else(|| anyhow!("请先打开 Smile AI Image 设置并保存 API Key"))?;
    let client = Client::new();
    let ratio = ratio.to_string();
    let outcomes = run_bounded_ordered(prompts, |index, prompt| {
        let parameters = parameters.clone();
        let client = client.clone();
        let key = key.clone();
        let ratio = ratio.clone();
        let mut output_request = request.clone();
        output_request.count = output_count;
        output_request.prompt = prompt;
        async move {
            let generated = match model {
                ImageModel::NanoBanana2 | ImageModel::NanoBanana21 | ImageModel::NanoBananaPro => {
                    generate_banana(&client, &key, model, resolution, &ratio, &output_request).await
                }
                ImageModel::GptImage2
                | ImageModel::GptImage2All
                | ImageModel::GptImage25Flare
                | ImageModel::GptImage25Sunburst => {
                    generate_gpt(
                        &client,
                        &key,
                        model,
                        resolution,
                        &ratio,
                        quality,
                        &output_request,
                    )
                    .await
                }
            };
            generated.and_then(|(mime_type, data)| {
                let path = save_image(&output_request, index as u8, &mime_type, &data)?;
                let (dimensions, warnings) = image_result::inspect(&path, &parameters);
                Ok(GeneratedImage {
                    path,
                    mime_type,
                    base64: data,
                    dimensions,
                    warnings,
                })
            })
        }
    })
    .await;

    let mut images = Vec::new();
    let mut failures = Vec::new();
    for (index, result) in outcomes {
        match result {
            Ok(image) => images.push((index, image)),
            Err(error) => failures.push(GenerationFailure {
                index: index as u8 + 1,
                message: error.to_string(),
            }),
        }
    }
    images.sort_by_key(|(index, _)| *index);
    Ok(GenerationOutcome {
        requested: output_count,
        parameters,
        images: images.into_iter().map(|(_, image)| image).collect(),
        failures,
    })
}

async fn run_bounded_ordered<T, U, F, Fut>(items: Vec<T>, operation: F) -> Vec<(usize, U)>
where
    F: Fn(usize, T) -> Fut,
    Fut: std::future::Future<Output = U>,
{
    let operation = &operation;
    let mut outcomes = stream::iter(items.into_iter().enumerate())
        .map(|(index, item)| async move { (index, operation(index, item).await) })
        .buffer_unordered(MAX_CONCURRENT_GENERATIONS)
        .collect::<Vec<_>>()
        .await;
    outcomes.sort_by_key(|(index, _)| *index);
    outcomes
}

fn effective_prompts(request: &GenerateRequest) -> Result<Vec<String>> {
    let prompts = if request.variant_prompts.is_empty() {
        if request.prompt.trim().is_empty() {
            bail!("提示词不能为空");
        }
        if request.count == 0 || usize::from(request.count) > MAX_GENERATION_COUNT {
            bail!("生成数量必须在 1 到 {MAX_GENERATION_COUNT} 之间");
        }
        vec![request.prompt.trim().to_string(); request.count as usize]
    } else {
        if request.variant_prompts.len() > MAX_GENERATION_COUNT {
            bail!("逐图提示词数量必须在 1 到 {MAX_GENERATION_COUNT} 之间");
        }
        request
            .variant_prompts
            .iter()
            .enumerate()
            .map(|(index, prompt)| {
                let prompt = prompt.trim();
                if prompt.is_empty() {
                    bail!("第 {} 条逐图提示词不能为空", index + 1);
                }
                Ok(prompt.to_string())
            })
            .collect::<Result<Vec<_>>>()?
    };

    if prompts.len() == 1 {
        return Ok(prompts);
    }

    let total = prompts.len();
    Ok(prompts
        .into_iter()
        .enumerate()
        .map(|(index, prompt)| {
            format!(
                "{prompt}\n\n只输出一张独立、完整的图片。不要生成拼图、网格、双联画、分镜或多画面版式。这是第 {} 个独立方案（共 {total} 个），请使用与其他方案不同的构图。",
                index + 1
            )
        })
        .collect())
}

async fn generate_banana(
    client: &Client,
    key: &str,
    model: ImageModel,
    resolution: Resolution,
    ratio: &str,
    request: &GenerateRequest,
) -> Result<(String, String)> {
    let mut parts = vec![json!({"text": request.prompt})];
    for path in &request.input_images {
        let data =
            fs::read(path).with_context(|| format!("读取输入图片失败: {}", path.display()))?;
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        parts.push(json!({"inlineData": {"mimeType": mime, "data": BASE64.encode(data)}}));
    }
    let mut image_config = json!({"imageSize": banana_image_size(resolution)});
    if ratio != "auto" {
        image_config["aspectRatio"] = Value::String(ratio.into());
    }
    let body = json!({"contents": [{"role": "user", "parts": parts}], "generationConfig": {"responseModalities": ["IMAGE"], "imageConfig": image_config}});
    let model_id = banana_model_id(model, resolution)?;
    let response = client
        .post(format!(
            "{API_BASE}/v1beta/models/{model_id}:generateContent"
        ))
        .bearer_auth(key)
        .header("x-goog-api-key", key)
        .json(&body)
        .send()
        .await?;
    parse_image_response(response).await
}

async fn generate_gpt(
    client: &Client,
    key: &str,
    model: ImageModel,
    resolution: Resolution,
    ratio: &str,
    quality: ImageQuality,
    request: &GenerateRequest,
) -> Result<(String, String)> {
    let size = gpt_size(resolution, ratio)?;
    let quality = quality.as_api_value();
    let model_id = gpt_model_id(model)?;
    let response = if request.input_images.is_empty() {
        client.post(format!("{API_BASE}/v1/images/generations")).bearer_auth(key).json(&json!({
            "model": model_id, "prompt": request.prompt, "n": 1, "size": size, "quality": quality, "output_format": "png"
        })).send().await?
    } else {
        let mut form = multipart::Form::new()
            .text("model", model_id)
            .text("prompt", request.prompt.clone())
            .text("n", "1")
            .text("size", size)
            .text("quality", quality)
            .text("output_format", "png");
        for path in &request.input_images {
            let bytes =
                fs::read(path).with_context(|| format!("读取输入图片失败: {}", path.display()))?;
            let filename = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("input.png")
                .to_string();
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            let part = multipart::Part::bytes(bytes)
                .file_name(filename)
                .mime_str(&mime)?;
            form = form.part("image[]", part);
        }
        client
            .post(format!("{API_BASE}/v1/images/edits"))
            .bearer_auth(key)
            .multipart(form)
            .send()
            .await?
    };
    parse_image_response(response).await
}

fn gpt_model_id(model: ImageModel) -> Result<&'static str> {
    match model {
        ImageModel::GptImage2 => Ok("gpt-image-2"),
        ImageModel::GptImage2All => Ok("gpt-image-2-all"),
        ImageModel::GptImage25Flare => Ok("gpt-image-2.5-flare"),
        ImageModel::GptImage25Sunburst => Ok("gpt-image-2.5-sunburst"),
        ImageModel::NanoBanana2 | ImageModel::NanoBanana21 | ImageModel::NanoBananaPro => {
            bail!("当前模型不是 GPT Image 模型")
        }
    }
}

fn is_gpt_image_model(model: ImageModel) -> bool {
    matches!(
        model,
        ImageModel::GptImage2
            | ImageModel::GptImage2All
            | ImageModel::GptImage25Flare
            | ImageModel::GptImage25Sunburst
    )
}

async fn parse_image_response(response: reqwest::Response) -> Result<(String, String)> {
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(http_error(status, &text));
    }
    let value: Value = serde_json::from_str(&text).context("图片接口返回的不是有效 JSON")?;
    if let Some((mime, data)) = find_image_data(&value) {
        return normalize_image_data(&mime, &data);
    }
    if let Some(url) = find_image_url(&value) {
        let response = Client::new().get(url).send().await?;
        let mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/png")
            .to_string();
        let bytes = response.bytes().await?;
        let data = BASE64.encode(bytes);
        return normalize_image_data(&mime, &data);
    }
    bail!("图片接口返回中没有找到图片数据")
}

fn normalize_image_data(declared_mime: &str, data: &str) -> Result<(String, String)> {
    let bytes = BASE64.decode(data).context("图片 Base64 数据无效")?;
    let mime = detect_image_mime(&bytes).unwrap_or(declared_mime);
    Ok((mime.into(), data.into()))
}

fn detect_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn http_error(status: StatusCode, text: &str) -> anyhow::Error {
    anyhow!("HTTP {}: {}", status.as_u16(), text)
}

fn find_image_data(value: &Value) -> Option<(String, String)> {
    match value {
        Value::Object(map) => {
            if let Some(data) = map.get("b64_json").and_then(Value::as_str) {
                return Some(("image/png".into(), data.into()));
            }
            for key in ["inlineData", "inline_data"] {
                if let Some(inline) = map.get(key).and_then(Value::as_object) {
                    if let Some(data) = inline.get("data").and_then(Value::as_str) {
                        let mime = inline
                            .get("mimeType")
                            .or_else(|| inline.get("mime_type"))
                            .and_then(Value::as_str)
                            .unwrap_or("image/png");
                        return Some((mime.into(), data.into()));
                    }
                }
            }
            map.values().find_map(find_image_data)
        }
        Value::Array(items) => items.iter().find_map(find_image_data),
        _ => None,
    }
}

fn find_image_url(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) => {
            for key in ["url", "image_url", "result_url"] {
                if let Some(url) = map.get(key).and_then(Value::as_str) {
                    return Some(url);
                }
            }
            map.values().find_map(find_image_url)
        }
        Value::Array(items) => items.iter().find_map(find_image_url),
        _ => None,
    }
}

fn save_image(request: &GenerateRequest, index: u8, mime: &str, data: &str) -> Result<PathBuf> {
    let output = request
        .output_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("output/imagegen"));
    fs::create_dir_all(&output)?;
    let extension = match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    };
    let stem = output_stem(request, index);
    let mut path = output.join(format!("{stem}.{extension}"));
    let mut version = 2;
    while path.exists() {
        path = output.join(format!("{stem}-{version}.{extension}"));
        version += 1;
    }
    fs::write(&path, BASE64.decode(data).context("图片 Base64 数据无效")?)?;
    Ok(path.canonicalize().unwrap_or(path))
}

fn output_stem(request: &GenerateRequest, index: u8) -> String {
    let base = sanitize_name(if request.name.trim().is_empty() {
        "生成图片"
    } else {
        request.name.trim()
    });
    if request.count > 1 {
        format!("{base}-{:02}", index + 1)
    } else {
        base
    }
}

fn sanitize_name(value: &str) -> String {
    let value: String = value
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .take(48)
        .collect();
    if value.trim().is_empty() {
        "生成图片".into()
    } else {
        value.trim().into()
    }
}

pub fn path_is_image(path: &Path) -> bool {
    mime_guess::from_path(path)
        .first()
        .is_some_and(|mime| mime.type_() == "image")
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use super::*;

    fn request(prompt: &str, count: u8, variant_prompts: Vec<&str>) -> GenerateRequest {
        GenerateRequest {
            prompt: prompt.into(),
            variant_prompts: variant_prompts.into_iter().map(str::to_string).collect(),
            name: "测试图片".into(),
            model: None,
            resolution: None,
            aspect_ratio: None,
            quality: None,
            count,
            input_images: Vec::new(),
            output_dir: None,
        }
    }

    #[test]
    fn repeated_prompt_gets_single_image_safeguards() {
        let prompts = effective_prompts(&request("红色跑车摄影", 2, vec![])).unwrap();

        assert_eq!(prompts.len(), 2);
        assert!(prompts[0].contains("不要生成拼图"));
        assert!(prompts[0].contains("第 1 个独立方案"));
        assert!(prompts[1].contains("第 2 个独立方案"));
    }

    #[test]
    fn variant_prompts_override_count() {
        let prompts = effective_prompts(&request(
            "红色跑车摄影",
            1,
            vec!["低机位海岸动态追拍", "高机位现代建筑俯拍"],
        ))
        .unwrap();

        assert_eq!(prompts.len(), 2);
        assert!(prompts[0].starts_with("低机位海岸动态追拍"));
        assert!(prompts[1].starts_with("高机位现代建筑俯拍"));
    }

    #[test]
    fn variant_prompts_reject_empty_items() {
        let error =
            effective_prompts(&request("红色跑车摄影", 2, vec!["构图一", " "])).unwrap_err();

        assert!(error.to_string().contains("第 2 条逐图提示词不能为空"));
    }

    #[test]
    fn variant_prompts_accept_twenty_items() {
        let variants = vec!["独立构图"; 20];
        let prompts = effective_prompts(&request("红色跑车摄影", 1, variants)).unwrap();

        assert_eq!(prompts.len(), 20);
    }

    #[test]
    fn variant_prompts_reject_more_than_twenty_items() {
        let variants = vec!["独立构图"; 21];
        let error = effective_prompts(&request("红色跑车摄影", 1, variants)).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("逐图提示词数量必须在 1 到 20 之间")
        );
    }

    #[test]
    fn count_accepts_twenty_and_rejects_twenty_one() {
        assert_eq!(
            effective_prompts(&request("红色跑车摄影", 20, vec![]))
                .unwrap()
                .len(),
            20
        );
        let error = effective_prompts(&request("红色跑车摄影", 21, vec![])).unwrap_err();
        assert!(error.to_string().contains("生成数量必须在 1 到 20 之间"));
    }

    #[tokio::test]
    async fn bounded_queue_runs_six_at_a_time_and_preserves_order() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let outcomes = run_bounded_ordered((0..20).collect(), |_, item| {
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                item
            }
        })
        .await;

        assert_eq!(peak.load(Ordering::SeqCst), MAX_CONCURRENT_GENERATIONS);
        assert_eq!(
            outcomes
                .into_iter()
                .map(|(_, item)| item)
                .collect::<Vec<_>>(),
            (0..20).collect::<Vec<_>>()
        );
    }

    #[test]
    fn multiple_outputs_keep_numbered_file_names() {
        let mut request = request("红色跑车摄影", 1, vec!["构图一", "构图二"]);
        request.count = effective_prompts(&request).unwrap().len() as u8;

        assert_eq!(output_stem(&request, 0), "测试图片-01");
        assert_eq!(output_stem(&request, 1), "测试图片-02");
    }

    #[test]
    fn model_ratios_are_separate() {
        assert!(supported_ratios(ImageModel::NanoBanana2).contains(&"1:8"));
        assert!(!supported_ratios(ImageModel::NanoBananaPro).contains(&"1:8"));
        assert!(supported_ratios(ImageModel::GptImage2).contains(&"1:3"));
        assert!(!supported_ratios(ImageModel::GptImage2).contains(&"1:4"));
    }

    #[test]
    fn gpt_sizes_follow_limits() {
        for ratio in supported_ratios(ImageModel::GptImage2)
            .iter()
            .filter(|ratio| **ratio != "auto")
        {
            let size = gpt_size(Resolution::FourK, ratio).unwrap();
            let (w, h) = size.split_once('x').unwrap();
            let w: u32 = w.parse().unwrap();
            let h: u32 = h.parse().unwrap();
            assert_eq!(w % 16, 0);
            assert_eq!(h % 16, 0);
            assert!(w <= 3840 && h <= 3840);
            assert!(u64::from(w) * u64::from(h) <= 8_294_400);
        }
    }

    #[test]
    fn key_mask_never_echoes_full_secret() {
        assert_eq!(mask_key("sk-1234567890"), "sk-1••••••7890");
    }

    #[test]
    fn balance_parser_supports_smile_quota_response() {
        let value = json!({
            "data": {
                "total_available": 13_481_179,
                "total_granted": 16_451_392,
                "total_used": 2_970_213,
                "unlimited_quota": false
            }
        });
        assert_eq!(find_balance(&value, None), Some(26.962358));
    }

    #[test]
    fn unlimited_quota_has_no_numeric_balance() {
        let value = json!({
            "data": {
                "total_available": 13_481_179,
                "unlimited_quota": true
            }
        });
        assert_eq!(find_balance(&value, None), None);
    }

    #[test]
    fn image_magic_overrides_incorrect_declared_mime() {
        let jpeg = BASE64.encode(b"\xff\xd8\xff\xe0jpeg");
        let (mime, data) = normalize_image_data("image/png", &jpeg).unwrap();
        assert_eq!(mime, "image/jpeg");
        assert_eq!(data, jpeg);
    }

    #[test]
    fn old_preferences_default_to_auto_quality() {
        let preferences: Preferences = serde_json::from_value(json!({
            "model": "gpt-image2",
            "resolution": "TWOK",
            "aspectRatio": "4:3",
            "theme": "system",
            "language": "system"
        }))
        .unwrap();

        assert_eq!(preferences.quality, ImageQuality::Auto);
    }

    #[test]
    fn image_quality_uses_official_api_values() {
        assert_eq!(ImageQuality::Auto.as_api_value(), "auto");
        assert_eq!(ImageQuality::Low.as_api_value(), "low");
        assert_eq!(ImageQuality::Medium.as_api_value(), "medium");
        assert_eq!(ImageQuality::High.as_api_value(), "high");
    }

    #[test]
    fn gpt_image_all_reuses_gpt_capabilities_and_model_id() {
        assert_eq!(gpt_model_id(ImageModel::GptImage2).unwrap(), "gpt-image-2");
        assert_eq!(
            gpt_model_id(ImageModel::GptImage2All).unwrap(),
            "gpt-image-2-all"
        );
        assert_eq!(
            supported_ratios(ImageModel::GptImage2All),
            supported_ratios(ImageModel::GptImage2)
        );
        assert!(validate_ratio(ImageModel::GptImage2All, "7:5").is_ok());
        assert_eq!(gpt_size(Resolution::FourK, "2:3").unwrap(), "2352x3520");
    }

    #[test]
    fn gpt_image_25_models_use_official_ids_and_capabilities() {
        assert_eq!(
            gpt_model_id(ImageModel::GptImage25Flare).unwrap(),
            "gpt-image-2.5-flare"
        );
        assert_eq!(
            gpt_model_id(ImageModel::GptImage25Sunburst).unwrap(),
            "gpt-image-2.5-sunburst"
        );
        assert_eq!(
            supported_ratios(ImageModel::GptImage25Flare),
            supported_ratios(ImageModel::GptImage2)
        );
        assert!(validate_ratio(ImageModel::GptImage25Sunburst, "7:5").is_ok());
        assert_eq!(
            serde_json::to_value(ImageModel::GptImage25Flare).unwrap(),
            json!("gpt-image-2.5-flare")
        );
    }
}
