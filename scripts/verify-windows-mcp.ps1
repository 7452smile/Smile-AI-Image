$ErrorActionPreference = "Stop"
$utf8 = New-Object System.Text.UTF8Encoding $false
[Console]::InputEncoding = $utf8
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8

$executable = Join-Path $env:LOCALAPPDATA "Smile AI Image\smile-ai-image-settings.exe"
$request = '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
$response = $request | & $executable --mcp | ConvertFrom-Json
$tool = $response.result.tools | Where-Object name -eq "generate_image"
$variants = $tool.inputSchema.properties.variantPrompts
$settingsRequest = '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_settings","arguments":{}}}'
$settingsResponse = $settingsRequest | & $executable --mcp | ConvertFrom-Json
$settings = $settingsResponse.result.content[0].text | ConvertFrom-Json

if ($null -eq $variants) {
    throw "generate_image schema does not include variantPrompts"
}

[pscustomobject]@{
    Version = (Get-Item $executable).VersionInfo.FileVersion
    HasApiKey = $settings.hasApiKey
    VariantPromptsType = $variants.type
    VariantPromptsMaxItems = $variants.maxItems
} | ConvertTo-Json -Compress
