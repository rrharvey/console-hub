//! `console-hub capture <provider folder | page URL> [out folder]`, for provider authors
//! (see PLUGINS.md). Opens the page in a window with the app's logins, records every JSON
//! response it fetches, writes them to `<out folder>/<name>-<time>.json` (default
//! `captures/`) and quits. You can sign in in that window if needed.
//!
//! Given a provider folder it waits until the provider reads a balance (then a few
//! seconds for optional captures); given a URL, until responses stop arriving. The
//! output is what `console-hub fixture` reads. It holds account data and session tokens.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tauri::{webview::PageLoadEvent, Context, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::providers::{self, Extracted, Provider};
use crate::{close_popups, eval_string, open_popup, WithVendorUserAgent, SCRAPE_JS};

const POLL: Duration = Duration::from_secs(1);
/// After a provider reads its balance, for optional captures.
const AFTER_BALANCE: Duration = Duration::from_secs(3);
/// With a URL: done once no response has arrived for this long.
const QUIET: Duration = Duration::from_secs(6);
const LIMIT: Duration = Duration::from_secs(60);
/// While the page sits on a login, wait this long for the user to sign in.
const LOGIN_LIMIT: Duration = Duration::from_secs(300);

pub struct Target {
    /// For the file name: the provider id or the page's host.
    name: String,
    page: Url,
    provider: Option<Provider>,
    out: PathBuf,
}

pub fn parse(args: &[String]) -> Result<Target, String> {
    let (what, out) = match args {
        [what] => (what, PathBuf::from("captures")),
        [what, out] => (what, PathBuf::from(out)),
        _ => return Err("usage: console-hub capture <provider folder | page URL> [out folder]".into()),
    };
    if what.starts_with("https://") {
        let page = Url::parse(what).map_err(|e| format!("{what}: {e}"))?;
        let name = page.host_str().unwrap_or("page").to_string();
        return Ok(Target { name, page, provider: None, out });
    }
    let (mut found, errors) = providers::inspect_all(Path::new(what));
    if found.len() != 1 {
        return Err(format!("{what}: {}", errors.first().map(String::as_str).unwrap_or("choose one provider's folder")));
    }
    let p = found.remove(0);
    let metric = p.metric().ok_or(format!("{what}: the provider has no metric, so there's no page to capture"))?;
    let page = Url::parse(&metric.page).map_err(|e| e.to_string())?;
    Ok(Target { name: p.manifest.id.clone(), page, provider: Some(p), out })
}

pub fn run(context: Context, target: Target) {
    let title = format!("Capturing {}", target.page);
    tauri::Builder::default()
        .setup(move |app| {
            let (captures, domains) = match &target.provider {
                Some(p) => {
                    let m = p.metric().expect("checked in parse");
                    (m.captures.values().cloned().collect::<Vec<_>>(), p.manifest.domains.clone())
                }
                None => (Vec::new(), vec![target.page.host_str().unwrap_or_default().to_string()]),
            };
            let config = serde_json::json!({ "captures": captures, "domains": domains, "capture": true });
            let init = format!("window.__chConfig = {config};\n{SCRAPE_JS}");
            let popup_app = app.handle().clone();
            let window = WebviewWindowBuilder::new(app, "capture", WebviewUrl::External(target.page.clone()))
                .title(&title)
                .inner_size(1280.0, 860.0)
                .with_vendor_user_agent()
                .initialization_script(&init)
                .on_new_window(move |url, features| open_popup(&popup_app, url, features))
                .on_page_load(|window, payload| {
                    if matches!(payload.event(), PageLoadEvent::Started) {
                        close_popups(window.app_handle());
                    }
                })
                .build()?;
            let app = app.handle().clone();
            std::thread::spawn(move || {
                let code = match watch(&window, &target) {
                    Ok(lines) => {
                        lines.iter().for_each(|l| println!("{l}"));
                        0
                    }
                    Err(e) => {
                        eprintln!("\u{2717} {e}");
                        1
                    }
                };
                app.exit(code);
            });
            Ok(())
        })
        .run(context)
        .expect("error while running Console Hub capture");
}

