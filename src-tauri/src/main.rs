#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use tauri::{
    menu::{Menu, MenuEvent, MenuItem},
    webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent, WebviewBuilder},
    AppHandle, LogicalPosition, LogicalSize, Manager, State, Url, Webview, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};

const SIDEBAR_WIDTH: f64 = 248.0;
const VENDOR_PREFIX: &str = "vendor-";
// Hidden background webviews that load each vendor's billing page to read the balance.
const SCRAPER_PREFIX: &str = "scrape-";
const SCRAPE_JS: &str = include_str!("../../ui/scrape.js");
const SCRAPE_TIMEOUT: Duration = Duration::from_secs(45);
const SCRAPE_POLL: Duration = Duration::from_millis(1500);
const SCRAPE_RELOAD_AT: [Duration; 2] = [Duration::from_secs(15), Duration::from_secs(30)];
const POPUP_PREFIX: &str = "popup-";
// Sidebar right-click menu item; the id carries the URL to copy.
const COPY_LINK_PREFIX: &str = "copy-link:";
static POPUP_COUNTER: AtomicU32 = AtomicU32::new(0);
// User agent for vendor webviews and login popups: the real Safari UA for the Safari
// installed on this Mac.
// - The default WKWebView UA lacks the `Version/… Safari/…` tokens, and some logins
//   (Anthropic via WorkOS) reject it.
// - Claiming to be Chrome made Google sign-in fail with "This browser or app may not be
//   secure": the page runs on WebKit, so a Chrome UA is an obvious mismatch.
// Safari's UA always reports macOS as 10_15_7; only the Version token changes.
const SAFARI_VERSION_FALLBACK: &str = "26.0";

fn vendor_user_agent() -> &'static str {
    static UA: OnceLock<String> = OnceLock::new();
    UA.get_or_init(|| {
        let version = Command::new("/usr/bin/defaults")
            .args(["read", "/Applications/Safari.app/Contents/Info.plist", "CFBundleShortVersionString"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
            .unwrap_or_else(|| SAFARI_VERSION_FALLBACK.to_string());
        format!(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{version} Safari/605.1.15"
        )
    })
}

fn window_size(window: &tauri::Window) -> tauri::Result<LogicalSize<f64>> {
    let scale = window.scale_factor()?;
    Ok(window.inner_size()?.to_logical::<f64>(scale))
}

// Height of the macOS title bar drawn over the top of the content area, in logical px.
// Tauri gives macOS windows a full-size content view (a devtools workaround), so the
// title bar covers the top of every child webview. WebKit insets each page's viewport
// by that amount, which is how we measure it (see `measure_top_inset`). Stored as f64 bits.
static TOP_INSET: AtomicU64 = AtomicU64::new(0);

fn top_inset() -> f64 {
    f64::from_bits(TOP_INSET.load(Ordering::Relaxed))
}

/// Area for vendor consoles: right of the sidebar, below the title bar.
fn content_size(window: &tauri::Window) -> tauri::Result<LogicalSize<f64>> {
    let full = window_size(window)?;
    Ok(LogicalSize::new(
        (full.width - SIDEBAR_WIDTH).max(0.0),
        (full.height - top_inset()).max(0.0),
    ))
}

fn content_origin() -> LogicalPosition<f64> {
    LogicalPosition::new(SIDEBAR_WIDTH, top_inset())
}

/// Measure the title bar inset from the sidebar, which spans the full window height:
/// its frame height minus the viewport height WebKit gives the page. Re-lays out the
/// webviews if it changed (e.g. entering or leaving full screen). Call off the main thread.
fn measure_top_inset(app: &AppHandle) {
    let (Some(window), Some(sidebar)) = (app.get_window("main"), app.get_webview("sidebar")) else {
        return;
    };
    let Ok(full) = window_size(&window) else { return };
    let Some(inner) = eval_string(&sidebar, "String(window.innerHeight)", Duration::from_secs(3)) else {
        return;
    };
    let Ok(inner) = inner.parse::<f64>() else { return };
    let inset = (full.height - inner).clamp(0.0, 200.0).round();
    if (inset - top_inset()).abs() >= 1.0 {
        TOP_INSET.store(inset.to_bits(), Ordering::Relaxed);
        layout(app);
    }
}

fn layout(app: &AppHandle) {
    let Some(window) = app.get_window("main") else {
        return;
    };
    let (Ok(full), Ok(content)) = (window_size(&window), content_size(&window)) else {
        return;
    };

    for (label, webview) in app.webviews() {
        if label == "sidebar" {
            let _ = webview.set_position(LogicalPosition::new(0.0, 0.0));
            let _ = webview.set_size(LogicalSize::new(SIDEBAR_WIDTH, full.height));
        } else if label.starts_with(VENDOR_PREFIX) {
            let _ = webview.set_position(content_origin());
            let _ = webview.set_size(content);
        }
    }
}

/// Serve a `window.open()` request from a vendor webview as a real popup window.
///
/// Without a new-window handler, WebKit returns `null` from `window.open()`: OAuth
/// popups (Anthropic/WorkOS, Google sign-in) never appear and their `postMessage`
/// back to the opener has nowhere to go. Building the popup on the opener's
/// WKWebViewConfiguration (done by `window_features`) makes it share the web process,
/// cookies and `window.opener` with the vendor webview, so the handshake completes.
fn open_popup(app: &AppHandle, url: Url, features: NewWindowFeatures) -> NewWindowResponse<tauri::Wry> {
    let n = POPUP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let label = format!("{POPUP_PREFIX}{n}");
    let title = url.host_str().unwrap_or("Sign in").to_string();
    // WebKit loads the requested URL into the popup itself; the builder URL is a placeholder.
    let built = WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::External("about:blank".parse().unwrap()),
    )
    .title(title)
    .inner_size(520.0, 700.0)
    .user_agent(vendor_user_agent())
    .window_features(features)
    .on_document_title_changed(|window, title| {
        let _ = window.set_title(&title);
    })
    .build();

    match built {
        Ok(window) => NewWindowResponse::Create { window },
        Err(e) => {
            eprintln!("popup for {url}: {e}; falling back to the default popup");
            NewWindowResponse::Allow
        }
    }
}

