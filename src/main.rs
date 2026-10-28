//! `kestrel shot`: headless web page screenshots for CI and agents.
//!
//! No screen, no tray app, no permissions: pages render in headless Chrome over the DevTools
//! protocol, so the same command works on Linux, macOS and Windows runners.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use base64::Engine;
use headless_chrome::protocol::cdp::{Emulation, Page};
use headless_chrome::{Browser, LaunchOptions, Tab};
use serde_json::{Value, json};

const USAGE: &str = "\
kestrel shot <url|file>... [options]

Captures web pages with headless Chrome (CHROME=/path overrides the auto-detected browser).

Options:
  -o, --out <dir>         output directory (default: .)
  --viewport <WxH>        viewport in CSS px, repeatable (default: 1440x900)
  --scale <n>             device pixel ratio (default: 1)
  --full-page             capture the whole scrollable page
  --selector <css>        capture only the first element matching <css>
  --wait-for <css>        wait until <css> exists before capturing
  --wait <ms>             extra delay after load (default: 0)
  --hide <css>            hide matching elements, repeatable (cookie banners, chat widgets)
  --dark                  emulate prefers-color-scheme: dark
  --format <png|jpeg|webp>  (default: png)
  --timeout <s>           per-page load timeout (default: 30)
  --no-sandbox            run Chrome without its sandbox (root in Docker)
  --json                  print results as JSON on stdout
  -h, --help              show this help
  -V, --version           show the version";

#[derive(Debug)]
struct Opts {
    targets: Vec<String>,
    out: PathBuf,
    viewports: Vec<(u32, u32)>,
    scale: f64,
    full_page: bool,
    selector: Option<String>,
    wait_for: Option<String>,
    wait: Duration,
    hide: Vec<String>,
    dark: bool,
    format: String,
    timeout: Duration,
    sandbox: bool,
    json: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-V" | "--version") => {
            println!("kestrel {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("shot") => match parse(&args[1..]) {
            Ok(o) => run(&o),
            Err(e) => {
                eprintln!("error: {e}\n\n{USAGE}");
                ExitCode::from(2)
            }
        },
        Some("-h" | "--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts {
        targets: vec![],
        out: ".".into(),
        viewports: vec![],
        scale: 1.0,
        full_page: false,
        selector: None,
        wait_for: None,
        wait: Duration::ZERO,
        hide: vec![],
        dark: false,
        format: "png".into(),
        timeout: Duration::from_secs(30),
        sandbox: true,
        json: false,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "-o" | "--out" => o.out = val()?.into(),
            "--viewport" => o.viewports.push(parse_viewport(&val()?)?),
            "--scale" => o.scale = val()?.parse().ok().filter(|s: &f64| *s > 0.0 && *s <= 4.0).ok_or("--scale must be 0-4")?,
            "--full-page" => o.full_page = true,
            "--selector" => o.selector = Some(val()?),
            "--wait-for" => o.wait_for = Some(val()?),
            "--wait" => o.wait = Duration::from_millis(val()?.parse().map_err(|_| "--wait takes milliseconds")?),
            "--hide" => o.hide.push(val()?),
            "--dark" => o.dark = true,
            "--format" => {
                o.format = val()?;
                if !matches!(o.format.as_str(), "png" | "jpeg" | "webp") {
                    return Err("--format must be png, jpeg or webp".into());
                }
            }
            "--timeout" => o.timeout = Duration::from_secs(val()?.parse().map_err(|_| "--timeout takes seconds")?),
            "--no-sandbox" => o.sandbox = false,
            "--json" => o.json = true,
            "-h" | "--help" => return Err("help".into()),
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => o.targets.push(s.to_string()),
        }
    }
    if o.targets.is_empty() {
        return Err("no url given".into());
    }
    if o.viewports.is_empty() {
        o.viewports.push((1440, 900));
    }
    Ok(o)
}

