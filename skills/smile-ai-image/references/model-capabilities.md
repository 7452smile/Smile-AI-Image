# Model capabilities

## Model and resolution mapping

| UI model | Resolution | Relay model ID |
| --- | --- | --- |
| Nano Banana 2 | 1K / 2K / 4K | `gemini-3.1-flash-image-preview-{1k,2k,4k}` |
| Nano Banana 2.1 | 1K / 2K / 4K | `gemini-nano-banana-2.1-{1k,2k,4k}` |
| Nano Banana Pro | 1K / 2K / 4K | `gemini-3-pro-image-preview-{1k,2k,4k}` |
| GPT Image 2 | Auto / 1K / 2K / 4K | `gpt-image-2`, with calculated `WIDTHxHEIGHT` |
| GPT Image 2 All | Auto / 1K / 2K / 4K | `gpt-image-2-all`, with the same calculated `WIDTHxHEIGHT` |
| GPT Image 2.5 Flare | Auto / 1K / 2K / 4K | `gpt-image-2.5-flare`, with the same calculated `WIDTHxHEIGHT` |
| GPT Image 2.5 Sunburst | Auto / 1K / 2K / 4K | `gpt-image-2.5-sunburst`, with the same calculated `WIDTHxHEIGHT` |

## Quality

GPT Image 2, GPT Image 2 All, GPT Image 2.5 Flare, and GPT Image 2.5 Sunburst support `AUTO`, `LOW`, `MEDIUM`, and `HIGH`. The saved default is `AUTO`. Quality is not sent to Nano Banana 2 or Nano Banana Pro.

## Aspect ratios

Nano Banana 2 and Nano Banana 2.1:

`auto`, `1:1`, `4:3`, `3:4`, `3:2`, `2:3`, `16:9`, `9:16`, `5:4`, `4:5`, `21:9`, `4:1`, `1:4`, `8:1`, `1:8`

Nano Banana Pro:

`auto`, `1:1`, `4:3`, `3:4`, `3:2`, `2:3`, `16:9`, `9:16`, `5:4`, `4:5`, `21:9`

GPT Image 2, GPT Image 2 All, GPT Image 2.5 Flare, and GPT Image 2.5 Sunburst presets:

`auto`, `1:1`, `4:3`, `3:4`, `3:2`, `2:3`, `16:9`, `9:16`, `5:4`, `4:5`, `2:1`, `1:2`, `21:9`, `9:21`, `3:1`, `1:3`

GPT Image 2, GPT Image 2 All, GPT Image 2.5 Flare, and GPT Image 2.5 Sunburst also accept custom `width:height` values between `1:3` and `3:1`. Their calculated edges must be multiples of 16, the maximum edge is 3840px, and total pixels cannot exceed 8,294,400.
