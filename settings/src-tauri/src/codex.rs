use std::{
    ffi::OsStr,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, value};

use super::{MARKETPLACE_NAME, PLUGIN_NAME, PluginInstallStatus, copy_directory};

const MCP_NAME: &str = "smile_ai_image";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) fn install(root: &Path, executable: &Path) -> Result<PluginInstallStatus> {
    let home = user_home_dir()?;
    let codex_home = resolve_codex_home(std::env::var_os("CODEX_HOME").as_deref(), &home)?;
    // Once the independent connection is configured, keep using it after Codex updates.
    // This also avoids trying to reinstall a native plugin on every settings launch.
    let fallback_exists = has_mcp_fallback(&codex_home)?;
    let apps = desktop_locations(&home);
    let desktop_installed = apps.iter().any(|path| path.is_dir());
    let cli = (!fallback_exists)
        .then(|| codex_executable(&apps))
        .flatten();
    install_at(
        root,
        executable,
        &codex_home,
        &home.join(".agents/skills").join(PLUGIN_NAME),
        desktop_installed || cli.is_some(),
        || match cli {
            Some(cli) => install_native_plugin(&cli, root, &codex_home),
            None => bail!("未找到可用的 Codex CLI"),
        },
    )
}

fn install_at(
    root: &Path,
    executable: &Path,
    codex_home: &Path,
    skill_dir: &Path,
    codex_installed: bool,
    native_install: impl FnOnce() -> Result<()>,
) -> Result<PluginInstallStatus> {
    if !has_mcp_fallback(codex_home)? && native_install().is_ok() {
        return Ok(PluginInstallStatus {
            codex_installed: true,
            plugin_installed: true,
            integration_mode: "plugin",
            message: "Codex 插件已安装；请开启新对话".into(),
        });
    }

    install_mcp_fallback(root, executable, codex_home, skill_dir)?;
    Ok(PluginInstallStatus {
        codex_installed,
        plugin_installed: true,
        integration_mode: "mcp",
        message: "Codex 接入已配置；请重启 Codex 并开启新对话".into(),
    })
}

fn user_home_dir() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let names = ["USERPROFILE", "HOME"];
    #[cfg(not(target_os = "windows"))]
    let names = ["HOME", "USERPROFILE"];
    names
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|s| !s.is_empty()))
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("无法定位用户目录"))
}

fn resolve_codex_home(custom: Option<&OsStr>, home: &Path) -> Result<PathBuf> {
    let path = custom
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    if !path.is_absolute() {
        bail!("CODEX_HOME 必须是绝对路径");
    }
    Ok(path)
}

fn read_config(codex_home: &Path) -> Result<String> {
    match fs::read_to_string(codex_home.join("config.toml")) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).context("读取 Codex 配置失败"),
    }
}

fn has_mcp_fallback(codex_home: &Path) -> Result<bool> {
    let text = read_config(codex_home)?;
    let document = text
        .parse::<DocumentMut>()
        .context("Codex 配置格式无效，请先修复 config.toml")?;
    Ok(document
        .get("mcp_servers")
        .and_then(|servers| servers.get(MCP_NAME))
        .is_some())
}

fn table<'a>(parent: &'a mut dyn TableLike, key: &str) -> Result<&'a mut dyn TableLike> {
    parent
        .entry(key)
        .or_insert(Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or_else(|| anyhow::anyhow!("Codex 配置中的 {key} 必须是表格"))
}

fn install_mcp_fallback(
    root: &Path,
    executable: &Path,
    codex_home: &Path,
    skill_dir: &Path,
) -> Result<()> {
    let original = read_config(codex_home)?;
    let mut document = original
        .parse::<DocumentMut>()
        .context("Codex 配置格式无效，请先修复 config.toml")?;
    let server = table(table(document.as_table_mut(), "mcp_servers")?, MCP_NAME)?;
    server.insert("command", value(executable.to_string_lossy().to_string()));
    let mut args = Array::new();
    args.push("--mcp");
    server.insert("args", value(args));
    // Preserve tool policies, environment and other servers while updating our launcher.
    server.remove("url");
    server.insert("enabled", value(true));
    if !server.contains_key("tool_timeout_sec") {
        server.insert("tool_timeout_sec", value(600));
    }
    let plugin_id = format!("{PLUGIN_NAME}@{MARKETPLACE_NAME}");
    let native_plugin = table(table(document.as_table_mut(), "plugins")?, &plugin_id)?;
    // The native bundle must not expose a second copy of these tools and skills.
    native_plugin.insert("enabled", value(false));

    let skill_source = root
        .join("plugins")
        .join(PLUGIN_NAME)
        .join("skills")
        .join(PLUGIN_NAME);
    copy_directory(&skill_source, skill_dir).context("安装 Smile AI Image 对话指引失败")?;
    write_config(codex_home, &original, &document.to_string())
}

