#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod cli;
mod providers;
mod settings;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use providers::{Catalog, Extracted, Provider};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

use tauri::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent, WebviewBuilder},
    AppHandle, LogicalPosition, LogicalSize, Manager, State, Url, Webview, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};

const SIDEBAR_WIDTH: f64 = 248.0;
const VENDOR_PREFIX: &str = "vendor-";
// Hidden background webviews that load each vendor's billing page to read the balance.
const SCRAPER_PREFIX: &str = "scrape-";
const SCRAPE_JS: &str = include_str!("scrape.js");
const SCRAPE_TIMEOUT: Duration = Duration::from_secs(45);
const SCRAPE_POLL: Duration = Duration::from_millis(1500);
const SCRAPE_RELOAD_AT: [Duration; 2] = [Duration::from_secs(15), Duration::from_secs(30)];
const POPUP_PREFIX: &str = "popup-";
// Sidebar right-click menu items; the id carries the URL to copy or the provider id.
const COPY_LINK_PREFIX: &str = "copy-link:";
const REMOVE_PROVIDER_PREFIX: &str = "remove-provider:";
const SHOW_PROVIDERS_FOLDER: &str = "show-providers-folder";
static POPUP_COUNTER: AtomicU32 = AtomicU32::new(0);
// User agent for vendor webviews and login popups on macOS: the real Safari UA for the
// Safari installed on this Mac. Windows (WebView2, Chromium) and Linux (WebKitGTK) keep
// their webview's own UA, which already matches the engine.
// - The default WKWebView UA lacks the `Version/… Safari/…` tokens, and some logins
//   (Anthropic via WorkOS) reject it.
// - Claiming to be Chrome made Google sign-in fail with "This browser or app may not be
//   secure": the page runs on WebKit, so a Chrome UA is an obvious mismatch.
// Safari's UA always reports macOS as 10_15_7; only the Version token changes.
const SAFARI_VERSION_FALLBACK: &str = "26.0";

#[cfg(target_os = "macos")]
fn vendor_user_agent() -> Option<&'static str> {
    static UA: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    Some(UA.get_or_init(|| {
        let version = std::process::Command::new("/usr/bin/defaults")
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
    }))
}

#[cfg(not(target_os = "macos"))]
fn vendor_user_agent() -> Option<&'static str> {
    None
}

/// Sets `vendor_user_agent()` on a webview builder, when there is one.
trait WithVendorUserAgent: Sized {
    fn with_vendor_user_agent(self) -> Self;
}

impl<R: tauri::Runtime> WithVendorUserAgent for WebviewBuilder<R> {
    fn with_vendor_user_agent(self) -> Self {
        match vendor_user_agent() {
            Some(ua) => self.user_agent(ua),
            None => self,
        }
    }
}

impl<'a, R: tauri::Runtime, M: Manager<R>> WithVendorUserAgent for WebviewWindowBuilder<'a, R, M> {
    fn with_vendor_user_agent(self) -> Self {
        match vendor_user_agent() {
            Some(ua) => self.user_agent(ua),
            None => self,
        }
    }
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
    .with_vendor_user_agent()
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
                .with_vendor_user_agent()
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
// Providers and balance scraping
// ---------------------------------------------------------------------------------

/// The user's providers (see providers::catalog). Rebuilt when the user allows, adds or
/// removes one.
#[derive(Default)]
struct Providers(Mutex<Catalog>);

/// The user's providers folder. None are built in; the repo's `samples/` has some to add.
/// It follows the bundle identifier, so unlike logins the dev build and the installed
/// app share it.
fn user_providers_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap_or_default().join("providers")
}

fn settings_path(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().unwrap_or_default().join("settings.json")
}

/// Rebuild the provider list from disk and the user's approvals.
fn reload_providers(app: &AppHandle) {
    let approved = settings::load(&settings_path(app)).approved;
    let catalog = providers::catalog(&user_providers_dir(app), &approved);
    for e in &catalog.errors {
        eprintln!("provider {e}");
    }
    *app.state::<Providers>().0.lock().unwrap() = catalog;
}

