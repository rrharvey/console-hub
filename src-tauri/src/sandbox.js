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

  // Name each capture by the latest response whose URL matches its pattern, then call the
  // provider's function for the metric. Until there's a result, says why: required
  // captures that haven't arrived ("missing"), or the function returned null ("empty").
  function run(metricJson, responsesJson) {
    const metric = JSON.parse(metricJson);
    const responses = JSON.parse(responsesJson);
    const captures = {};
    for (const [name, pattern] of Object.entries(metric.captures)) {
      const re = new RegExp(pattern);
      for (const r of responses) if (re.test(r.url)) captures[name] = r.body;
    }
    const missing = metric.required.filter((name) => !(name in captures));
    if (missing.length) return JSON.stringify({ status: "missing", missing });
    const out = globalThis[metric.key](captures, helpers);
    if (out == null) return JSON.stringify({ status: "empty" });
    return JSON.stringify({ status: "ok", value: out.value, detail: out.detail ?? null });
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

  return { run, check, money, number };
})()