fn write_config(codex_home: &Path, original: &str, updated: &str) -> Result<()> {
    if original == updated {
        return Ok(());
    }
    fs::create_dir_all(codex_home)?;
    let config_path = codex_home.join("config.toml");
    let mut temporary = tempfile::NamedTempFile::new_in(codex_home)?;
    if config_path.is_file() {
        temporary
            .as_file()
            .set_permissions(fs::metadata(&config_path)?.permissions())?;
    }
    temporary.write_all(updated.as_bytes())?;
    temporary.as_file().sync_all()?;
    if read_config(codex_home)? != original {
        bail!("Codex 配置已由其他程序修改，请点击重试");
    }
    if config_path.is_file() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        fs::copy(
            &config_path,
            codex_home.join(format!("config.toml.smile-backup-{stamp}")),
        )?;
    }
    temporary
        .persist(&config_path)
        .context("保存 Codex 配置失败")?;
    Ok(())
}

fn install_native_plugin(cli: &Path, root: &Path, codex_home: &Path) -> Result<()> {
    let text = run_codex(
        cli,
        &["plugin", "marketplace", "list", "--json"],
        codex_home,
    )?;
    let response: serde_json::Value = serde_json::from_str(&text)?;
    let items = response
        .get("marketplaces")
        .unwrap_or(&response)
        .as_array()
        .context("Codex marketplace 返回格式不受支持")?;
    let registered = items
        .iter()
        .any(|item| item.get("name").and_then(|v| v.as_str()) == Some(MARKETPLACE_NAME));
    if !registered {
        run_codex(
            cli,
            &[
                "plugin",
                "marketplace",
                "add",
                root.to_string_lossy().as_ref(),
                "--json",
            ],
            codex_home,
        )?;
    }
    run_codex(
        cli,
        &[
            "plugin",
            "add",
            &format!("{PLUGIN_NAME}@{MARKETPLACE_NAME}"),
            "--json",
        ],
        codex_home,
    )?;
    Ok(())
}

fn command(program: impl AsRef<OsStr>) -> Command {
    let command = Command::new(program);
    #[cfg(target_os = "windows")]
    let command = {
        use std::os::windows::process::CommandExt;
        let mut command = command;
        command.creation_flags(0x0800_0000);
        command
    };
    command
}

fn run_codex(executable: &Path, args: &[&str], codex_home: &Path) -> Result<String> {
    #[cfg(target_os = "windows")]
    let mut process = if executable
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("cmd") || s.eq_ignore_ascii_case("bat"))
    {
        let mut process = command("cmd.exe");
        process.args(["/D", "/C"]).arg(executable);
        process
    } else {
        command(executable)
    };
    #[cfg(not(target_os = "windows"))]
    let mut process = command(executable);
    process.args(args).env("CODEX_HOME", codex_home);
    output_with_timeout(process, COMMAND_TIMEOUT)
}

fn output_with_timeout(mut command: Command, timeout: Duration) -> Result<String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().context("无法读取 Codex 输出")?;
    // Read concurrently so a full pipe cannot make a CLI command hang.
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(256 * 1024)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("Codex 插件命令不受支持或执行失败");
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            let bytes = receiver
                .recv_timeout(remaining)
                .context("读取 Codex 输出超时")??;
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            // A wrapper's child might still own the pipe; do not wait on the reader here.
            bail!("Codex 插件命令超时");
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn desktop_locations(home: &Path) -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut locations: Vec<PathBuf> =
            [PathBuf::from("/Applications"), home.join("Applications")]
                .iter()
                .flat_map(|dir| [dir.join("Codex.app"), dir.join("ChatGPT.app")])
                .collect();
        let mut query = command("/usr/bin/mdfind");
        query.arg("kMDItemCFBundleIdentifier == 'com.openai.chat' || kMDItemCFBundleIdentifier == 'com.openai.codex'");
        if let Ok(output) = output_with_timeout(query, Duration::from_secs(2)) {
            locations.extend(
                output
                    .lines()
                    .map(PathBuf::from)
                    .filter(|p| p.extension() == Some(OsStr::new("app"))),
            );
        }
        locations
    }
    #[cfg(target_os = "windows")]
    {
        let _ = home;
        let mut locations = Vec::new();
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let programs = PathBuf::from(local).join("Programs");
            locations.extend([programs.join("Codex"), programs.join("ChatGPT")]);
        }
        let mut query = command("powershell.exe");
        query.args(["-NoProfile", "-NonInteractive", "-Command",
            "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); Get-AppxPackage | Where-Object { $_.Name -in @('OpenAI.Codex','OpenAI.ChatGPT') } | ForEach-Object { $_.InstallLocation }"]);
        if let Ok(output) = output_with_timeout(query, Duration::from_secs(3)) {
            locations.extend(
                output
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from),
            );
        }
        locations
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = home;
        Vec::new()
    }
}

