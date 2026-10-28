# kestrel-cli

`kestrel shot` screenshots web pages without a screen. It drives headless Chrome over the DevTools protocol, so the same command works on Linux, macOS and Windows: CI runners, containers, or an agent's shell. Chrome is preinstalled on GitHub Actions runners; elsewhere install Chrome or Chromium, or set `CHROME=/path/to/chrome`.

## Install

```bash
# Linux x64 (also: kestrel-linux-arm64, kestrel-macos, kestrel-windows-x64.exe)
curl -fsSL https://github.com/Pixel-Process-UG/kestrel-cli/releases/latest/download/kestrel-linux-x64 -o kestrel
chmod +x kestrel
```

Or from source: `cargo install --git https://github.com/Pixel-Process-UG/kestrel-cli`.

## Use

```bash
kestrel shot https://example.com dist/index.html \
  --viewport 390x844 --viewport 1440x900 --full-page --dark \
  --wait-for "#app" --hide ".cookie-banner" -o screenshots --json
```

| Option | |
|---|---|
| `-o, --out <dir>` | output directory (default `.`) |
| `--viewport <WxH>` | viewport in CSS px, repeatable (default `1440x900`) |
| `--scale <n>` | device pixel ratio (default 1) |
| `--full-page` | the whole scrollable page (up to 16 000 px tall) |
| `--selector <css>` | only the first matching element |
| `--wait-for <css>` | wait until the element exists |
| `--wait <ms>` | extra delay after load |
| `--hide <css>` | hide matching elements, repeatable |
| `--dark` | emulate `prefers-color-scheme: dark` |
| `--format <png\|jpeg\|webp>` | default `png` |
| `--timeout <s>` | per-page load timeout (default 30) |
| `--no-sandbox` | run Chrome without its sandbox (root in Docker) |
| `--json` | results as JSON on stdout |

Local paths become `file://` URLs, bare hosts get `https://`. Files are named after the page (`example.com.png`, or `example.com-390x844.png` with several viewports). `--json` prints `{url, viewport, path}` per capture or `{url, viewport, error}`; the exit code is non-zero if any capture failed.

## GitHub Action

```yaml
- uses: Pixel-Process-UG/kestrel-cli@v0.1.0
  with:
    urls: http://localhost:3000 http://localhost:3000/pricing
    args: --full-page --viewport 390x844 --viewport 1440x900
- uses: actions/upload-artifact@v4
  with: { name: screenshots, path: screenshots }
```

Inputs: `urls`, `args`, `out` (default `screenshots`), `version` (defaults to the action's tag). Output: `json`.

## Release

Push a `v*` tag; the release workflow builds the four binaries and publishes the release.