#[derive(Deserialize)]
struct Progress {
    login: bool,
    count: usize,
}

#[derive(Deserialize)]
struct PageResult {
    #[serde(default)]
    responses: Vec<providers::Response>,
}

/// Wait until the page is done, then write the capture. Returns what to tell the user.
fn watch(window: &tauri::WebviewWindow, target: &Target) -> Result<Vec<String>, String> {
    let webview = window.as_ref();
    let started = Instant::now();
    let mut last_count = 0;
    let mut last_change = Instant::now();
    let mut balance_at: Option<Instant> = None;
    let mut balance = None;
    let mut told_login = false;
    loop {
        std::thread::sleep(POLL);
        let Some(raw) = eval_string(webview, "window.__ch ? window.__ch.progress() : ''", Duration::from_secs(5)) else {
            continue;
        };
        let Ok(p) = serde_json::from_str::<Progress>(&raw) else { continue };
        if p.count != last_count {
            last_count = p.count;
            last_change = Instant::now();
        }
        if p.login {
            if !told_login {
                println!("The page wants you to sign in. Sign in in the capture window; waiting\u{2026}");
                told_login = true;
            }
            if started.elapsed() > LOGIN_LIMIT {
                return Err("still on a sign-in page; nothing written".into());
            }
            continue;
        }
        if let Some(p) = &target.provider {
            if balance_at.is_none() {
                let raw = eval_string(webview, "window.__ch.result()", Duration::from_secs(10)).unwrap_or_default();
                let page: PageResult = serde_json::from_str(&raw).unwrap_or(PageResult { responses: Vec::new() });
                if let Ok(Extracted::Ok { value, .. }) = providers::extract(p, p.metric().expect("checked"), &page.responses) {
                    balance = Some(value);
                    balance_at = Some(Instant::now());
                }
            }
            if balance_at.is_some_and(|t| t.elapsed() > AFTER_BALANCE) {
                break;
            }
        } else if last_count > 0 && last_change.elapsed() > QUIET {
            break;
        }
        if started.elapsed() > LIMIT + if told_login { LOGIN_LIMIT } else { Duration::ZERO } {
            break;
        }
    }

    let raw = eval_string(webview, "window.__ch.capture()", Duration::from_secs(60)).ok_or("couldn't read the capture from the page")?;
    let mut capture: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    capture["page"] = target.page.as_str().into();
    let count = capture["responses"].as_array().map_or(0, Vec::len);
    std::fs::create_dir_all(&target.out).map_err(|e| format!("{}: {e}", target.out.display()))?;
    let file = target.out.join(format!("{}-{}.json", target.name, timestamp()));
    let json = serde_json::to_string_pretty(&capture).map_err(|e| e.to_string())?;
    std::fs::write(&file, json + "\n").map_err(|e| format!("{}: {e}", file.display()))?;

    let final_url = capture["url"].as_str().unwrap_or_default().to_string();
    let mut lines = vec![format!("\u{2713} wrote {} ({count} JSON responses from {final_url})", file.display())];
    if let Some(p) = &target.provider {
        lines.push(match balance {
            Some(v) => format!("  {} read: {v}", p.manifest.name),
            None => format!("  ! {} read no balance in time; look through the responses for it", p.manifest.name),
        });
        lines.push(format!("  Next: npm run fixture -- {} {} <name>", file.display(), p.dir.display()));
    }
    lines.push("  It holds account data and session tokens. Don't commit or share it; delete it when done.".into());
    Ok(lines)
}

/// UTC `YYYYMMDD-HHMMSS`, for file names.
fn timestamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn timestamp_looks_right() {
        let t = super::timestamp();
        assert_eq!(t.len(), 15, "{t}");
        assert!(t.starts_with("20") && t.as_bytes()[8] == b'-', "{t}");
    }
}