fn parse_viewport(s: &str) -> Result<(u32, u32), String> {
    let bad = || format!("bad viewport {s:?}, expected WxH like 1440x900");
    let (w, h) = s.split_once('x').ok_or_else(bad)?;
    let (w, h): (u32, u32) = (w.parse().map_err(|_| bad())?, h.parse().map_err(|_| bad())?);
    if !(100..=10_000).contains(&w) || !(100..=10_000).contains(&h) {
        return Err(bad());
    }
    Ok((w, h))
}

/// Existing local path → `file://`, bare host → `https://`, anything with a scheme as is.
fn to_url(target: &str) -> String {
    if target.contains("://") {
        return target.to_string();
    }
    match Path::new(target).canonicalize() {
        Ok(p) => {
            let p = p.to_string_lossy().replace('\\', "/");
            let p = p.trim_start_matches("//?/");
            if p.starts_with('/') { format!("file://{p}") } else { format!("file:///{p}") }
        }
        Err(_) => format!("https://{target}"),
    }
}

/// `https://example.com/docs?a=1` → `example.com-docs-a-1`
fn slug(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    // Local files: the file name, not the whole absolute path.
    let rest = if url.starts_with("file://") { rest.rsplit('/').next().unwrap_or(rest) } else { rest };
    let mut s = String::new();
    for c in rest.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s: String = s.trim_matches(['-', '.']).chars().take(80).collect();
    if s.is_empty() { "page".into() } else { s }
}

