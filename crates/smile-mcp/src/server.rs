use std::{
    io::{self, BufRead, Write},
    process::Command,
};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use smile_core::{GenerateRequest, generate};

const PROTOCOL_VERSION: &str = "2024-11-05";

#[tokio::main]
pub async fn run() -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    loop {
        let Some(message) = read_message(&mut reader)? else {
            break;
        };
        if let Some(response) = handle_message(message).await {
            write_message(&response)?;
        }
    }
    Ok(())
}

fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>> {
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let message = line.trim_end_matches(['\r', '\n']);
        if !message.trim().is_empty() {
            return Ok(Some(serde_json::from_str(message)?));
        }
    }
}

fn write_message(value: &Value) -> Result<()> {
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    write_json_line(&mut writer, value)
}

fn write_json_line(writer: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

async fn handle_message(message: Value) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "smile-ai-image", "version": env!("CARGO_PKG_VERSION")}
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_specs()})),
        "tools/call" => {
            let params = message.get("params").cloned().unwrap_or_default();
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            call_tool(name, arguments).await
        }
        _ => Err(anyhow!("不支持的方法: {method}")),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": error.to_string()}})
        }
    })
}

fn tool_specs() -> Vec<Value> {
    vec![
        json!({"name": "get_settings", "description": "读取当前默认模型、分辨率、比例、GPT Image 2 / 2.5 质量和 API Key 掩码状态。不会返回完整 Key。", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}}),
        json!({"name": "get_model_capabilities", "description": "读取七款图片模型支持的分辨率、比例和质量选项。生成前用它校验用户要求。", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}}),
        json!({"name": "check_balance", "description": "验证已保存的 API Key 并读取 Smile AI 账户美元余额。", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}}),
        json!({"name": "open_settings", "description": "打开或聚焦 Smile AI Image 独立设置应用。", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}}),
        json!({
            "name": "generate_image",
            "description": "使用 Smile AI 中转站生成或编辑图片，保存到本地并直接返回图片供 Codex 预览。一次用户请求只能调用一次；画面不满意时不得自动再次调用。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "prompt": {"type": "string", "description": "完整图片生成或编辑提示词"},
                    "variantPrompts": {"type": "array", "minItems": 1, "maxItems": 20, "items": {"type": "string", "minLength": 1}, "description": "可选逐图提示词。提供时覆盖 count，每项只描述一张独立图片，并各生成一张。多张图片有不同构图或内容时必须使用。插件最多并发处理 6 张。"},
                    "name": {"type": "string", "description": "从提示词提取的简短中文文件名，不含扩展名"},
                    "model": {"type": "string", "enum": ["nano-banana2", "nano-banana-2.1", "nano-banana-pro", "gpt-image2", "gpt-image2-all", "gpt-image-2.5-flare", "gpt-image-2.5-sunburst"]},
                    "resolution": {"type": "string", "enum": ["AUTO", "ONEK", "TWOK", "FOURK"], "description": "省略时继承 get_settings 的默认分辨率；如果设置为 FOURK，必须传 FOURK。对 Banana 模型不要用 AUTO 替代省略值，AUTO 会请求 1K。只有用户明确要求自动时才传 AUTO。"},
                    "aspectRatio": {"type": "string", "description": "例如 auto、1:1、16:9；GPT Image 2 / 2.5 支持范围内的自定义宽:高"},
                    "quality": {"type": "string", "enum": ["AUTO", "LOW", "MEDIUM", "HIGH"], "description": "用于 GPT Image 2、GPT Image 2 All、GPT Image 2.5 Flare 和 GPT Image 2.5 Sunburst；未指定时使用设置应用中的默认质量"},
                    "count": {"type": "integer", "minimum": 1, "maximum": 20, "default": 1, "description": "输出数量。插件最多并发处理 6 张，其余自动排队。"},
                    "inputImages": {"type": "array", "items": {"type": "string"}},
                    "outputDir": {"type": "string", "description": "可选输出目录；默认 output/imagegen"}
                },
                "required": ["prompt", "name"],
                "additionalProperties": false
            }
        }),
    ]
}