fn codex_executable(apps: &[PathBuf]) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let names = ["codex.exe", "codex.cmd", "codex.bat", "codex"];
    #[cfg(not(target_os = "windows"))]
    let names = ["codex", "codex-cli"];
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    #[cfg(target_os = "windows")]
    if let Some(app_data) = std::env::var_os("APPDATA") {
        directories.push(PathBuf::from(app_data).join("npm"));
    }
    #[cfg(target_os = "macos")]
    {
        directories.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);
    }
    for directory in directories {
        for name in names {
            let candidate = directory.join(name);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    apps.iter().find_map(|app| {
        #[cfg(target_os = "macos")]
        let resources = app.join("Contents/Resources");
        #[cfg(not(target_os = "macos"))]
        let resources = app.clone();
        find_bundled_cli(&resources, 6)
    })
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_bundled_cli(directory: &Path, depth: usize) -> Option<PathBuf> {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in &entries {
        if matches!(entry.file_name().to_str(), Some("codex" | "codex.exe"))
            && is_executable(&entry.path())
        {
            return Some(entry.path());
        }
    }
    if depth == 0 {
        return None;
    }
    entries
        .iter()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .find_map(|entry| find_bundled_cli(&entry.path(), depth - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(root: &Path) {
        super::super::write_plugin_bundle(
            root,
            Path::new("/Applications/Smile AI Image.app/Contents/MacOS/smile-ai-image-settings"),
        )
        .unwrap();
    }

    #[test]
    fn missing_codex_still_configures_working_mcp_and_skill_without_touching_cache() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("marketplace");
        let codex_home = temp.path().join("custom-codex-home");
        let skill_dir = temp.path().join(".agents/skills/smile-ai-image");
        bundle(&root);
        fs::create_dir_all(&codex_home).unwrap();
        let original = "# user settings\nmodel = 'my-model'\n\n[mcp_servers.other]\ncommand = 'other-tool'\n\n[plugins.\"smile-ai-image@smile-ai\"]\nenabled = true\n\n[plugins.\"other@market\"]\nenabled = true\n";
        fs::write(codex_home.join("config.toml"), original).unwrap();
        let executable = temp.path().join("Smile AI Image.exe");
        let status = install_at(&root, &executable, &codex_home, &skill_dir, false, || {
            bail!("no Codex")
        })
        .unwrap();
        assert!(status.plugin_installed);
        assert!(!status.codex_installed);
        assert_eq!(status.integration_mode, "mcp");
        let text = read_config(&codex_home).unwrap();
        let config = text.parse::<DocumentMut>().unwrap();
        assert_eq!(config["model"].as_str(), Some("my-model"));
        assert!(text.contains("# user settings"));
        assert_eq!(
            config["mcp_servers"]["other"]["command"].as_str(),
            Some("other-tool")
        );
        assert_eq!(
            config["mcp_servers"][MCP_NAME]["command"].as_str(),
            executable.to_str()
        );
        assert_eq!(
            config["mcp_servers"][MCP_NAME]["args"]
                .as_array()
                .unwrap()
                .get(0)
                .unwrap()
                .as_str(),
            Some("--mcp")
        );
        assert_eq!(
            config["plugins"]["smile-ai-image@smile-ai"]["enabled"].as_bool(),
            Some(false)
        );
        assert_eq!(
            config["plugins"]["other@market"]["enabled"].as_bool(),
            Some(true)
        );
        assert!(skill_dir.join("SKILL.md").is_file());
        assert!(skill_dir.join("references/model-capabilities.md").is_file());
        assert!(!codex_home.join("plugins/cache").exists());
        let backup = fs::read_dir(&codex_home)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config.toml.smile-backup-")
            })
            .unwrap();
        assert_eq!(fs::read_to_string(backup.path()).unwrap(), original);
    }

    #[test]
    fn changed_plugin_command_falls_back_and_future_launches_keep_that_connection() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("marketplace");
        let codex_home = temp.path().join("codex");
        let skill = temp.path().join("skill");
        bundle(&root);
        let status = install_at(
            &root,
            Path::new("first.exe"),
            &codex_home,
            &skill,
            true,
            || bail!("unknown subcommand plugin"),
        )
        .unwrap();
        assert_eq!(status.integration_mode, "mcp");
        let status = install_at(
            &root,
            Path::new("updated.exe"),
            &codex_home,
            &skill,
            true,
            || panic!("must not depend on Codex CLI after fallback"),
        )
        .unwrap();
        assert_eq!(status.integration_mode, "mcp");
        let config = read_config(&codex_home)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(
            config["mcp_servers"][MCP_NAME]["command"].as_str(),
            Some("updated.exe")
        );
        let files_before = fs::read_dir(&codex_home).unwrap().count();
        install_at(
            &root,
            Path::new("updated.exe"),
            &codex_home,
            &skill,
            true,
            || unreachable!(),
        )
        .unwrap();
        assert_eq!(fs::read_dir(&codex_home).unwrap().count(), files_before);
    }

    #[test]
    fn successful_native_install_does_not_add_duplicate_mcp_or_skill() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("codex");
        let skill = temp.path().join("skill");
        let status = install_at(
            temp.path(),
            Path::new("app.exe"),
            &home,
            &skill,
            true,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(status.integration_mode, "plugin");
        assert!(!home.join("config.toml").exists());
        assert!(!skill.exists());
    }

    #[test]
    fn invalid_config_is_preserved_instead_of_overwritten_or_panicking() {
        let temp = tempfile::tempdir().unwrap();
        for original in ["broken = [", "mcp_servers = false", "plugins = false"] {
            fs::write(temp.path().join("config.toml"), original).unwrap();
            let result = install_mcp_fallback(
                temp.path(),
                Path::new("app.exe"),
                temp.path(),
                &temp.path().join("skill"),
            );
            assert!(result.is_err());
            assert_eq!(read_config(temp.path()).unwrap(), original);
        }
    }

    #[test]
    fn custom_codex_home_is_respected_and_relative_paths_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let custom = temp.path().join("another-profile");
        assert_eq!(
            resolve_codex_home(Some(custom.as_os_str()), temp.path()).unwrap(),
            custom
        );
        assert_eq!(
            resolve_codex_home(None, temp.path()).unwrap(),
            temp.path().join(".codex")
        );
        assert!(resolve_codex_home(Some(OsStr::new("relative")), temp.path()).is_err());
    }

    #[test]
    fn finds_cli_after_the_app_changes_its_internal_directory_layout() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(target_os = "windows")]
        let name = "codex.exe";
        #[cfg(not(target_os = "windows"))]
        let name = "codex";
        for layout in ["old/resources", "new/runtime/cli/nested/MacOS"] {
            let parent = temp.path().join(layout);
            fs::create_dir_all(&parent).unwrap();
            let binary = parent.join(name);
            fs::write(&binary, "test executable").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
            }
            assert_eq!(find_bundled_cli(temp.path(), 6), Some(binary.clone()));
            fs::remove_file(binary).unwrap();
        }
    }

    #[test]
    fn concurrent_config_change_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("config.toml"), "model = 'newer'\n").unwrap();
        assert!(write_config(temp.path(), "model = 'old'\n", "model = 'ours'\n").is_err());
        assert_eq!(read_config(temp.path()).unwrap(), "model = 'newer'\n");
    }

    #[test]
    fn hanging_cli_does_not_block_settings_or_fallback() {
        #[cfg(target_os = "windows")]
        let process = {
            let mut process = command("powershell.exe");
            process.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 5",
            ]);
            process
        };
        #[cfg(not(target_os = "windows"))]
        let process = {
            let mut process = command("/bin/sleep");
            process.arg("5");
            process
        };
        let started = Instant::now();
        assert!(output_with_timeout(process, Duration::from_millis(100)).is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    #[ignore = "requires SMILE_TEST_CODEX_CLI; uses an isolated Codex config"]
    fn current_codex_accepts_native_plugin_then_independent_fallback() {
        let cli = PathBuf::from(std::env::var_os("SMILE_TEST_CODEX_CLI").expect("CLI path"));
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("marketplace");
        let codex_home = temp.path().join("codex");
        let skill = temp.path().join("skill");
        bundle(&root);
        fs::create_dir_all(&codex_home).unwrap();
        install_native_plugin(&cli, &root, &codex_home).unwrap();
        let output = run_codex(&cli, &["plugin", "list", "--json"], &codex_home).unwrap();
        assert!(output.contains(PLUGIN_NAME));
        let status = install_at(
            &root,
            Path::new("smile-app.exe"),
            &codex_home,
            &skill,
            true,
            || bail!("simulate changed plugin command"),
        )
        .unwrap();
        assert_eq!(status.integration_mode, "mcp");
        let output = run_codex(&cli, &["mcp", "list", "--json"], &codex_home).unwrap();
        let servers: serde_json::Value = serde_json::from_str(&output).unwrap();
        let smile = servers
            .as_array()
            .unwrap()
            .iter()
            .find(|server| server["name"] == MCP_NAME)
            .unwrap();
        assert_eq!(smile["enabled"], true);
        assert_eq!(smile["transport"]["command"], "smile-app.exe");
        assert_eq!(smile["transport"]["args"], serde_json::json!(["--mcp"]));
        let config = read_config(&codex_home)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(
            config["plugins"]["smile-ai-image@smile-ai"]["enabled"].as_bool(),
            Some(false)
        );
    }
}
