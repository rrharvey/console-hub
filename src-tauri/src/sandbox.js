// Evaluated first in each QuickJS sandbox (providers.rs), before the provider's extract.js.
// Returns the entry points Rust calls, so a provider can't replace them. QuickJS has no
// Intl, so `money` and `number` reproduce Intl.NumberFormat("en-US") for the cases
// providers use (checked against Node's output in providers.rs tests).
(() => {
  // Round the shortest decimal form of `n` (what String(n) prints) to `digits` places,
  // half away from zero, as Intl does: 1.005 -> "1.01" although the double is just below.
  function decimal(n, digits) {
    const [mantissa, e = "0"] = String(Math.abs(n)).split("e");
    const [whole, frac = ""] = mantissa.split(".");
    let ds = whole + frac;
    let point = whole.length + Number(e);
    if (point < 0) { ds = "0".repeat(-point) + ds; point = 0; }
    ds = ds.padEnd(point + digits + 1, "0");
    let kept = ds.slice(0, point + digits).split("");
    if (ds[point + digits] >= "5") {
      let i = kept.length - 1;
      while (i >= 0 && kept[i] === "9") kept[i--] = "0";
      if (i < 0) kept.unshift("1"); else kept[i] = String(Number(kept[i]) + 1);
    }
    kept = kept.join("");
    const cut = kept.length - digits;
    return { int: kept.slice(0, cut).replace(/^0+(?=\d)/, "") || "0", frac: kept.slice(cut) };
  }
  const group = (int) => int.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  const negative = (n) => n < 0 || Object.is(n, -0);

  const SYMBOLS = { USD: "$", EUR: "€", GBP: "£", JPY: "¥", CNY: "CN¥", KRW: "₩", INR: "₹",
                    CAD: "CA$", AUD: "A$", HKD: "HK$" };
  const NO_CENTS = new Set(["JPY", "KRW"]);

  function money(value, currency = "USD") {
    const cur = String(currency).toUpperCase();
    const symbol = SYMBOLS[cur] ?? `${cur} `;
    const n = Number(value);
    if (Number.isNaN(n)) return `${symbol}NaN`;
    if (!Number.isFinite(n)) return `${n < 0 ? "-" : ""}${symbol}∞`;
    const d = decimal(n, NO_CENTS.has(cur) ? 0 : 2);
    return `${negative(n) ? "-" : ""}${symbol}${group(d.int)}${d.frac ? "." + d.frac : ""}`;
  }

  // Like n.toLocaleString("en-US"): grouped, at most 3 decimals.
  function number(value) {
    const n = Number(value);
    if (!Number.isFinite(n)) return String(n);
    const d = decimal(n, 3);
    const frac = d.frac.replace(/0+$/, "");
    return `${negative(n) ? "-" : ""}${group(d.int)}${frac ? "." + frac : ""}`;
  }

  const helpers = Object.freeze({ money, number });

  // Name each capture by the latest response whose URL matches its pattern.
  function pick(metric, responses) {
    const picked = {};
    for (const [name, pattern] of Object.entries(metric.captures)) {
      const re = new RegExp(pattern);
      for (const r of responses) if (re.test(r.url)) picked[name] = r;
    }
    return picked;
  }

  // Call the provider's function for the metric. Until there's a result, says why:
  // required captures that haven't arrived ("missing"), or it returned null ("empty").
  function call(metric, captures) {
    const missing = metric.required.filter((name) => !(name in captures));
    if (missing.length) return { status: "missing", missing };
    const out = globalThis[metric.key](captures, helpers);
    if (out == null) return { status: "empty" };
    return { status: "ok", value: out.value, detail: out.detail ?? null };
  }

  const bodies = (picked) => Object.fromEntries(Object.entries(picked).map(([name, r]) => [name, r.body]));

  function run(metricJson, responsesJson) {
    const metric = JSON.parse(metricJson);
    return JSON.stringify(call(metric, bodies(pick(metric, JSON.parse(responsesJson)))));
  }

  // For the fixture tool (cli.rs): run the function on captures wrapped in proxies that
  // record what it reads, and return each picked response cut down to those parts. If
  // the cut-down responses don't give the same result, `same` is false and they're whole.
  const WHOLE = {};
  const own = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
  function trace(metricJson, responsesJson) {
    const metric = JSON.parse(metricJson);
    const picked = pick(metric, JSON.parse(responsesJson));
    const reads = {};
    const wrap = (value, name, path) => {
      if (value === null || typeof value !== "object") return value;
      return new Proxy(value, {
        get(t, k, r) {
          const v = Reflect.get(t, k, r);
          if (typeof k !== "string" || !own(t, k)) return v;
          reads[name].push(path.concat(k));
          return wrap(v, name, path.concat(k));
        },
        has(t, k) {
          if (typeof k === "string") reads[name].push(path.concat(k));
          return Reflect.has(t, k);
        },
        // Enumerating (Object.keys, for..in, JSON.stringify) keeps the whole object.
        ownKeys(t) {
          reads[name].push(path.concat(WHOLE));
          return Reflect.ownKeys(t);
        },
      });
    };
    const proxied = {};
    for (const [name, r] of Object.entries(picked)) {
      reads[name] = [[]];
      proxied[name] = wrap(r.body, name, []);
    }
    const full = call(metric, proxied);

    const container = (v) => (Array.isArray(v) ? [] : {});
    const copyPath = (src, dst, path) => {
      for (const k of path) {
        if (k === WHOLE) {
          for (const key of Object.keys(src)) dst[key] = JSON.parse(JSON.stringify(src[key]));
          return;
        }
        if (src === null || typeof src !== "object" || !own(src, k)) return;
        if (Array.isArray(src) && k === "length") { dst.length = src.length; return; }
        const v = src[k];
        if (v === null || typeof v !== "object") { dst[k] = v; return; }
        if (!own(dst, k)) dst[k] = container(v);
        src = v;
        dst = dst[k];
      }
    };
    const pruned = {};
    for (const [name, r] of Object.entries(picked)) {
      if (r.body === null || typeof r.body !== "object") { pruned[name] = r; continue; }
      const body = container(r.body);
      for (const path of reads[name]) copyPath(r.body, body, path);
      pruned[name] = { url: r.url, body };
    }
    const same = JSON.stringify(call(metric, bodies(pruned))) === JSON.stringify(full);
    const out = Object.entries(same ? pruned : picked).map(([name, r]) => ({ name, url: r.url, body: r.body }));
    return JSON.stringify({ result: full, same, responses: out });
  }

  // Load-time checks that need the JS engine: patterns compile, the function exists.
  function check(metricJson) {
    const metric = JSON.parse(metricJson);
    for (const [name, pattern] of Object.entries(metric.captures)) {
      try { new RegExp(pattern); } catch (e) { throw new Error(`capture "${name}": ${e.message}`); }
    }
    if (typeof globalThis[metric.key] !== "function") {
      throw new Error(`extract.js must define function ${metric.key}(captures, helpers)`);
    }
  }

  return { run, check, trace, money, number };
})()
