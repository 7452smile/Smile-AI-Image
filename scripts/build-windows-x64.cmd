@echo off
setlocal

set "VSDEVCMD=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"
if not exist "%VSDEVCMD%" (
  echo Visual Studio 2022 Build Tools not found.
  exit /b 1
)

set "HOST_ARCH=x64"
if /I "%PROCESSOR_ARCHITECTURE%"=="ARM64" set "HOST_ARCH=arm64"

call "%VSDEVCMD%" -arch=x64 -host_arch=%HOST_ARCH%
if errorlevel 1 exit /b %errorlevel%

set "PROJECT_ROOT=%~dp0.."
cargo test --manifest-path "%PROJECT_ROOT%\Cargo.toml" --workspace --target x86_64-pc-windows-msvc
if errorlevel 1 exit /b %errorlevel%

call npm.cmd --prefix "%PROJECT_ROOT%\settings" run tauri -- build --target x86_64-pc-windows-msvc --bundles nsis
if errorlevel 1 exit /b %errorlevel%

endlocal