/// Ask the sidebar to list the providers again.
fn notify_providers_changed(app: &AppHandle) {
    if let Some(sidebar) = app.get_webview("sidebar") {
        let _ = sidebar.eval("window.onProvidersChanged && window.onProvidersChanged()");
    }
}

/// Forget what was shown for `id` before it was replaced, allowed or removed: its stored
/// reading, and its console webview, which may be on the old provider's URL. The sidebar
/// reopens the console (show_vendor creates it again).
fn forget_provider(app: &AppHandle, id: &str) {
    app.state::<Balances>().0.lock().unwrap().remove(id);
    if let Some(webview) = app.get_webview(&format!("{VENDOR_PREFIX}{id}")) {
        let _ = webview.close();
    }
}

fn tilde(path: &Path) -> String {
    let s = path.display().to_string();
    match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        Ok(home) if !home.is_empty() && s.starts_with(&home) => format!("~{}", &s[home.len()..]),
        _ => s,
    }
}

/// What the sidebar needs to list a provider.
#[derive(Serialize)]
struct ProviderInfo {
    id: String,
    name: String,
    url: String,
    external: bool,
    balance: bool,
    /// Not allowed yet, or changed since: click to review.
    review: bool,
}

#[derive(Serialize)]
struct ProviderList {
    providers: Vec<ProviderInfo>,
    errors: Vec<String>,
}

#[tauri::command]
fn list_providers(state: State<Providers>) -> ProviderList {
    let c = state.0.lock().unwrap();
    let info = |p: &Provider, review: bool| ProviderInfo {
        id: p.manifest.id.clone(),
        name: p.manifest.name.clone(),
        url: p.manifest.url.clone(),
        external: p.manifest.external,
        balance: !review && p.metric().is_some() && !p.manifest.external,
        review,
    };
    let mut providers: Vec<ProviderInfo> =
        c.active.iter().map(|p| info(p, false)).chain(c.pending.iter().map(|p| info(p, true))).collect();
    providers.sort_by_key(|p| p.name.to_lowercase());
    ProviderList { providers, errors: c.errors.clone() }
}

/// What a provider will do, for the approval dialog. URLs are shown in their parsed form,
/// a second guard against odd characters (validation refuses them).
fn describe(p: &Provider) -> Vec<String> {
    let m = &p.manifest;
    let shown = |url: &str| Url::parse(url).map(|u| u.to_string()).unwrap_or_else(|_| url.to_string());
    let mut lines = vec![match &m.author {
        Some(author) => format!("{} \u{2014} by {author} (not verified)", m.name),
        None => m.name.clone(),
    }];
    lines.push(format!("Console: {}", shown(&m.url)));
    match p.metric() {
        Some(metric) => {
            lines.push(format!("Loads {} in the background, signed in as you,", shown(&metric.page)));
            lines.push(format!("and reads responses from {}.", m.domains.join(", ")));
        }
        None => lines.push("Reads nothing in the background.".into()),
    }
    lines
}

/// Ask to allow `providers`, listing what each will do. `notes` go above the list.
fn ask_to_allow(app: &AppHandle, providers: &[Provider], notes: &[String]) -> bool {
    let mut lines: Vec<String> = notes.to_vec();
    for p in providers {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(describe(p));
    }
    lines.push(String::new());
    lines.push("Provider code runs in a sandbox with no network access. Only allow providers from people you trust.".into());
    let title = match providers {
        [p] => format!("Allow the provider \u{201c}{}\u{201d}?", p.manifest.name),
        _ => format!("Allow these {} providers?", providers.len()),
    };
    app.dialog()
        .message(lines.join("\n"))
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Allow".into(), "Don't Allow".into()))
        .blocking_show()
}

