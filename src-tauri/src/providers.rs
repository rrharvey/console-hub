//! Provider plugins. Each provider is a folder in `providers/`:
//!
//! - `manifest.json`: name, console URL, the domains it may touch, and for its balance
//!   the page to load and the URL patterns of the JSON responses to capture.
//! - `extract.js`: a function named after the metric that turns those responses into
//!   `{ value, detail? }`.
//!
//! Provider code only runs here, in a fresh QuickJS sandbox per call: no network, no DOM,
//! no Tauri commands, with time and memory limits. The scraper page (scrape.js) only
//! records the responses the manifest asks for.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, CaughtError, Context, Function, Object, Runtime, Value};
use serde::{Deserialize, Serialize};
use tauri::Url;

pub const API_VERSION: u32 = 1;
const SANDBOX_JS: &str = include_str!("sandbox.js");
const SANDBOX_TIMEOUT: Duration = Duration::from_millis(500);
const SANDBOX_MEMORY: usize = 32 << 20;
const MAX_NAME_CHARS: usize = 40;
const MAX_VALUE_CHARS: usize = 40;
const MAX_DETAIL_CHARS: usize = 1000;

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    /// The console shown when the provider is selected.
    pub url: String,
    /// Opens the console in the default browser instead of an embedded webview.
    #[serde(default)]
    pub external: bool,
    /// Every page and captured response must be on one of these hosts or their subdomains.
    pub domains: Vec<String>,
    /// At most one in apiVersion 1: the balance shown in the sidebar.
    #[serde(default)]
    pub metrics: Vec<Metric>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    /// Also the name of the function in extract.js.
    pub key: String,
    /// The page loaded in the background to read the metric.
    pub page: String,
    /// Capture name -> JS regex matched against response URLs.
    pub captures: BTreeMap<String, String>,
    /// Captures that must arrive before the function runs. Defaults to all of them.
    #[serde(default)]
    pub required: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct Provider {
    pub manifest: Manifest,
    pub script: String,
}

impl Provider {
    pub fn metric(&self) -> Option<&Metric> {
        self.manifest.metrics.first()
    }
}

/// A JSON response the scraper page captured.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Response {
    pub url: String,
    pub body: serde_json::Value,
}

#[derive(Debug, PartialEq)]
pub enum Extracted {
    /// Waiting for responses, or the function returned null.
    Pending,
    Ok { value: String, detail: Option<String> },
}