fn run(o: &Opts) -> ExitCode {
    if let Err(e) = std::fs::create_dir_all(&o.out) {
        eprintln!("error: can't create {}: {e}", o.out.display());
        return ExitCode::FAILURE;
    }
    let browser = match launch(o) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: couldn't start Chrome: {e}\nInstall Chrome or Chromium, or set CHROME=/path/to/chrome.");
            return ExitCode::FAILURE;
        }
    };
    let mut results = vec![];
    let mut failed = false;
    for target in &o.targets {
        let url = to_url(target);
        for &(w, h) in &o.viewports {
            let name = if o.viewports.len() > 1 { format!("{}-{w}x{h}", slug(&url)) } else { slug(&url) };
            let path = o.out.join(format!("{name}.{}", o.format));
            let viewport = format!("{w}x{h}");
            match shoot(&browser, o, &url, w, h).and_then(|bytes| Ok(std::fs::write(&path, bytes)?)) {
                Ok(()) => {
                    if !o.json {
                        println!("{}", path.display());
                    }
                    results.push(json!({ "url": url, "viewport": viewport, "path": path }));
                }
                Err(e) => {
                    failed = true;
                    eprintln!("error: {url} @ {viewport}: {e}");
                    results.push(json!({ "url": url, "viewport": viewport, "error": e.to_string() }));
                }
            }
        }
    }
    if o.json {
        println!("{}", Value::Array(results));
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn launch(o: &Opts) -> anyhow::Result<Browser> {
    let opts = LaunchOptions::default_builder()
        .headless(true)
        .sandbox(o.sandbox)
        .window_size(Some((1440, 900)))
        // Without this the browser quits after 30 s idle, mid-capture on slow pages.
        .idle_browser_timeout(o.timeout + Duration::from_secs(60))
        .args(vec![std::ffi::OsStr::new("--hide-scrollbars")])
        .build()
        .map_err(|e| anyhow::anyhow!(e))?;
    Browser::new(opts)
}

fn shoot(browser: &Browser, o: &Opts, url: &str, w: u32, h: u32) -> anyhow::Result<Vec<u8>> {
    let tab = browser.new_tab()?;
    let res = capture(&tab, o, url, w, h);
    let _ = tab.close(false);
    res
}

fn capture(tab: &Tab, o: &Opts, url: &str, w: u32, h: u32) -> anyhow::Result<Vec<u8>> {
    tab.set_default_timeout(o.timeout);
    tab.call_method(Emulation::SetDeviceMetricsOverride {
        width: w,
        height: h,
        device_scale_factor: o.scale,
        mobile: false,
        scale: None,
        screen_width: None,
        screen_height: None,
        position_x: None,
        position_y: None,
        dont_set_visible_size: None,
        screen_orientation: None,
        viewport: None,
        display_feature: None,
        device_posture: None,
    })?;
    if o.dark {
        tab.call_method(Emulation::SetEmulatedMedia {
            media: None,
            features: Some(vec![Emulation::MediaFeature { name: "prefers-color-scheme".into(), value: "dark".into() }]),
        })?;
    }
    tab.navigate_to(url)?.wait_until_navigated()?;
    if let Some(sel) = &o.wait_for {
        tab.wait_for_element_with_custom_timeout(sel, o.timeout)?;
    }
    if !o.hide.is_empty() {
        let css = format!("{} {{ visibility: hidden !important; }}", o.hide.join(", "));
        tab.evaluate(&format!("document.head.appendChild(Object.assign(document.createElement('style'), {{ textContent: {} }}))", json!(css)), false)?;
    }
    // Web fonts swap in after `load`; capturing earlier shows fallback fonts.
    tab.evaluate("document.fonts.ready.then(() => true)", true)?;
    std::thread::sleep(o.wait);

    let clip = match (&o.selector, o.full_page) {
        (Some(sel), _) => Some(rect(tab, &format!(
            "(() => {{ const e = document.querySelector({}); if (!e) return null; const r = e.getBoundingClientRect(); \
             return [r.left + scrollX, r.top + scrollY, r.width, r.height]; }})()",
            json!(sel)
        )).ok_or_else(|| anyhow::anyhow!("no element matches {sel:?}"))?),
        // ponytail: one capture, Chrome caps textures near 16k px; tile it if taller pages matter.
        (None, true) => rect(tab, "[0, 0, document.documentElement.scrollWidth, Math.min(document.documentElement.scrollHeight, 16000)]"),
        (None, false) => None,
    };
    let format = match o.format.as_str() {
        "jpeg" => Page::CaptureScreenshotFormatOption::Jpeg,
        "webp" => Page::CaptureScreenshotFormatOption::Webp,
        _ => Page::CaptureScreenshotFormatOption::Png,
    };
    let data = tab
        .call_method(Page::CaptureScreenshot {
            quality: (o.format != "png").then_some(90),
            format: Some(format),
            capture_beyond_viewport: Some(clip.is_some()),
            clip,
            from_surface: Some(true),
            optimize_for_speed: None,
        })?
        .data;
    Ok(base64::prelude::BASE64_STANDARD.decode(data)?)
}

/// Evaluates JS returning `[x, y, w, h]` in document coordinates, or null.
fn rect(tab: &Tab, js: &str) -> Option<Page::Viewport> {
    // Objects come back as remote references, so ship the array as a JSON string.
    let v = tab.evaluate(&format!("JSON.stringify({js})"), false).ok()?.value?;
    let v: Value = serde_json::from_str(v.as_str()?).ok()?;
    let n: Vec<f64> = v.as_array()?.iter().filter_map(Value::as_f64).collect();
    let [x, y, width, height] = n[..] else { return None };
    (width > 0.0 && height > 0.0).then_some(Page::Viewport { x, y, width, height, scale: 1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_urls() {
        assert_eq!(slug("https://example.com/docs?a=1"), "example.com-docs-a-1");
        assert_eq!(slug("https://example.com/"), "example.com");
        assert_eq!(slug("file:///"), "page");
        assert_eq!(slug("file:///home/runner/dist/index.html"), "index.html");
        assert_eq!(to_url("example.com"), "https://example.com");
        assert_eq!(to_url("http://localhost:3000"), "http://localhost:3000");
        assert!(to_url("Cargo.toml").starts_with("file:///"));
    }

    #[test]
    fn parses_args() {
        let a = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        let o = parse(&a("example.com --viewport 390x844 --viewport 1440x900 --full-page -o out")).unwrap();
        assert_eq!(o.viewports, [(390, 844), (1440, 900)]);
        assert!(o.full_page && o.out == Path::new("out"));
        assert_eq!(parse(&a("example.com")).unwrap().viewports, [(1440, 900)]);
        assert!(parse(&a("example.com --viewport 10x10")).is_err());
        assert!(parse(&a("--dark")).is_err());
    }
}