/// `window.close()` inside a popup is a no-op in wry (no `webViewDidClose:`), so close
/// any open popups once the vendor webview starts loading a new page, which is what
/// happens when the login completes.
fn close_popups(app: &AppHandle) {
    for (label, popup) in app.webview_windows() {
        if label.starts_with(POPUP_PREFIX) {
            let _ = popup.close();
        }
    }
}

#[tauri::command]
fn show_vendor(app: AppHandle, id: String, url: String) -> Result<(), String> {
    let window = app.get_window("main").ok_or("main window not found")?;
    let label = format!("{VENDOR_PREFIX}{id}");

    for (other, webview) in app.webviews() {
        if other.starts_with(VENDOR_PREFIX) && other != label {
            webview.hide().map_err(|e| e.to_string())?;
        }
    }

    if let Some(existing) = app.get_webview(&label) {
        return existing.show().map_err(|e| e.to_string());
    }

    let parsed = url.parse().map_err(|e| format!("bad url {url}: {e}"))?;
    let size = content_size(&window).map_err(|e| e.to_string())?;
    let popup_app = app.clone();
    window
        .add_child(
            WebviewBuilder::new(&label, WebviewUrl::External(parsed))
                .user_agent(vendor_user_agent())
                .on_new_window(move |url, features| open_popup(&popup_app, url, features))
                .on_page_load(|webview, payload| {
                    if matches!(payload.event(), PageLoadEvent::Started) {
                        close_popups(webview.app_handle());
                    }
                }),
            content_origin(),
            size,
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------------
// Balance scraping
// ---------------------------------------------------------------------------------

#[derive(Deserialize, Clone)]
struct ScrapeTarget {
    id: String,
    url: String,
}

/// One vendor's latest balance reading, as shown in the sidebar.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Balance {
    id: String,
    /// "loading" | "ok" | "login" | "notfound" | "unsupported" | "error"
    status: String,
    value: Option<String>,
    detail: Option<String>,
    url: Option<String>,
    /// Unix seconds of the last successful reading.
    updated: Option<u64>,
}

#[derive(Default)]
struct Balances(Mutex<HashMap<String, Balance>>);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Store a reading and push it to the sidebar. Keeps the last good value when a
/// refresh fails, so a flaky page doesn't blank the balance.
fn publish(app: &AppHandle, mut b: Balance) {
    let state = app.state::<Balances>();
    let mut map = state.0.lock().unwrap();
    if b.status != "ok" {
        if let Some(prev) = map.get(&b.id) {
            if b.value.is_none() {
                b.value = prev.value.clone();
                b.updated = prev.updated;
            }
        }
    }
    map.insert(b.id.clone(), b.clone());
    drop(map);
    if let (Some(sidebar), Ok(json)) = (app.get_webview("sidebar"), serde_json::to_string(&b)) {
        let _ = sidebar.eval(format!("window.onBalance && window.onBalance({json})"));
    }
}

/// Run `js` (which must evaluate to a string) in `webview` and wait for the result.
fn eval_string(webview: &Webview, js: &str, timeout: Duration) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    webview
        .eval_with_callback(js, move |raw| {
            let _ = tx.send(raw);
        })
        .ok()?;
    let raw = rx.recv_timeout(timeout).ok()?;
    // The callback hands back the JSON encoding of the JS value: a quoted string here.
    serde_json::from_str::<String>(&raw).ok()
}

