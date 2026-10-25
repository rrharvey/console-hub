// Injected at document start into the hidden background "scraper" webviews only
// (never into the consoles you browse). It runs inside the vendor's page, so it sees
// the same logged-in session. Rust polls `window.__ch.result()` until it reports a value.
//
// Two sources of data:
//   1. JSON responses the page itself fetches (fetch/XHR are wrapped and matching
//      responses are kept in `captures`). Internal API JSON is sturdier than the DOM.
//   2. The rendered page text, as a fallback.
(() => {
  if (window.__ch) return;
  const vendor = window.__chVendor;
  const ch = (window.__ch = { captures: [], errors: [] });
  const noteError = (msg) => { if (ch.errors.length < 20) ch.errors.push(String(msg).slice(0, 300)); };
  window.addEventListener("error", (e) => noteError(e.message || e));
  window.addEventListener("unhandledrejection", (e) => noteError("rejection: " + (e.reason && e.reason.stack || e.reason)));

  // Set by Rust when CONSOLEHUB_DUMP_DIR is set: also keep non-JSON responses (e.g. the
  // Next.js page data `text/x-component` some consoles use) so the dump shows them.
  const DEBUG = !!window.__chDebug;
  const INTERESTING = /balance|credit|grant|wallet|remaining|available|quota|billing|prepaid|usage|plan|subscription/i;
  function keep(url, text, contentType = "") {
    if (!text || text.length > 300000) return;
    if (!INTERESTING.test(url) && !INTERESTING.test(text.slice(0, 20000))) return;
    let json;
    try { json = contentType.includes("x-component") ? parseRsc(text) : JSON.parse(text); } catch {
      if (!DEBUG) return;
      json = { __raw: text.slice(0, 20000), contentType };
    }
    ch.captures.push({ url: String(url), json, size: text.length });
    if (ch.captures.length > 60) ch.captures.shift();
  }
  const SKIP_TYPES = /image|font|javascript|css|video|audio|octet-stream/i;
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
        if (ct.includes("json") || ct.includes("x-component") || (DEBUG && !SKIP_TYPES.test(ct))) {
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
        if (this.responseType === "" || this.responseType === "text") keep(u, this.responseText);
        else if (this.responseType === "json") keep(u, JSON.stringify(this.response));
      } catch {}
    });
    return origOpen.call(this, method, url, ...rest);
  };

  // ---- helpers for extractors -------------------------------------------------
  const text = () => (document.body ? document.body.innerText : "");
  const findCapture = (re) => {
    for (let i = ch.captures.length - 1; i >= 0; i--) if (re.test(ch.captures[i].url)) return ch.captures[i].json;
    return null;
  };
  const money = (n, cur = "USD") =>
    new Intl.NumberFormat("en-US", { style: "currency", currency: cur }).format(Number(n));
  // Look for `label` in the page text and return the first amount within the next ~120 chars.
  const amountAfter = (label) => {
    const t = text();
    const re = new RegExp(label.source + "[\\s\\S]{0,120}?(-?[$€£¥]\\s?-?[\\d,]+(?:\\.\\d+)?|-?[\\d,]+(?:\\.\\d+)?\\s?(?:USD|CNY|credits?))", label.flags.replace("g", ""));
    const m = t.match(re);
    return m ? m[1].replace(/\s+/g, " ").trim() : null;
  };
  const looksLikeLogin = () =>
    /\/(log-?in|sign-?in|auth|sso|oauth)\b/i.test(location.pathname) ||
    /^(auth|login|accounts?|sso)\./i.test(location.hostname);

  // ---- per-vendor page actions ---------------------------------------------------
  // Run on every poll before the extractor, for balances that need a click to load.
  const clickByText = (label) => {
    const el = [...document.querySelectorAll("a, button, [role=menuitem], [role=button]")]
      .find((e) => e.innerText && e.innerText.trim() === label && e.offsetParent !== null);
    if (el) el.click();
    return !!el;
  };
  const PREPARE = {};

  // ---- per-vendor extractors ----------------------------------------------------
  // Each returns {value, detail?} when found, or null to keep waiting.
  // Endpoints were found by dumping the JSON each billing page loads (CONSOLEHUB_DUMP_DIR).
  const EXTRACTORS = {
    // GET api.openai.com/v1/dashboard/billing/credit_grants -> { total_available, total_granted, ... }
    openai: ({ findCapture, money }) => {
      const g = findCapture(/\/dashboard\/billing\/credit_grants/);
      if (!g || g.total_available == null) return null;
      const w = findCapture(/\/dashboard\/billing\/wallet/);
      const detail = [`${money(g.total_used)} used of ${money(g.total_granted)} granted`];
      if (w && Number(w.balance) > 0) detail.push(`Wallet: ${money(w.balance)}`);
      return { value: money(g.total_available), detail: detail.join("\n") };
    },

    // GET /api/organizations/<org>/prepaid/credits -> { amount (minor units), currency, balance.credits }
    anthropic: ({ findCapture, money }) => {
      const c = findCapture(/\/prepaid\/credits(\?|$)/);
      if (!c) return null;
      const minor = c.balance?.credits?.amount_minor ?? c.amount;
      const exp = c.balance?.credits?.exponent ?? 2;
      if (minor == null) return null;
      return { value: money(minor / 10 ** exp, c.currency || "USD") };
    },

    // GET /api/v0/users/get_user_summary -> { data.biz_data: { normal_wallets[], bonus_wallets[] } }
    deepseek: ({ findCapture, money }) => {
      const d = findCapture(/\/users\/get_user_summary/)?.data?.biz_data;
      if (!d || !d.normal_wallets) return null;
      const sum = (ws, cur) => (ws || []).filter((w) => w.currency === cur).reduce((a, w) => a + Number(w.balance || 0), 0);
      const cur = d.normal_wallets[0]?.currency || "USD";
      const paid = sum(d.normal_wallets, cur);
      const bonus = sum(d.bonus_wallets, cur);
      const detail = [`Topped up: ${money(paid, cur)}`, `Granted: ${money(bonus, cur)}`];
      return { value: money(paid + bonus, cur), detail: detail.join("\n") };
    },

    // GET prod.dashboardbackend.voyageai.com/api/balance/<org> -> { data: { balance: { balance: "5" } } }
    voyage: ({ findCapture, money }) => {
      const b = findCapture(/\/api\/balance\//)?.data?.balance;
      if (!b || b.balance == null) return null;
      return { value: money(b.balance), detail: b.autoRechargeEnabled ? "Auto recharge on" : "Auto recharge off" };
    },

    // GET app.tavily.com/api/account -> { usage, limit, addon_limit, plan_display_name, paygo* }
    // Tavily sells credits, not a dollar balance, so show credits remaining this cycle.
    // With pay as you go on, the console adds paygo_usage/paygo_limit (credits) to the plan's.
    tavily: ({ findCapture, money }) => {
      const a = findCapture(/\/api\/account(\?|$)/);
      if (!a || a.limit == null) return null;
      const fmt = (n) => n.toLocaleString("en-US");
      const planLimit = Number(a.limit) + Number(a.addon_limit || 0);
      const planUsed = Number(a.usage || 0);
      const paygo = a.paygo ? { used: Number(a.paygo_usage || 0), limit: Number(a.paygo_limit || 0) } : null;
      const used = planUsed + (paygo ? paygo.used : 0);
      const limit = planLimit + (paygo ? paygo.limit : 0);
      const detail = [`${fmt(used)} / ${fmt(limit)} credits used`];
      detail.push(`Plan (${a.plan_display_name || a.current_plan}): ${fmt(planUsed)} / ${fmt(planLimit)}`);
      if (paygo) {
        let line = `Pay as you go: ${fmt(paygo.used)} / ${fmt(paygo.limit)}`;
        if (a.paygo_cpc != null) line += ` (${money(paygo.used * Number(a.paygo_cpc))} at $${Number(a.paygo_cpc)}/credit)`;
        detail.push(line);
      }
      if (a.reset_cycle) detail.push(`Resets ${a.reset_cycle}`);
      return { value: `${fmt(Math.max(0, limit - used))} credits`, detail: detail.join("\n") };
    },

    // The sidebar shows the balance. Loading /keys makes a Next.js server call
    // whose response has a row { ok, data: { balanceCents } }.
    typesafe: ({ captures, money }) => {
      for (let i = captures.length - 1; i >= 0; i--) {
        const rows = captures[i].json;
        const cents = Array.isArray(rows) ? rows.find((r) => r?.data?.balanceCents != null)?.data.balanceCents : null;
        if (cents != null) return { value: money(cents / 100) };
      }
      return null;
    },
  };

  ch.result = () => {
    const out = { url: location.href };
    if (looksLikeLogin()) return JSON.stringify({ ...out, status: "login" });
    try { PREPARE[vendor]?.(); } catch (e) { noteError("prepare: " + e); }
    const ex = EXTRACTORS[vendor];
    if (!ex) return JSON.stringify({ ...out, status: "unsupported" });
    let r = null;
    try { r = ex({ text, findCapture, money, amountAfter, captures: ch.captures }); }
    catch (e) { return JSON.stringify({ ...out, status: "pending", detail: String(e) }); }
    return JSON.stringify(r ? { ...out, status: "ok", ...r } : { ...out, status: "pending" });
  };

  // Debug dump used during development (CONSOLEHUB_DUMP_DIR).
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
      captures: ch.captures.map((c) => ({ url: c.url, size: c.size, json: JSON.stringify(c.json).slice(0, 12000) })),
    });
})();
