---
name: smile-ai-image
description: Generate or edit raster images through the fixed Smile AI relay using Nano Banana 2, Nano Banana 2.1, Nano Banana Pro, GPT Image 2, GPT Image 2 All, GPT Image 2.5 Flare, or GPT Image 2.5 Sunburst. Use when the user asks Codex to generate, draw, create, retouch, transform, restyle, or otherwise edit an image with Smile AI Image; asks to check image-model settings or balance; or asks to open Smile AI Image settings.
---

# Smile AI Image

MIT open source. [Source code and issues](https://github.com/7452smile/Smile-AI-Image) · [Smile AI website](https://api.smile-ai-studio.com/).

Use the bundled `smile_ai_image` MCP tools. Never request or expose the user's complete API Key in conversation.

## Route the request

1. Call `open_settings` when the user asks to configure the plugin or says “打开 Smile AI Image 设置”.
2. Call `check_balance` when the user asks about authentication or balance.
3. For image generation or editing, call `get_settings` first and use saved defaults for omitted model, resolution, ratio, and GPT Image quality. If the saved resolution is `FOURK`, pass `FOURK` explicitly; do not replace it with `AUTO` or `ONEK`. Only pass `AUTO` when the user explicitly asks for automatic sizing. `AUTO` on Banana models requests 1K.
4. Call `get_model_capabilities` when the user requests an unusual ratio or when changing models would affect supported ratios.

## Understand image requests

- Treat a prompt without attached images as generation.
- Treat attached images plus a clear modification instruction as editing. Pass every relevant local image path in `inputImages`, preserving the order referenced by the user.
- If the user sends only images, ask what they want changed. Do not infer an edit.
- If an edit instruction is ambiguous, ask what specifically should change, summarize the intended edit, and ask for confirmation before calling the tool.
- If the generation or edit instruction is clear, call the tool directly without an extra confirmation.
- Infer image roles from natural language. When multiple images are used, make the final prompt identify them by order.

## Build the tool call

- Preserve the user's requested model, resolution, ratio, quality, count, and output directory.
- Nano Banana 2.1 uses `gemini-nano-banana-2.1-1k`, `gemini-nano-banana-2.1-2k`, or `gemini-nano-banana-2.1-4k` according to the selected resolution.
- Use saved defaults for omitted values. Do not invent model, resolution, ratio, or quality overrides.
- Pass `quality` only for GPT Image 2, GPT Image 2 All, GPT Image 2.5 Flare, or GPT Image 2.5 Sunburst. Map natural-language requests for draft/fast, balanced, or final/high quality to `LOW`, `MEDIUM`, or `HIGH`; otherwise keep the saved default.
- Set `name` to a short Chinese subject extracted from the prompt, such as `上海外滩`.
- Keep count between 1 and 20. The MCP performs separate `n: 1` requests with at most 6 running concurrently; remaining outputs wait in the internal queue.
- When the user requests multiple outputs with different compositions, scenes, or requirements, use `variantPrompts`. Write exactly one complete single-image prompt per output; never combine multiple layouts into one prompt. `variantPrompts` overrides `count`.
- Complete all requested outputs in exactly one `generate_image` tool call. One confirmed user request authorizes only that one call.
- Do not pass model-specific parameters that are absent from the tool schema.
- For model and ratio details, read [model-capabilities.md](references/model-capabilities.md).

## Present results

- The tool saves every result and returns image content for Codex preview.
- The result includes the relay model ID, requested size, expected pixels, actual pixels, and warnings. Treat a warning such as actual `1024×1024` for a 4K request as an upstream resolution mismatch; report it to the user and do not claim the image is 4K.
- Show each returned image in the conversation and report its saved absolute path.
- Do not create another history database or copy the output elsewhere unless requested.
- Treat a successfully returned image as a completed, billable generation even when its composition or appearance is unsatisfactory. Do not regenerate, correct, or replace it automatically.
- Before any additional billable generation after a successful call, explain what would change and obtain explicit user confirmation.
- Never imply that moving or deleting a local output cancels the relay request, usage record, or charge.

## Handle failures

- Preserve the relay's HTTP status and original error text in the response.
- If only some outputs fail, present every successful image plus the failed output numbers and reasons. Do not discard successful results.
- Retry only timeouts, network interruptions, HTTP 429, and temporary 5xx errors.
- Make at most one retry and keep the request identical.
- An aesthetic, composition, spelling, or subject mismatch in a successfully returned image is not a retryable failure.
- Do not retry invalid Key, insufficient balance, policy violations, unsupported ratios, or bad parameters.
- Tell the user to open settings when no API Key is configured.
- Warn when a known USD balance is below `$1.00`. An authenticated Key with unknown or unlimited balance remains valid.
