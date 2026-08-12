---
name: browser-cdp
description: "Native CDP browser automation reference. Headless/headed Chrome control, screenshots, JS evaluation. (/browser-cdp, browser automation, cdp, scraping)"
---

# Native Browser Automation (CDP)

Built-in browser control via Chrome DevTools Protocol — no Node.js, no Playwright, pure Rust.

## Tools

| Tool | Required | Optional | What |
|------|----------|---------|------|
| `browser_navigate` | `url` | `headless` | Navigate to URL |
| `browser_click` | `selector` | — | Click element by CSS selector |
| `browser_type` | `text` | `selector` | Type text into input |
| `browser_screenshot` | — | `selector` | Screenshot (returns file path) |
| `browser_eval` | `script` | — | Execute JavaScript |
| `browser_content` | — | `selector`, `text_only` | Extract text/HTML |
| `browser_wait` | — | `selector`, `timeout_secs`, `delay_secs` | Wait for element or delay |
| `browser_find` | — | `pattern`, `mode`, `limit` | Inventory (no `pattern`) or find elements |

## Headless vs Headed

- **Headless (default):** No visible window. Fast, low resources.
- **Headed:** Visible Chrome window. Pass `headless: false` to `browser_navigate`.

## Usage Tips

- Compose workflows: navigate → inventory/find → click → type → screenshot
- **Inventory mode:** call `browser_find` with NO `pattern` to get every visible
  interactive element on the page, each with a stable `[data-opencrabs-match="N"]`
  selector. Prefer this over `browser_screenshot` when you have just landed and do
  not yet know what to click — text beats pixels for deciding where to act.
- Screenshots return file paths. Use `<<IMG:path>>` to send to channels.
- JavaScript evaluation for complex DOM manipulation and SPAs.
- No credentials stored. Navigate to login page and use click/type for auth (with user approval).
- Requires Chrome/Chromium installed. Feature-gated: `browser` flag.

## Chrome Launch Failures

1. **SingletonLock exists:** `rm -f ~/.opencrabs/chrome-profile/SingletonLock ~/.opencrabs/chrome-profile/SingletonCookie ~/.opencrabs/chrome-profile/SingletonSocket` then retry.
2. **DevTools data dir conflict:** `pkill -f chrome` then retry.
3. **Channel closed:** Kill Chrome, delete lock files, retry.