async fn call_tool(name: &str, arguments: Value) -> Result<Value> {
    match name {
        "get_settings" => text_result(serde_json::to_value(smile_core::settings_snapshot()?)?),
        "get_model_capabilities" => text_result(json!({
            "nano-banana2": {"resolutions": ["1K", "2K", "4K"], "ratios": smile_core::supported_ratios(smile_core::ImageModel::NanoBanana2)},
            "nano-banana-2.1": {"resolutions": ["1K", "2K", "4K"], "ratios": smile_core::supported_ratios(smile_core::ImageModel::NanoBanana21), "relayModels": ["gemini-nano-banana-2.1-1k", "gemini-nano-banana-2.1-2k", "gemini-nano-banana-2.1-4k"]},
            "nano-banana-pro": {"resolutions": ["1K", "2K", "4K"], "ratios": smile_core::supported_ratios(smile_core::ImageModel::NanoBananaPro)},
            "gpt-image2": {"resolutions": ["自动", "1K", "2K", "4K"], "ratios": smile_core::supported_ratios(smile_core::ImageModel::GptImage2), "customRatio": "1:3..3:1", "qualities": ["AUTO", "LOW", "MEDIUM", "HIGH"]},
            "gpt-image2-all": {"resolutions": ["自动", "1K", "2K", "4K"], "ratios": smile_core::supported_ratios(smile_core::ImageModel::GptImage2All), "customRatio": "1:3..3:1", "qualities": ["AUTO", "LOW", "MEDIUM", "HIGH"]},
            "gpt-image-2.5-flare": {"resolutions": ["自动", "1K", "2K", "4K"], "ratios": smile_core::supported_ratios(smile_core::ImageModel::GptImage25Flare), "customRatio": "1:3..3:1", "qualities": ["AUTO", "LOW", "MEDIUM", "HIGH"]},
            "gpt-image-2.5-sunburst": {"resolutions": ["自动", "1K", "2K", "4K"], "ratios": smile_core::supported_ratios(smile_core::ImageModel::GptImage25Sunburst), "customRatio": "1:3..3:1", "qualities": ["AUTO", "LOW", "MEDIUM", "HIGH"]}
        })),
        "check_balance" => {
            let balance = smile_core::check_balance().await?;
            text_result(
                json!({"authenticated": true, "balanceUsd": balance, "lowBalance": balance.is_some_and(|value| value < 1.0)}),
            )
        }
        "open_settings" => {
            open_settings()?;
            text_result(json!({"ok": true, "message": "已打开 Smile AI Image 设置"}))
        }
        "generate_image" => {
            let request: GenerateRequest = serde_json::from_value(arguments)?;
            let outcome = generate(request).await?;
            let has_failures = !outcome.failures.is_empty();
            let all_failed = outcome.images.is_empty() && has_failures;
            let mut content = Vec::new();
            let paths: Vec<String> = outcome
                .images
                .iter()
                .map(|image| image.path.display().to_string())
                .collect();
            content.push(json!({"type": "text", "text": serde_json::to_string_pretty(&json!({
                "ok": !has_failures,
                "partial": has_failures && !all_failed,
                "requested": outcome.requested,
                "completed": outcome.images.len(),
                "failed": outcome.failures.len(),
                "parameters": outcome.parameters.clone(),
                "paths": paths,
                "failures": outcome.failures,
                "imageDimensions": outcome.images.iter().map(|image| json!({"path": image.path, "width": image.dimensions.map(|d| d.width), "height": image.dimensions.map(|d| d.height), "warnings": image.warnings})).collect::<Vec<_>>(),
            }))?}));
            for image in outcome.images {
                content.push(
                    json!({"type": "image", "data": image.base64, "mimeType": image.mime_type}),
                );
            }
            Ok(json!({"content": content, "isError": all_failed}))
        }
        _ => Err(anyhow!("未知工具: {name}")),
    }
}

fn text_result(value: Value) -> Result<Value> {
    Ok(json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(&value)?}]}))
}

fn open_settings() -> Result<()> {
    #[cfg(target_os = "macos")]
    Command::new("open")
        .args(["-a", "Smile AI Image"])
        .spawn()?;
    #[cfg(target_os = "windows")]
    Command::new("cmd")
        .args(["/C", "start", "", "Smile AI Image"])
        .spawn()?;
    #[cfg(target_os = "linux")]
    return Err(anyhow!("Smile AI Image 首版仅支持 macOS 和 Windows"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn reads_newline_delimited_json_rpc() {
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n";
        let message = read_message(&mut Cursor::new(input)).unwrap().unwrap();

        assert_eq!(message["method"], "initialize");
        assert_eq!(message["id"], 1);
    }

    #[test]
    fn writes_newline_delimited_json_rpc() {
        let mut output = Vec::new();
        let response = json!({"jsonrpc": "2.0", "id": 1, "result": {}});

        write_json_line(&mut output, &response).unwrap();

        assert!(output.ends_with(b"\n"));
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&output).unwrap(),
            response
        );
    }

    #[test]
    fn generate_schema_supports_independent_variant_prompts() {
        let generate = tool_specs()
            .into_iter()
            .find(|tool| tool["name"] == "generate_image")
            .unwrap();
        let variants = &generate["inputSchema"]["properties"]["variantPrompts"];

        assert_eq!(variants["type"], "array");
        assert_eq!(variants["maxItems"], 20);
        assert_eq!(
            generate["inputSchema"]["properties"]["count"]["maximum"],
            20
        );
    }

    #[test]
    fn generate_schema_exposes_gpt_image_25_models() {
        let generate = tool_specs()
            .into_iter()
            .find(|tool| tool["name"] == "generate_image")
            .unwrap();
        let models = generate["inputSchema"]["properties"]["model"]["enum"]
            .as_array()
            .unwrap();

        assert!(models
            .iter()
            .any(|model| model.as_str() == Some("gpt-image-2.5-flare")));
        assert!(models
            .iter()
            .any(|model| model.as_str() == Some("gpt-image-2.5-sunburst")));
        assert!(models
            .iter()
            .any(|model| model.as_str() == Some("nano-banana-2.1")));
    }
}