/// Record that the user allowed exactly these contents.
fn approve(app: &AppHandle, providers: &[Provider]) -> Result<(), String> {
    let path = settings_path(app);
    let mut s = settings::load(&path);
    for p in providers {
        s.approved.insert(p.manifest.id.clone(), p.hash.clone());
    }
    settings::save(&path, &s)
}

/// Ask the user to allow a provider that's in their folder but not allowed (or changed
/// since). Allowing stores the hash of exactly what was shown, so a file changed while
/// the dialog was open stays waiting.
#[tauri::command]
async fn review_provider(app: AppHandle, id: String) -> Result<bool, String> {
    let p = {
        let c = app.state::<Providers>();
        let c = c.0.lock().unwrap();
        c.pending.iter().find(|p| p.manifest.id == id).cloned()
    };
    let p = p.ok_or_else(|| format!("{id} is not waiting for review"))?;
    let mut notes = vec![format!("In {}", tilde(&p.dir))];
    if settings::load(&settings_path(&app)).approved.contains_key(&id) {
        notes.push("It changed since you allowed it.".into());
    }
    if !ask_to_allow(&app, std::slice::from_ref(&p), &notes) {
        return Ok(false);
    }
    approve(&app, std::slice::from_ref(&p))?;
    forget_provider(&app, &id);
    reload_providers(&app);
    Ok(true)
}

/// Pick a provider folder, or a folder of them such as the repo's `samples/`, check them,
/// and ask once to allow them all. Only then are they copied into the user's folder.
/// Returns the ids added (empty if the user cancelled).
#[tauri::command]
async fn add_provider(app: AppHandle) -> Result<Vec<String>, String> {
    let Some(picked) = app
        .dialog()
        .file()
        .set_title("Choose a provider folder, or a folder of them")
        .blocking_pick_folder()
    else {
        return Ok(Vec::new());
    };
    let folder = picked.into_path().map_err(|e| e.to_string())?;
    let (found, errors) = providers::inspect_all(&folder);
    if found.is_empty() {
        return Err(format!("{}: {}", tilde(&folder), errors.join("; ")));
    }
    let user_dir = user_providers_dir(&app);
    let mut notes = vec![format!("From {}", tilde(&folder))];
    let replacing: Vec<&str> = found.iter().filter(|p| user_dir.join(&p.manifest.id).exists()).map(|p| p.manifest.name.as_str()).collect();
    if !replacing.is_empty() {
        notes.push(format!("Replaces your current copy of {}.", replacing.join(", ")));
    }
    if !errors.is_empty() {
        notes.push(format!("Skipped, with problems: {}", errors.join("; ")));
    }
    if !ask_to_allow(&app, &found, &notes) {
        return Ok(Vec::new());
    }
    let mut installed = Vec::new();
    for p in &found {
        installed.push(providers::install(p, &user_dir)?);
    }
    approve(&app, &installed)?;
    for p in &installed {
        forget_provider(&app, &p.manifest.id);
    }
    reload_providers(&app);
    Ok(installed.into_iter().map(|p| p.manifest.id).collect())
}

/// Delete a provider from the user's folder, after asking. Runs off the main thread
/// (from the sidebar's right-click menu), since the dialog blocks.
fn remove_provider(app: &AppHandle, id: &str) -> Result<(), String> {
    let found = {
        let c = app.state::<Providers>();
        let c = c.0.lock().unwrap();
        c.active.iter().chain(c.pending.iter()).find(|p| p.manifest.id == id).cloned()
    };
    let p = found.ok_or_else(|| format!("{id} is not in your providers folder"))?;
    let remove = app
        .dialog()
        .message(format!("This deletes {}.", tilde(&p.dir)))
        .title(format!("Remove the provider \u{201c}{}\u{201d}?", p.manifest.name))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Remove".into(), "Cancel".into()))
        .blocking_show();
    if !remove {
        return Ok(());
    }
    std::fs::remove_dir_all(&p.dir).map_err(|e| format!("removing {}: {e}", tilde(&p.dir)))?;
    let path = settings_path(app);
    let mut s = settings::load(&path);
    if s.approved.remove(id).is_some() {
        settings::save(&path, &s)?;
    }
    forget_provider(app, id);
    reload_providers(app);
    notify_providers_changed(app);
    Ok(())
}