/// Load every provider folder in `dir`, sorted by name. A provider that fails to load is
/// left out and reported in the error list; it never stops the others.
pub fn load_dir(dir: &Path) -> (Vec<Provider>, Vec<String>) {
    let mut providers = Vec::new();
    let mut errors = Vec::new();
    let mut folders: Vec<_> = match std::fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect(),
        Err(e) => return (providers, vec![format!("{}: {e}", dir.display())]),
    };
    folders.sort();
    for folder in folders {
        let name = folder.file_name().unwrap_or_default().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with('_') {
            continue;
        }
        match load(&folder) {
            Ok(p) => providers.push(p),
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    providers.sort_by_key(|p| p.manifest.name.to_lowercase());
    (providers, errors)
}

fn load(folder: &Path) -> Result<Provider, String> {
    let raw = std::fs::read_to_string(folder.join("manifest.json")).map_err(|e| format!("manifest.json: {e}"))?;
    let mut manifest: Manifest = serde_json::from_str(&raw).map_err(|e| format!("manifest.json: {e}"))?;
    let script = match std::fs::read_to_string(folder.join("extract.js")) {
        Ok(s) => s,
        Err(_) if manifest.metrics.is_empty() => String::new(),
        Err(e) => return Err(format!("extract.js: {e}")),
    };
    let folder_name = folder.file_name().unwrap_or_default().to_string_lossy();
    validate(&mut manifest, &folder_name)?;
    let provider = Provider { manifest, script };
    if let Some(metric) = provider.metric() {
        let json = serde_json::to_string(metric).map_err(|e| e.to_string())?;
        sandbox(&provider.script, |api| api.get::<_, Function>("check")?.call::<_, ()>((json,)))?;
    }
    Ok(provider)
}

/// Checks that don't need the JS engine. Fills in `required` when it's left out.
fn validate(m: &mut Manifest, folder: &str) -> Result<(), String> {
    if m.api_version != API_VERSION {
        return Err(format!("apiVersion {} is not supported (this app reads {API_VERSION})", m.api_version));
    }
    if !is_slug(&m.id) || m.id != folder {
        return Err(format!("id \"{}\" must be lowercase letters, digits and dashes, and match the folder name", m.id));
    }
    if m.name.trim().is_empty() || m.name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("name must be 1 to {MAX_NAME_CHARS} characters"));
    }
    if m.domains.is_empty() {
        return Err("domains must list at least one host".into());
    }
    for d in &m.domains {
        check_domain(d)?;
    }
    check_url("url", &m.url, &m.domains)?;
    if m.metrics.len() > 1 {
        return Err(format!("apiVersion {API_VERSION} allows at most one metric"));
    }
    for metric in &mut m.metrics {
        if !is_identifier(&metric.key) {
            return Err(format!("metric key \"{}\" must be a JS identifier", metric.key));
        }
        check_url("page", &metric.page, &m.domains)?;
        if metric.captures.is_empty() {
            return Err("captures must name at least one response".into());
        }
        if let Some(bad) = metric.captures.keys().find(|k| !is_identifier(k)) {
            return Err(format!("capture name \"{bad}\" must be a JS identifier"));
        }
        let required = metric.required.get_or_insert_with(|| metric.captures.keys().cloned().collect());
        if let Some(bad) = required.iter().find(|r| !metric.captures.contains_key(*r)) {
            return Err(format!("required capture \"{bad}\" is not in captures"));
        }
    }
    Ok(())
}

/// A registrable host such as `openai.com` or `app.tavily.com`. Suffix matching makes it
/// cover subdomains, so single labels (`com`), IP addresses and `localhost` are refused.
fn check_domain(d: &str) -> Result<(), String> {
    let labels: Vec<&str> = d.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty() && !l.starts_with('-') && l.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    let tld_is_numeric = labels.last().is_some_and(|l| l.chars().all(|c| c.is_ascii_digit()));
    if labels.len() < 2 || !labels.iter().all(label_ok) || tld_is_numeric || d.ends_with(".localhost") {
        return Err(format!("domain \"{d}\" must be a lowercase host name like example.com"));
    }
    Ok(())
}

fn check_url(field: &str, url: &str, domains: &[String]) -> Result<(), String> {
    let parsed = Url::parse(url).map_err(|e| format!("{field} \"{url}\": {e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!("{field} \"{url}\" must use https"));
    }
    if !parsed.host_str().is_some_and(|h| host_allowed(h, domains)) {
        return Err(format!("{field} \"{url}\" is not on one of the provider's domains"));
    }
    Ok(())
}

pub fn host_allowed(host: &str, domains: &[String]) -> bool {
    domains.iter().any(|d| host == d || host.strip_suffix(d.as_str()).is_some_and(|rest| rest.ends_with('.')))
}

