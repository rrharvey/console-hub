// Injected at document start into the hidden background "scraper" webviews only
// (never into the consoles you browse). It runs inside the provider's page, so it sees
// the same logged-in session. No provider code runs here: this records the JSON
// responses the page itself fetches whose URLs match the provider manifest's capture
// patterns, and Rust polls `window.__ch.result()` and hands them to the provider's
// extract.js in the QuickJS sandbox (providers.rs).
//
// `window.__chConfig` is set by Rust: { captures: [regex source], domains: [host], debug }.
(() => {
  if (window.__ch) return;
  const config = window.__chConfig;
  const patterns = config.captures.map((p) => new RegExp(p));
  const ch = (window.__ch = { responses: [], debug: [], errors: [] });
  const noteError = (msg) => { if (ch.errors.length < 20) ch.errors.push(String(msg).slice(0, 300)); };
  window.addEventListener("error", (e) => noteError(e.message || e));
  window.addEventListener("unhandledrejection", (e) => noteError("rejection: " + (e.reason && e.reason.stack || e.reason)));

  const absolute = (url) => { try { return new URL(url, location.href).href; } catch { return null; } };
  // Same rule as providers::host_allowed: a listed domain or one of its subdomains.
  const allowed = (url) => {
    const host = new URL(url).hostname;
    return config.domains.some((d) => host === d || host.endsWith("." + d));
  };
  const wanted = (url) => allowed(url) && patterns.some((re) => re.test(url));

  // With CONSOLEHUB_DUMP_DIR set, also keep every other JSON-ish response for the dump,
  // so a provider author can find the endpoint that carries the balance.
  const DEBUG = !!config.debug;
  const SKIP_TYPES = /image|font|javascript|css|video|audio|octet-stream/i;

  function keep(rawUrl, text, contentType = "") {
    const url = absolute(rawUrl);
    if (!url || !text || text.length > 300000) return;
    const want = wanted(url);
    if (!want && !DEBUG) return;
    let body;
    try { body = contentType.includes("x-component") ? parseRsc(text) : JSON.parse(text); } catch {
      if (!DEBUG) return;
      body = { __raw: text.slice(0, 20000), contentType };
    }
    if (want) {
      ch.responses.push({ url, body });
      if (ch.responses.length > 30) ch.responses.shift();
    }
    if (DEBUG) {
      ch.debug.push({ url, body, size: text.length });
      if (ch.debug.length > 60) ch.debug.shift();
    }
  }
  // Next.js server responses (`text/x-component`) are lines like `1:{"ok":true,...}`.
  // Return the JSON value of each line that parses; throw if none do.
  function parseRsc(text) {
    const rows = [];
    for (const line of text.split("\n")) {
      const i = line.indexOf(":");
      if (i < 0) continue;
      try { rows.push(JSON.parse(line.slice(i + 1))); } catch {}
    }
    if (!rows.length) throw new Error("no JSON rows");
    return rows;
  }

  const origFetch = window.fetch;
  if (origFetch) {
    window.fetch = async function (...args) {
      const res = await origFetch.apply(this, args);
      try {
        const ct = res.headers.get("content-type") || "";
        const json = ct.includes("json") || ct.includes("x-component");
        if ((json && (DEBUG || wanted(absolute(res.url)))) || (DEBUG && !SKIP_TYPES.test(ct))) {
          res.clone().text().then((t) => keep(res.url, t, ct), () => {});
        }
      } catch {}
      return res;
    };
  }
  const origOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    this.addEventListener("load", () => {
      try {
        const u = this.responseURL || url;
        if (this.responseType === "" || this.responseType === "text") keep(u, this.responseText, this.getResponseHeader("content-type") || "");
        else if (this.responseType === "json") keep(u, JSON.stringify(this.response));
      } catch {}
    });
    return origOpen.call(this, method, url, ...rest);
  };

  const looksLikeLogin = () =>
    /\/(log-?in|sign-?in|auth|sso|oauth)\b/i.test(location.pathname) ||
    /^(auth|login|accounts?|sso)\./i.test(location.hostname);

  ch.result = () =>
    JSON.stringify(looksLikeLogin()
      ? { status: "login", url: location.href }
      : { status: "loaded", url: location.href, responses: ch.responses });

  // Debug dump used during development (CONSOLEHUB_DUMP_DIR).
  const text = () => (document.body ? document.body.innerText : "");
  ch.dump = () =>
    JSON.stringify({
      url: location.href,
      title: document.title,
      visibility: document.visibilityState,
      text: text().slice(0, 20000),
      links: [...document.querySelectorAll("a[href]")]
        .map((a) => [a.innerText.trim().slice(0, 60), a.href])
        .filter(([t, h]) => /bill|credit|balance|usage|plan|payment|wallet|top|settings/i.test(t + h))
        .slice(0, 80),
      dialogs: [...document.querySelectorAll("[role=dialog], [role=alertdialog]")].map((d) => d.innerText.slice(0, 5000)),
      errors: ch.errors,
      requests: performance.getEntriesByType("resource").map((r) => r.name).filter((u) => !/\.(js|css|png|svg|woff2?|jpg|ico)(\?|$)/.test(u)).slice(-60),
      matched: ch.responses.map((r) => r.url),
      captures: ch.debug.map((c) => ({ url: c.url, size: c.size, json: JSON.stringify(c.body).slice(0, 12000) })),
    });
})();