fn scrape_one(app: AppHandle, webview: Webview, id: String) {
    let started = Instant::now();
    let mut last = Balance { id: id.clone(), status: "notfound".into(), ..Default::default() };
    let mut reloads = 0;
    while started.elapsed() < SCRAPE_TIMEOUT {
        std::thread::sleep(SCRAPE_POLL);
        // Single-page consoles occasionally stall on first boot (seen with Anthropic:
        // title set, empty body, no API calls). Reloading usually clears it.
        if reloads < SCRAPE_RELOAD_AT.len() && started.elapsed() > SCRAPE_RELOAD_AT[reloads] {
            reloads += 1;
            let _ = webview.eval("location.reload()");
            continue;
        }
        let Some(raw) = eval_string(&webview, "window.__ch ? window.__ch.result() : ''", Duration::from_secs(5)) else {
            continue;
        };
        let Ok(mut r) = serde_json::from_str::<Balance>(&raw) else { continue };
        r.id = id.clone();
        last = r;
        match last.status.as_str() {
            "ok" => {
                last.updated = Some(now_secs());
                break;
            }
            // Give a redirect-to-login a few seconds to settle before giving up.
            "login" | "unsupported" if started.elapsed() > Duration::from_secs(8) => break,
            _ => {}
        }
    }
    if last.status == "pending" {
        last.status = "notfound".into();
    }

    if let Ok(dir) = std::env::var("CONSOLEHUB_DUMP_DIR") {
        if let Some(dump) = eval_string(&webview, "window.__ch ? window.__ch.dump() : ''", Duration::from_secs(5)) {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(format!("{dir}/{id}.json"), dump);
        }
        let summary = serde_json::json!({ "elapsed_ms": started.elapsed().as_millis() as u64, "result": &last });
        let _ = std::fs::write(format!("{dir}/{id}.result.json"), summary.to_string());
        // What the sidebar shows, after this reading is pushed (the last vendor to finish wins).
        let app2 = app.clone();
        let dir2 = dir.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let Some(sidebar) = app2.get_webview("sidebar") else { return };
            let js = "JSON.stringify({w: innerWidth, h: innerHeight, items: [...document.querySelectorAll('nav button')].map(e => e.className + ' | ' + e.innerText.replace(/\\n/g, ' '))})";
            if let Some(state) = eval_string(&sidebar, js, Duration::from_secs(5)) {
                let state = format!("{state}\ntop_inset={}", top_inset());
                let vendors: Vec<String> = app2.webviews().into_iter()
                    .filter(|(l, _)| l.starts_with(VENDOR_PREFIX) || l == "sidebar")
                    .map(|(l, w)| format!("{l} pos={:?} size={:?}", w.position().ok(), w.size().ok()))
                    .collect();
                let _ = std::fs::write(format!("{dir2}/sidebar.json"), format!("{state}\n{vendors:?}"));
            }
        });
    }

    publish(&app, last);
    let _ = webview.close();
}

#[tauri::command]
fn get_balances(state: State<Balances>) -> Vec<Balance> {
    state.0.lock().unwrap().values().cloned().collect()
}