/// One provider's latest balance reading, as shown in the sidebar.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Balance {
    id: String,
    /// "loading" | "ok" | "login" | "notfound" | "error"
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

/// What `window.__ch.result()` in scrape.js reports.
#[derive(Deserialize)]
struct PageState {
    /// "login" | "loaded"
    status: String,
    url: Option<String>,
    #[serde(default)]
    responses: Vec<providers::Response>,
}

fn scrape_one(app: AppHandle, webview: Webview, provider: Provider) {
    let id = provider.manifest.id.clone();
    let Some(metric) = provider.metric() else { return };
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
        let Ok(page) = serde_json::from_str::<PageState>(&raw) else { continue };
        last.url = page.url;
        if page.status == "login" {
            last.status = "login".into();
            last.detail = None;
            // Give a redirect-to-login a few seconds to settle before giving up.
            if started.elapsed() > Duration::from_secs(8) {
                break;
            }
            continue;
        }
        // An error may clear up as more responses arrive, so keep polling; the last
        // one is reported if the scrape times out.
        match providers::extract(&provider, metric, &page.responses) {
            Ok(Extracted::Ok { value, detail }) => {
                last = Balance { id: id.clone(), status: "ok".into(), value: Some(value), detail, url: last.url, updated: Some(now_secs()) };
                break;
            }
            // Say which file to fix: a capture pattern that no longer matches (the API moved)
            // or extract.js that no longer finds the balance (the response changed).
            Ok(Extracted::Missing(names)) => {
                last.status = "notfound".into();
                let wanted: Vec<String> = names.iter().map(|n| format!("{n}: {}", metric.captures[n])).collect();
                last.detail = Some(format!(
                    "The page never fetched the response this provider reads ({}). The console's API may have moved; the provider's manifest.json needs updating.",
                    wanted.join(", ")
                ));
            }
            Ok(Extracted::Empty) => {
                last.status = "notfound".into();
                last.detail = Some("The response arrived but no balance came out of it. Its format may have changed; the provider's extract.js needs updating.".into());
            }
            Err(e) => {
                last.status = "notfound".into();
                last.detail = Some(format!("Provider error: {e}"));
            }
        }
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

/// Load each provider's balance page (or just `only`'s) in an off-screen webview (sharing
/// the login cookies of the visible consoles), read the balance, then close it.
#[tauri::command]
fn refresh_balances(app: AppHandle, only: Option<String>) -> Result<(), String> {
    let window = app.get_window("main").ok_or("main window not found")?;
    let debug = std::env::var_os("CONSOLEHUB_DUMP_DIR").is_some();
    // Copied out so the lock isn't held while webviews are created.
    let list: Vec<Provider> = app.state::<Providers>().0.lock().unwrap().active.clone();
    let wanted = |p: &&Provider| !p.manifest.external && only.as_ref().is_none_or(|id| *id == p.manifest.id);
    for p in list.iter().filter(wanted) {
        let Some(metric) = p.metric() else { continue };
        let id = p.manifest.id.clone();
        let label = format!("{SCRAPER_PREFIX}{id}");
        if app.get_webview(&label).is_some() {
            continue; // a scrape for this provider is still running
        }
        // Validated when the provider loaded.
        let Ok(url) = metric.page.parse::<Url>() else { continue };
        let config = serde_json::json!({
            "captures": metric.captures.values().collect::<Vec<_>>(),
            "domains": &p.manifest.domains,
            "debug": debug,
        });
        let init = format!("window.__chConfig = {config};\n{SCRAPE_JS}");
        // Off-screen rather than hidden, so the page doesn't think it's in a background tab.
        let built = window.add_child(
            WebviewBuilder::new(&label, WebviewUrl::External(url))
                .with_vendor_user_agent()
                .initialization_script(&init)
                .on_new_window(|_, _| NewWindowResponse::Deny),
            LogicalPosition::new(-20000.0, -20000.0),
            LogicalSize::new(1280.0, 900.0),
        );
        match built {
            Ok(webview) => {
                publish(&app, Balance { id, status: "loading".into(), ..Default::default() });
                let (app2, provider) = (app.clone(), p.clone());
                std::thread::spawn(move || scrape_one(app2, webview, provider));
            }
            Err(e) => publish(&app, Balance { id, status: "error".into(), detail: Some(e.to_string()), ..Default::default() }),
        }
    }
    Ok(())
}

/// Open a provider's console in the default browser (`"external": true`). Only https
/// URLs, passed in parsed form; the opener plugin hands them to the OS without a shell
/// (an earlier `cmd /C start` version let `&` in a URL run commands on Windows).
#[tauri::command]
fn open_in_browser(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|e| format!("{url}: {e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!("{url}: only https links open in the browser"));
    }
    app.opener().open_url(parsed.as_str(), None::<&str>).map_err(|e| e.to_string())
}

/// Right-click menu for a sidebar provider. Shown at the cursor; the click is handled
/// in `on_menu_event`.
#[tauri::command]
fn vendor_menu(app: AppHandle, window: tauri::Window, id: String, url: String) -> Result<(), String> {
    let copy = MenuItem::with_id(&app, format!("{COPY_LINK_PREFIX}{url}"), "Copy Link", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let separator = PredefinedMenuItem::separator(&app).map_err(|e| e.to_string())?;
    let remove = MenuItem::with_id(&app, format!("{REMOVE_PROVIDER_PREFIX}{id}"), "Remove Provider\u{2026}", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let folder = MenuItem::with_id(&app, SHOW_PROVIDERS_FOLDER, "Show Providers Folder", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&app, &[&copy, &separator, &remove, &folder]).map_err(|e| e.to_string())?;
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    if let Some(url) = id.strip_prefix(COPY_LINK_PREFIX) {
        if let Err(e) = app.clipboard().write_text(url) {
            eprintln!("copy link: {e}");
        }
    } else if id == SHOW_PROVIDERS_FOLDER {
        if let Err(e) = show_providers_folder(app) {
            eprintln!("show providers folder: {e}");
        }
    } else if let Some(provider) = id.strip_prefix(REMOVE_PROVIDER_PREFIX) {
        let (app, provider) = (app.clone(), provider.to_string());
        std::thread::spawn(move || {
            if let Err(e) = remove_provider(&app, &provider) {
                eprintln!("remove provider: {e}");
            }
        });
    }
}

/// Open the user's providers folder in the file manager, creating it on a fresh install.
fn show_providers_folder(app: &AppHandle) -> Result<(), String> {
    let dir = user_providers_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

fn main() {
    // Provider author tools: cli.rs (no window) and capture.rs.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let context = tauri::generate_context!();
    match args.first().map(String::as_str) {
        Some("test-provider") => std::process::exit(cli::test_provider(&args[1..])),
        Some("fixture") => std::process::exit(cli::fixture(&args[1..])),
        Some("capture") => match capture::parse(&args[1..]) {
            Ok(target) => return capture::run(context, target),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(2);
            }
        },
        _ => {}
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(Balances::default())
        .manage(Providers::default())
        .on_menu_event(on_menu_event)
        .invoke_handler(tauri::generate_handler![
            show_vendor,
            open_in_browser,
            list_providers,
            review_provider,
            add_provider,
            get_balances,
            refresh_balances,
            vendor_menu,
        ])
        .setup(|app| {
            reload_providers(app.handle());

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
        .run(context)
        .expect("error while running Console Hub");
}
