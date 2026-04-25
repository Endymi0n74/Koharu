---
title: Use Codex Image Generation
---

# Use Codex Image Generation

Koharu can use Codex for end-to-end image-to-image generation. The workflow sends a source page image and a prompt to Codex, then stores the generated image as a rendered page result.

## Requirements

- a ChatGPT account with Codex access
- two-factor authentication enabled on that account
- network access to OpenAI and ChatGPT services

Two-factor authentication is required before device-code login can complete successfully.

## What the feature does

Codex image-to-image generation is a full-page redraw workflow. It can use the source image and prompt to:

- translate visible text
- remove original lettering
- redraw edited regions
- preserve panel layout, speech bubbles, tone, and composition
- produce a generated page image in one pass

This is separate from Koharu's staged local pipeline, where detection, OCR, inpainting, translation, and rendering run as individual steps. The Codex workflow sends the page image to a remote service and receives a generated image result.

## Prompting

Use a prompt that describes the complete page-level result you want. For example:

```text
Translate all visible text to natural English, remove the original lettering,
and redraw the page as a clean manga image while preserving the artwork,
panel layout, speech bubbles, tone, and composition.
```

For narrower edits, describe the target change and what must be preserved. The model receives the source page image, so the prompt should focus on transformation goals rather than restating every visual detail.

## Privacy and reliability

This feature sends the source page image and prompt to the ChatGPT Codex backend. Use the local pipeline instead when you need offline processing or do not want to send page images to a remote provider.

Codex image generation depends on OpenAI's upstream service. If generation fails, Koharu surfaces the upstream response text and request ID when available. Retrying can succeed if the failure is transient. Persistent failures may indicate account access, service availability, or backend support limitations for image-generation tool calls.

## When to use it

Use Codex image generation when you want a fast end-to-end redraw and are comfortable with a remote model rewriting the final image.

Use the local staged pipeline when you want more control over intermediate OCR, cleanup masks, translation text, fonts, and editable output.