/// Load each vendor's billing page in an off-screen webview (sharing the login cookies
/// of the visible consoles), read the balance, then close it.
#[tauri::command]
fn refresh_balances(app: AppHandle, targets: Vec<ScrapeTarget>) -> Result<(), String> {
    let window = app.get_window("main").ok_or("main window not found")?;
    for t in targets {
        let label = format!("{SCRAPER_PREFIX}{}", t.id);
        if app.get_webview(&label).is_some() {
            continue; // a scrape for this vendor is still running
        }
        let Ok(url) = t.url.parse::<Url>() else {
            publish(&app, Balance { id: t.id, status: "error".into(), detail: Some("bad url".into()), ..Default::default() });
            continue;
        };
        let debug = std::env::var_os("CONSOLEHUB_DUMP_DIR").is_some();
        let init = format!(
            "window.__chVendor = {};\nwindow.__chDebug = {debug};\n{SCRAPE_JS}",
            serde_json::to_string(&t.id).unwrap()
        );
        // Off-screen rather than hidden, so the page doesn't think it's in a background tab.
        let built = window.add_child(
            WebviewBuilder::new(&label, WebviewUrl::External(url))
                .user_agent(vendor_user_agent())
                .initialization_script(&init)
                .on_new_window(|_, _| NewWindowResponse::Deny),
            LogicalPosition::new(-20000.0, -20000.0),
            LogicalSize::new(1280.0, 900.0),
        );
        match built {
            Ok(webview) => {
                publish(&app, Balance { id: t.id.clone(), status: "loading".into(), ..Default::default() });
                let app2 = app.clone();
                std::thread::spawn(move || scrape_one(app2, webview, t.id));
            }
            Err(e) => publish(&app, Balance { id: t.id, status: "error".into(), detail: Some(e.to_string()), ..Default::default() }),
        }
    }
    Ok(())
}

/// Open a URL in the user's default browser. Used for vendors whose auth
/// flow doesn't work inside an embedded webview (Anthropic/WorkOS, …).
#[tauri::command]
fn open_in_browser(url: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "cmd";
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let program = "xdg-open";

    #[cfg(target_os = "windows")]
    let args: Vec<&str> = vec!["/C", "start", "", &url];
    #[cfg(not(target_os = "windows"))]
    let args: Vec<&str> = vec![&url];

    Command::new(program)
        .args(&args)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Right-click menu for a sidebar vendor. Shown at the cursor; the click is handled
/// in `on_menu_event`.
#[tauri::command]
fn vendor_menu(app: AppHandle, window: tauri::Window, url: String) -> Result<(), String> {
    let copy = MenuItem::with_id(&app, format!("{COPY_LINK_PREFIX}{url}"), "Copy Link", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&app, &[&copy]).map_err(|e| e.to_string())?;
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

fn on_menu_event(_app: &AppHandle, event: MenuEvent) {
    if let Some(url) = event.id().as_ref().strip_prefix(COPY_LINK_PREFIX) {
        if let Err(e) = copy_to_clipboard(url) {
            eprintln!("copy link: {e}");
        }
    }
}

fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    let mut child = Command::new("/usr/bin/pbcopy").stdin(Stdio::piped()).spawn()?;
    child.stdin.take().expect("piped stdin").write_all(text.as_bytes())?;
    child.wait().map(|_| ())
}

fn main() {
    tauri::Builder::default()
        .manage(Balances::default())
        .on_menu_event(on_menu_event)
        .invoke_handler(tauri::generate_handler![
            show_vendor,
            open_in_browser,
            get_balances,
            refresh_balances,
            vendor_menu,
        ])
        .setup(|app| {
            let window = tauri::window::WindowBuilder::new(app, "main")
                .title("Console Hub")
                .inner_size(1280.0, 820.0)
                .build()?;

            let full = window_size(&window)?;
            window.add_child(
                WebviewBuilder::new("sidebar", WebviewUrl::App("index.html".into())).on_page_load(
                    |webview, payload| {
                        if matches!(payload.event(), PageLoadEvent::Finished) {
                            let app = webview.app_handle().clone();
                            std::thread::spawn(move || {
                                // WebKit applies the inset shortly after load; check twice.
                                for delay in [100, 600] {
                                    std::thread::sleep(Duration::from_millis(delay));
                                    measure_top_inset(&app);
                                }
                            });
                        }
                    },
                ),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(SIDEBAR_WIDTH, full.height),
            )?;

            let handle = app.handle().clone();
            window.on_window_event(move |event| {
                if matches!(event, WindowEvent::Resized(_)) {
                    layout(&handle);
                    // Full screen hides the title bar, which changes the inset.
                    let app = handle.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(250));
                        measure_top_inset(&app);
                    });
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Console Hub");
}