fn is_slug(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_identifier(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Run the provider's function for `metric` over the captured responses. Responses from
/// hosts outside the provider's domains are dropped first.
pub fn extract(provider: &Provider, metric: &Metric, responses: &[Response]) -> Result<Extracted, String> {
    let domains = &provider.manifest.domains;
    let allowed: Vec<&Response> = responses
        .iter()
        .filter(|r| Url::parse(&r.url).ok().and_then(|u| u.host_str().map(|h| host_allowed(h, domains))).unwrap_or(false))
        .collect();
    let metric_json = serde_json::to_string(metric).map_err(|e| e.to_string())?;
    let responses_json = serde_json::to_string(&allowed).map_err(|e| e.to_string())?;
    let out: String = sandbox(&provider.script, |api| {
        api.get::<_, Function>("run")?.call((metric_json, responses_json))
    })?;

    #[derive(Deserialize)]
    struct Out {
        status: String,
        value: Option<serde_json::Value>,
        detail: Option<serde_json::Value>,
    }
    let out: Out = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    if out.status != "ok" {
        return Ok(Extracted::Pending);
    }
    let value = match out.value {
        Some(serde_json::Value::String(v)) if !v.trim().is_empty() && v.chars().count() <= MAX_VALUE_CHARS => v,
        _ => return Err(format!("{}() must return value as a string of 1 to {MAX_VALUE_CHARS} characters", metric.key)),
    };
    let detail = match out.detail {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(d)) if d.chars().count() <= MAX_DETAIL_CHARS => Some(d),
        _ => return Err(format!("{}() must return detail as a string of at most {MAX_DETAIL_CHARS} characters", metric.key)),
    };
    Ok(Extracted::Ok { value, detail })
}

/// Evaluate sandbox.js and then the provider script in a fresh QuickJS runtime, and call
/// `f` with the entry points sandbox.js returned.
fn sandbox<T>(script: &str, f: impl for<'js> FnOnce(Object<'js>) -> rquickjs::Result<T>) -> Result<T, String> {
    let rt = Runtime::new().map_err(|e| e.to_string())?;
    rt.set_memory_limit(SANDBOX_MEMORY);
    let deadline = Instant::now() + SANDBOX_TIMEOUT;
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
    ctx.with(|ctx| {
        let api: Object = ctx.eval(SANDBOX_JS).catch(&ctx).map_err(|e| format!("sandbox.js: {e}"))?;
        ctx.eval::<Value, _>(script).catch(&ctx).map_err(|e| format!("extract.js: {}", js_error(e)))?;
        f(api).catch(&ctx).map_err(js_error)
    })
}

