//! Tools for provider authors, run through the app binary so they use the app's own
//! validation and sandbox (see PLUGINS.md). Neither opens a window.
//!
//!   console-hub test-provider <folder>...
//!       Check each provider (or folder of them) and run its fixtures.
//!   console-hub fixture <capture.json> <provider folder> <name>
//!       Turn a capture (`console-hub capture`, capture.rs) into fixtures/<name>.json,
//!       cut down to what extract.js reads and with identifying strings replaced.

use std::path::Path;

use serde::Deserialize;
use tauri::Url;

use crate::providers::{self, Expect, Extracted, Fixture, Provider, Response};

pub fn test_provider(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: console-hub test-provider <provider folder, or a folder of them>...");
        return 2;
    }
    let mut failed = 0;
    for arg in args {
        let (found, errors) = providers::inspect_all(Path::new(arg));
        for e in &errors {
            println!("\u{2717} {arg}: {e}");
            failed += 1;
        }
        for p in &found {
            println!("{} ({})", p.manifest.name, p.dir.display());
            if p.metric().is_none() {
                println!("  \u{2713} manifest.json (no metric, nothing to run)");
                continue;
            }
            let files = providers::fixture_files(&p.dir);
            if files.is_empty() {
                println!("  ! no fixtures in fixtures/ (see `npm run fixture`)");
            }
            for file in files {
                let name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
                match providers::check_fixture(p, &file) {
                    Ok(()) => println!("  \u{2713} {name}"),
                    Err(e) => {
                        println!("  \u{2717} {name}: {e}");
                        failed += 1;
                    }
                }
            }
        }
    }
    if failed > 0 {
        println!("\n{failed} failed");
        1
    } else {
        0
    }
}

/// What `console-hub capture` writes (the fields used here).
#[derive(Deserialize)]
pub struct Capture {
    pub responses: Vec<Response>,
}

pub fn fixture(args: &[String]) -> i32 {
    let [capture, folder, name] = args else {
        eprintln!("usage: console-hub fixture <capture.json> <provider folder> <fixture name>");
        return 2;
    };
    match make_fixture(Path::new(capture), Path::new(folder), name) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            0
        }
        Err(e) => {
            eprintln!("\u{2717} {e}");
            1
        }
    }
}

/// Write `<provider>/fixtures/<name>.json` from a capture and return what to tell the user.
fn make_fixture(capture: &Path, folder: &Path, name: &str) -> Result<Vec<String>, String> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(format!("fixture name \"{name}\" must be lowercase letters, digits and dashes"));
    }
    let p = one_provider(folder)?;
    let metric = p.metric().ok_or("the provider has no metric, so there's nothing to test")?;
    let file = p.dir.join("fixtures").join(format!("{name}.json"));
    if file.exists() {
        return Err(format!("{} already exists; pick another name or delete it", file.display()));
    }
    let raw = std::fs::read_to_string(capture).map_err(|e| format!("{}: {e}", capture.display()))?;
    let cap: Capture = serde_json::from_str(&raw).map_err(|e| format!("{}: not a capture: {e}", capture.display()))?;

    // Only what the app would pass the provider: responses on its domains.
    let responses: Vec<Response> = cap
        .responses
        .into_iter()
        .filter(|r| Url::parse(&r.url).ok().and_then(|u| u.host_str().map(|h| providers::host_allowed(h, &p.manifest.domains))).unwrap_or(false))
        .collect();
    let traced = providers::trace(&p, metric, &responses)?;
    if let Extracted::Missing(names) = &traced.result {
        let wanted: Vec<String> = names.iter().map(|n| format!("{n}: {}", metric.captures[n])).collect();
        return Err(format!("the capture has no response for {}. Check the patterns against the URLs in the capture.", wanted.join(", ")));
    }
    let redacted = providers::redact(metric, &traced.responses)?;
    if !redacted.errors.is_empty() {
        return Err(format!(
            "redacting changed URLs so they no longer match: {}. Write this fixture by hand.",
            redacted.errors.join("; ")
        ));
    }
    let result = providers::extract(&p, metric, &redacted.responses)?;
    let fx = Fixture {
        note: Some(format!(
            "Made from a capture of {}. Check it holds no account data, change the numbers to made-up values, and update expect.",
            metric.page
        )),
        responses: redacted.responses,
        expect: Expect::from(&result),
    };
    std::fs::create_dir_all(p.dir.join("fixtures")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&fx).map_err(|e| e.to_string())?;
    std::fs::write(&file, json + "\n").map_err(|e| format!("{}: {e}", file.display()))?;
    providers::check_fixture(&p, &file).map_err(|e| format!("the new fixture doesn't pass: {e}"))?;

    let mut lines = vec![format!("\u{2713} wrote {}", file.display())];
    lines.push(if traced.same {
        format!("  {} response(s), cut down to the parts extract.js reads", traced.responses.len())
    } else {
        "  ! extract.js gave a different result on the cut-down responses, so they're whole. Remove what it doesn't need by hand.".into()
    });
    lines.push(format!("  {} identifying string(s) replaced", redacted.count));
    lines.push(match &fx.expect {
        Some(e) => format!("  expect: {}", serde_json::to_string(e).unwrap_or_default()),
        None => "  expect: null (no balance came out of this capture)".into(),
    });
    lines.push("  Numbers are kept as captured: change balances and usage to made-up values, then update expect.".into());
    Ok(lines)
}

