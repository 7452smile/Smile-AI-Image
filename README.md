# Smile AI Image

在 Codex 对话中生成和编辑图片，使用独立的桌面设置应用管理模型、分辨率、比例和 API Key。支持 macOS 与 Windows，源码基于 [MIT 许可证](LICENSE) 开放。

[Smile AI 主站](https://api.smile-ai-studio.com/) · [GitHub 源码](https://github.com/7452smile/Smile-AI-Image) · [问题反馈](https://github.com/7452smile/Smile-AI-Image/issues) · [安装包发布页](https://github.com/7452smile/Smile-AI-Image/releases)

## 开始使用

1. 在发布页选择与你的系统、CPU 架构匹配的安装包。如果尚无对应 Release，需按下方说明自行构建；GitHub 的 Source code ZIP 是源码，不是安装包。
2. macOS 将应用拖入“应用程序”后打开；Windows 运行安装程序后打开 Smile AI Image。
3. 前往 [Smile AI 主站](https://api.smile-ai-studio.com/) 创建或管理 API Key，在设置应用中“测试并保存”。
4. 选择默认模型、分辨率和比例，等待 Codex 接入状态显示配置完成，然后重启 Codex 并开启新对话。
5. 在 Codex 中描述需求，例如“用 Smile AI Image 生成一张 4K、1:1 的产品摄影图片”。编辑图片时附上参考图片并说明修改内容。

开源的是插件、MCP 服务和设置应用。图片生成通过 Smile AI 中转服务完成，需要有效的 API Key 和可用额度；服务计费及上游模型条款独立于 MIT 许可证。欢迎到主站体验和管理模型服务，也欢迎为项目 Star 或提交改进。

本项目由 Smile AI Studio 维护，与 OpenAI、Google 无官方隶属关系。

## 支持的模型

| 设置中的模型 | Smile AI 中转模型 ID |
| --- | --- |
| Nano Banana 2 | `gemini-3.1-flash-image-preview-{1k,2k,4k}` |
| Nano Banana 2.1 | `gemini-nano-banana-2.1-{1k,2k,4k}` |
| Nano Banana Pro | `gemini-3-pro-image-preview-{1k,2k,4k}` |
| GPT Image 2 | `gpt-image-2` |
| GPT Image 2 All | `gpt-image-2-all` |
| GPT Image 2.5 Flare | `gpt-image-2.5-flare` |
| GPT Image 2.5 Sunburst | `gpt-image-2.5-sunburst` |

花括号表示按分辨率选择其中一个后缀。上述 ID 是本项目发送给中转站的标识，实际可用性、计费及能力以中转服务为准。

- 支持生成、参考图编辑、逐图提示词和本地原图保存。
- 单次最多 20 张，最多 6 个请求并发，分别生成独立图片。
- GPT Image 系列使用 Images API 格式：生成通过 JSON，编辑通过 multipart；支持质量与自定义比例。
- 保存后读取 PNG、JPEG、WebP 原图尺寸，并在 MCP 结果中返回请求模型、预期像素、实际像素和尺寸警告。

模型能力见 [模型说明](skills/smile-ai-image/references/model-capabilities.md)，GPT 请求格式见 [算法与请求逻辑](GPT-Image-2-算法与请求逻辑.md)。

## 关于 4K 和 Codex 更新

`-4k` 后缀代表请求档位，不能单独证明返回图像达到该尺寸。Nano Banana 2 / 2.1 的 1:1 请求预期为：1K `1024×1024`、2K `2048×2048`、4K `4096×4096`。GPT Image 的实际请求尺寸受像素总量和比例限制，使用另一套换算规则。

代码保留已保存的默认分辨率，并检查返回原图。如果请求 4K 却收到 `1024×1024`，会保留结果并提示尺寸不符，不会自动重试产生额外费用。

为减少 Codex 更新导致的失效，应用支持原生插件接入和 STDIO MCP 后备接入。后备模式把 MCP 启动命令指向 Smile AI Image 自身，尊重 `CODEX_HOME` 并安装用户 skill，避免依赖 Codex 内部程序路径。配置变更前会备份原文件；未来公开接口如发生不兼容变化，仍需适配。

## 隐私与数据流

- API Key 保存在本机系统凭据库，不写入仓库或普通设置文件。工具仅返回掩码状态。
- 生成或编辑时，提示词、模型参数和所选参考图片通过 HTTPS 发往 Smile AI 中转服务，由服务路由到上游。认证和余额查询也需要访问主站。
- 原图保存在本地；默认目录为当前工作目录下的 `output/imagegen`，也可通过工具指定。图片和保存路径会返回 Codex 用于预览。
- 设置应用会从主站查询版本信息；主站和 GitHub 链接由用户点击后打开。
- 本仓库不包含账户数据、密钥、签名证书、生成图片或本机 Codex 配置。提交反馈前请先脱敏，详见 [安全说明](SECURITY.md)。

## 源码与构建

两平台共用 TypeScript 前端和 Rust 业务逻辑，但安装包需要分别构建：macOS 使用 DMG，Windows 使用 NSIS EXE；Apple Silicon、Intel 和 Windows x64 需要匹配各自架构。直接下载源码不会自动安装插件。

需要 Rust stable、Node.js 22.12+ 和 npm。macOS 另需 Xcode Command Line Tools；Windows 另需 Visual Studio 2022 Build Tools 的“Desktop development with C++”、MSVC Rust target 和 WebView2。平台依赖参见 [Tauri 官方要求](https://v2.tauri.app/start/prerequisites/)。

```bash
git clone https://github.com/7452smile/Smile-AI-Image.git
cd Smile-AI-Image
npm ci --prefix settings
```

macOS 构建（默认当前主机架构）：

```bash
cd settings
npm run tauri -- build --bundles dmg
```

Windows x64 构建（在 Windows 项目根目录执行）：

```bat
scripts\build-windows-x64.cmd
```

macOS 产物在 `target/release/bundle/dmg/`；Windows 产物在 `target/x86_64-pc-windows-msvc/release/bundle/nsis/`。正式发行的签名和公证需发布者自行配置，仓库不包含凭据。

| 目录 | 用途 |
| --- | --- |
| `crates/smile-core` | 模型请求、设置、凭据和图片处理 |
| `crates/smile-mcp` | STDIO MCP 服务及工具定义 |
| `settings` | Tauri 设置应用和前端 |
| `skills` | Codex 使用指引与模型说明 |
| `.codex-plugin`、`plugin.json`、`.agents/plugins` | 插件清单及市场入口 |
| `scripts` | 开发、构建和验证脚本 |

根目录的 MCP 清单是源码模板，引用构建后的 `bin/smile-mcp`；仓库不附带该二进制。安装后的设置应用会生成指向自身 `--mcp` 模式的实际清单。不要把源码仓库直接当作含二进制的即装插件。

详细步骤见 [源码包说明](源码包说明.md)。提交改进前请阅读 [贡献指南](CONTRIBUTING.md)。本项目代码采用 MIT；第三方依赖保留各自许可证，品牌和商标权利不因代码开源而改变。