fn js_error(e: CaughtError) -> String {
    let s = e.to_string();
    // Exceptions print with their stack; the first line is the message.
    let first = s.lines().next().unwrap_or_default().trim();
    if first.is_empty() { s.trim().to_string() } else { first.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn providers_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../providers")
    }

    fn provider(manifest: serde_json::Value, script: &str) -> Result<Provider, String> {
        let dir = std::env::temp_dir().join(format!("ch-test-{}-{}", std::process::id(), rand_suffix()));
        let id = manifest["id"].as_str().unwrap_or("x").to_string();
        let folder = dir.join(&id);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("manifest.json"), manifest.to_string()).unwrap();
        std::fs::write(folder.join("extract.js"), script).unwrap();
        let out = load(&folder);
        let _ = std::fs::remove_dir_all(&dir);
        out
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    fn manifest() -> serde_json::Value {
        serde_json::json!({
            "apiVersion": 1, "id": "demo", "name": "Demo", "url": "https://app.example.com/",
            "domains": ["example.com"],
            "metrics": [{ "key": "balance", "page": "https://app.example.com/billing",
                          "captures": { "b": "/api/balance" } }]
        })
    }

    const SCRIPT: &str = "function balance({ b }, { money }) { return b.cents == null ? null : { value: money(b.cents / 100) }; }";

    fn response(url: &str, body: serde_json::Value) -> Response {
        Response { url: url.into(), body }
    }

    #[test]
    fn bundled_providers_load() {
        let (providers, errors) = load_dir(&providers_dir());
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(providers.len(), 7);
    }

    /// Each fixture lists the responses a page fetched and the result the sidebar should
    /// show. The expectations came from the extractors these plugins replaced.
    #[test]
    fn bundled_provider_fixtures() {
        #[derive(Deserialize)]
        struct Fixture {
            responses: Vec<Response>,
            expect: Option<Expect>,
        }
        #[derive(Deserialize)]
        struct Expect {
            value: String,
            detail: Option<String>,
        }
        let (providers, _) = load_dir(&providers_dir());
        let mut checked = 0;
        for p in &providers {
            let dir = providers_dir().join(&p.manifest.id).join("fixtures");
            let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
            files.sort();
            assert!(!files.is_empty(), "{} has no fixtures", p.manifest.id);
            for file in files {
                let fx: Fixture = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
                let got = extract(p, p.metric().unwrap(), &fx.responses).unwrap();
                let want = match fx.expect {
                    None => Extracted::Pending,
                    Some(e) => Extracted::Ok { value: e.value, detail: e.detail },
                };
                assert_eq!(got, want, "{}", file.display());
                checked += 1;
            }
        }
        assert!(checked >= 14);
    }

    /// Expected strings are Node's Intl.NumberFormat("en-US") / toLocaleString("en-US").
    #[test]
    fn money_and_number_match_intl() {
        let money = [
            ("0", "USD", "$0.00"), ("3.625", "USD", "$3.63"), ("1.005", "USD", "$1.01"),
            ("-5.05", "USD", "-$5.05"), ("-0.004", "USD", "-$0.00"), ("1234567.891", "USD", "$1,234,567.89"),
            ("'1234.5'", "USD", "$1,234.50"), ("'5'", "USD", "$5.00"), ("0.5", "JPY", "¥1"),
            ("1234.5", "JPY", "¥1,235"), ("1234.5", "CNY", "CN¥1,234.50"), ("12.3", "EUR", "€12.30"),
            ("12.3", "GBP", "£12.30"), ("12.3", "CHF", "CHF\u{a0}12.30"), ("1e21", "USD", "$1,000,000,000,000,000,000,000.00"),
            ("2.675", "USD", "$2.68"), ("-1234.5", "EUR", "-€1,234.50"), ("1234.5", "KRW", "₩1,235"),
            ("1234.5", "CAD", "CA$1,234.50"), ("NaN", "USD", "$NaN"), ("-0", "USD", "-$0.00"),
        ];
        let number = [
            ("0", "0"), ("733", "733"), ("117500", "117,500"), ("1234567", "1,234,567"), ("1.5", "1.5"),
            ("1234.5678", "1,234.568"), ("-9876.54321", "-9,876.543"), ("0.0005", "0.001"), ("-0", "-0"),
        ];
        sandbox("", |api| {
            let m: Function = api.get("money")?;
            let n: Function = api.get("number")?;
            let ctx = api.ctx().clone();
            for (input, cur, want) in money {
                let v: Value = ctx.eval(input)?;
                assert_eq!(m.call::<_, String>((v, cur))?, want, "money({input}, {cur})");
            }
            for (input, want) in number {
                let v: Value = ctx.eval(input)?;
                assert_eq!(n.call::<_, String>((v,))?, want, "number({input})");
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn extracts_and_waits_for_required_captures() {
        let p = provider(manifest(), SCRIPT).unwrap();
        let m = p.metric().unwrap();
        assert_eq!(extract(&p, m, &[]).unwrap(), Extracted::Pending);
        let rs = [response("https://api.example.com/api/balance", serde_json::json!({ "cents": 1234 }))];
        assert_eq!(extract(&p, m, &rs).unwrap(), Extracted::Ok { value: "$12.34".into(), detail: None });
        // The function returning null keeps the scrape waiting.
        let rs = [response("https://api.example.com/api/balance", serde_json::json!({}))];
        assert_eq!(extract(&p, m, &rs).unwrap(), Extracted::Pending);
    }

    #[test]
    fn drops_responses_from_other_domains() {
        let p = provider(manifest(), SCRIPT).unwrap();
        let rs = [
            response("https://example.com.evil.net/api/balance", serde_json::json!({ "cents": 1 })),
            response("https://notexample.com/api/balance", serde_json::json!({ "cents": 2 })),
        ];
        assert_eq!(extract(&p, p.metric().unwrap(), &rs).unwrap(), Extracted::Pending);
    }

    #[test]
    fn typesafe_pattern_ignores_api_keys() {
        let (providers, _) = load_dir(&providers_dir());
        let p = providers.iter().find(|p| p.manifest.id == "typesafe").unwrap();
        let rows = serde_json::json!([{ "ok": true, "data": { "balanceCents": 5 } }]);
        let rs = [response("https://console.typesafe.ai/api/api-keys", rows)];
        assert_eq!(extract(p, p.metric().unwrap(), &rs).unwrap(), Extracted::Pending);
    }

    #[test]
    fn rejects_bad_manifests() {
        let cases: [(&str, serde_json::Value); 10] = [
            ("apiVersion", serde_json::json!({ "apiVersion": 2 })),
            ("https", serde_json::json!({ "url": "http://app.example.com/" })),
            ("domains", serde_json::json!({ "url": "https://example.org/" })),
            ("example.com", serde_json::json!({ "domains": ["com"] })),
            ("example.com", serde_json::json!({ "domains": ["127.0.0.1"] })),
            ("example.com", serde_json::json!({ "domains": ["localhost"] })),
            ("at most one", serde_json::json!({ "metrics": [manifest()["metrics"][0], manifest()["metrics"][0]] })),
            ("folder", serde_json::json!({ "id": "Demo" })),
            ("unknown field", serde_json::json!({ "scripts": ["https://cdn.example.com/x.js"] })),
            ("page", serde_json::json!({ "metrics": [{ "key": "balance", "page": "https://other.net/", "captures": { "b": "x" } }] })),
        ];
        for (want, patch) in cases {
            let mut m = manifest();
            for (k, v) in patch.as_object().unwrap() {
                m[k] = v.clone();
            }
            let err = provider(m, SCRIPT).unwrap_err();
            assert!(err.contains(want), "expected \"{want}\" in: {err}");
        }
        let mut m = manifest();
        m["metrics"][0]["required"] = serde_json::json!(["missing"]);
        assert!(provider(m, SCRIPT).unwrap_err().contains("not in captures"));
        let mut m = manifest();
        m["metrics"][0]["captures"]["b"] = serde_json::json!("(");
        assert!(provider(m, SCRIPT).unwrap_err().contains("capture \"b\""));
        assert!(provider(manifest(), "function other() {}").unwrap_err().contains("must define function balance"));
        assert!(provider(manifest(), "function balance( {").unwrap_err().contains("extract.js"));
    }

    #[test]
    fn rejects_bad_results() {
        let rs = [response("https://app.example.com/api/balance", serde_json::json!({}))];
        for script in [
            "function balance() { return { value: 12 }; }",
            "function balance() { return { value: '' }; }",
            "function balance() { return { value: 'x'.repeat(41) }; }",
            "function balance() { return { value: '$1', detail: ['a'] }; }",
        ] {
            let p = provider(manifest(), script).unwrap();
            assert!(extract(&p, p.metric().unwrap(), &rs).is_err(), "{script}");
        }
    }

    #[test]
    fn sandbox_has_no_network_and_stops_runaway_code() {
        let names = "['fetch', 'XMLHttpRequest', 'WebSocket', 'require', 'process', 'std', 'os', 'window', 'document']";
        let script = format!("function balance() {{ return {{ value: {names}.filter(n => n in globalThis).join(',') || 'none' }}; }}");
        let p = provider(manifest(), &script).unwrap();
        let rs = [response("https://app.example.com/api/balance", serde_json::json!({}))];
        let want = Extracted::Ok { value: "none".into(), detail: None };
        assert_eq!(extract(&p, p.metric().unwrap(), &rs).unwrap(), want);

        let p = provider(manifest(), "function balance() { for (;;) {} }").unwrap();
        let started = Instant::now();
        assert!(extract(&p, p.metric().unwrap(), &rs).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