fn one_provider(folder: &Path) -> Result<Provider, String> {
    let (mut found, errors) = providers::inspect_all(folder);
    match (found.len(), errors.first()) {
        (1, _) => Ok(found.remove(0)),
        (0, Some(e)) => Err(format!("{}: {e}", folder.display())),
        _ => Err(format!("{}: choose one provider's folder", folder.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("ch-cli-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn copy_sample(id: &str, to: &Path) -> PathBuf {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples").join(id);
        let dst = to.join(id);
        std::fs::create_dir_all(&dst).unwrap();
        for f in ["manifest.json", "extract.js"] {
            std::fs::copy(src.join(f), dst.join(f)).unwrap();
        }
        dst
    }

    /// A capture shaped like a real one: the account response carries personal fields
    /// next to the ones the provider reads, plus unrelated responses with ids in URLs.
    #[test]
    fn fixture_from_capture_keeps_only_what_extract_reads() {
        let dir = temp_dir("fixture");
        let provider = copy_sample("tavily", &dir);
        let account = serde_json::json!({
            "name": "Jane Doe", "email": "jane.doe@example.org", "uid": "u_8f3k2l9d0s7a6q1w2e3r",
            "picture": "https://lh3.googleusercontent.com/a/ACg8ocJ4kLmN0pQrStUvWx",
            "usage": 412, "limit": 1000, "addon_limit": 0, "current_plan": "researcher",
            "plan_display_name": "Researcher", "paygo": false, "paygo_usage": 0, "paygo_limit": 0,
            "paygo_cpc": 0.008, "reset_cycle": "2026-11-01",
            "email_alert": { "alert_type": "usage", "threshold": 80, "enabled": true },
            "session_token": "s3cr3t-token-value-1234567890"
        });
        let capture = serde_json::json!({
            "page": "https://app.tavily.com/home",
            "responses": [
                { "url": "https://app.tavily.com/api/keys?include_deleted=true", "body": [{ "key": "tvly-ABCDEF1234567890abcdef" }] },
                { "url": "https://app.tavily.com/api/account?uid=u_8f3k2l9d0s7a6q1w2e3r", "body": account },
                { "url": "https://cdn.cookielaw.org/consent/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d/en.json", "body": { "x": 1 } }
            ]
        });
        let cap_file = dir.join("capture.json");
        std::fs::write(&cap_file, capture.to_string()).unwrap();

        let lines = make_fixture(&cap_file, &provider, "plan").unwrap();
        assert!(lines.iter().any(|l| l.contains("cut down")), "{lines:?}");
        let written = std::fs::read_to_string(provider.join("fixtures/plan.json")).unwrap();
        for leak in ["Jane", "jane.doe", "u_8f3k2l9d0s7a6q1w2e3r", "googleusercontent", "s3cr3t", "tvly-", "cookielaw", "email_alert"] {
            assert!(!written.contains(leak), "{leak} leaked into:\n{written}");
        }
        let fx: Fixture = serde_json::from_str(&written).unwrap();
        assert_eq!(fx.responses.len(), 1);
        assert_eq!(fx.responses[0].url, "https://app.tavily.com/api/account?uid=redacted");
        assert_eq!(fx.expect.unwrap().value, "588 credits");
        // It passes as a fixture, and a second run won't overwrite it.
        assert_eq!(test_provider(&[provider.display().to_string()]), 0);
        assert!(make_fixture(&cap_file, &provider, "plan").unwrap_err().contains("already exists"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Account ids in URL paths are replaced, and the URL still matches its pattern.
    #[test]
    fn fixture_redacts_ids_in_urls() {
        let dir = temp_dir("urls");
        let provider = copy_sample("voyage", &dir);
        let capture = serde_json::json!({
            "page": "https://dashboard.voyageai.com/organization/billing",
            "responses": [{
                "url": "https://prod.dashboardbackend.voyageai.com/api/balance/org_7Hq2kLm9Xp4R",
                "body": { "data": { "balance": { "balance": "25", "autoRechargeEnabled": false,
                          "orgId": "org_7Hq2kLm9Xp4R", "updatedBy": "3f2a9c4e-1b7d-4e8a-9c0f-5d6e7a8b9c01" } } }
            }]
        });
        let cap_file = dir.join("capture.json");
        std::fs::write(&cap_file, capture.to_string()).unwrap();
        make_fixture(&cap_file, &provider, "balance").unwrap();
        let written = std::fs::read_to_string(provider.join("fixtures/balance.json")).unwrap();
        assert!(!written.contains("7Hq2kLm9Xp4R") && !written.contains("3f2a9c4e"), "{written}");
        assert!(written.contains("/api/balance/org_example"), "{written}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// TypeSafe's response is an array of Next.js rows that extract.js searches with
    /// Array.isArray and find, which must work the same through the tracing proxies.
    #[test]
    fn fixture_from_rows_keeps_only_the_balance_row() {
        let dir = temp_dir("rows");
        let provider = copy_sample("typesafe", &dir);
        let rows = serde_json::json!([
            { "a": "$@1", "q": "session-9f8e7d6c5b4a39281706", "i": false },
            { "ok": true, "data": { "balanceCents": 1999, "userEmail": "someone@example.org" } }
        ]);
        let capture = serde_json::json!({
            "page": "https://console.typesafe.ai/keys",
            "responses": [{ "url": "https://console.typesafe.ai/keys", "body": rows }]
        });
        let cap_file = dir.join("capture.json");
        std::fs::write(&cap_file, capture.to_string()).unwrap();
        let lines = make_fixture(&cap_file, &provider, "rows").unwrap();
        assert!(lines.iter().any(|l| l.contains("cut down")), "{lines:?}");
        let written = std::fs::read_to_string(provider.join("fixtures/rows.json")).unwrap();
        for leak in ["session-9f8e", "$@1", "someone@", "userEmail"] {
            assert!(!written.contains(leak), "{leak} leaked into:\n{written}");
        }
        let fx: Fixture = serde_json::from_str(&written).unwrap();
        assert_eq!(fx.expect.unwrap().value, "$19.99");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// When tracing changes the result, the fixture keeps whole responses and says so,
    /// rather than writing a wrong expect. Here: each read through a proxy returns a new
    /// proxy, so an object isn't identical to itself.
    #[test]
    fn fixture_keeps_whole_responses_when_tracing_changes_the_result() {
        let dir = temp_dir("fallback");
        let provider = dir.join("demo");
        std::fs::create_dir_all(&provider).unwrap();
        let manifest = serde_json::json!({
            "apiVersion": 1, "id": "demo", "name": "Demo", "url": "https://app.example.com/", "domains": ["example.com"],
            "metrics": [{ "key": "balance", "page": "https://app.example.com/billing", "captures": { "b": "/api/balance" } }]
        });
        std::fs::write(provider.join("manifest.json"), manifest.to_string()).unwrap();
        std::fs::write(provider.join("extract.js"), "function balance({ b }) { return { value: b.data === b.data ? 'same' : 'different' }; }").unwrap();
        let capture = serde_json::json!({ "page": "https://app.example.com/billing", "responses": [
            { "url": "https://app.example.com/api/balance", "body": { "data": { "x": 1 }, "other": "kept" } }
        ]});
        let cap_file = dir.join("capture.json");
        std::fs::write(&cap_file, capture.to_string()).unwrap();
        let lines = make_fixture(&cap_file, &provider, "whole").unwrap();
        assert!(lines.iter().any(|l| l.contains("so they're whole")), "{lines:?}");
        let fx: Fixture = serde_json::from_str(&std::fs::read_to_string(provider.join("fixtures/whole.json")).unwrap()).unwrap();
        assert_eq!(fx.expect.unwrap().value, "same");
        assert_eq!(fx.responses[0].body["other"], "kept");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fixture_explains_a_capture_without_the_response() {
        let dir = temp_dir("missing");
        let provider = copy_sample("openai", &dir);
        let cap_file = dir.join("capture.json");
        let capture = serde_json::json!({ "page": "https://platform.openai.com/", "responses": [] });
        std::fs::write(&cap_file, capture.to_string()).unwrap();
        let err = make_fixture(&cap_file, &provider, "x").unwrap_err();
        assert!(err.contains("grants: /dashboard/billing/credit_grants"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_provider_runs_every_sample() {
        let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples");
        assert_eq!(test_provider(&[samples.display().to_string()]), 0);
    }
}
