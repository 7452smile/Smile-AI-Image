# 贡献指南

欢迎提交 Bug、文档修正、模型适配和兼容性改进。一般问题使用 GitHub Issues，安全漏洞请按 [SECURITY.md](SECURITY.md) 私密报告。

## 开发流程

1. Fork 仓库并创建功能分支，保持每个 PR 聚焦一个问题。
2. 按 [README](README.md) 安装开发依赖。
3. 修改模型时同步工具 schema、设置页、skill 和能力说明；修改插件信息时同步静态清单和 `settings/src-tauri/src/lib.rs` 中的动态清单。
4. 运行与改动相关的检查，并在 PR 中说明验证环境及未验证的平台。

仅检查类型和格式，不生成安装包：

```bash
cd settings
npx --no-install tsc --noEmit
cd ..
cargo fmt --all -- --check
```

业务逻辑测试会编译测试程序；仅在允许构建时执行：

```bash
cargo test --workspace --locked
```

涉及 Tauri 或安装逻辑的变更需要在对应平台验证。实时生成可能产生费用，不应作为默认测试，也不要在 CI 中加入个人 API Key。

## 提交前检查

- 不提交 `.env`、API Key、认证文件、Codex 本机配置、私钥、签名证书或账户数据。
- 不提交 `target`、`node_modules`、安装包、生成图片、日志或 macOS 的 `._*` 元数据文件。
- 截图和日志先移除密钥、余额、私人路径、参考图片、提示词和可访问的临时图片地址。
- 用 `git diff --cached` 确认实际提交内容；可用 [Gitleaks](https://github.com/gitleaks/gitleaks) 在推送前扫描暂存内容或提交历史。
- Git 提交身份可使用 GitHub Settings → Emails 提供的 noreply 邮箱，避免公开私人邮箱。
- 保留已有许可证和版权声明；新增依赖应与项目 MIT 发布方式兼容。

提交 PR 即表示你有权以本项目 MIT 许可证贡献相关代码。无需将个人联系方式写入源码。
