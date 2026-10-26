// Redaction for the fixture tool (cli.rs), evaluated in its own QuickJS sandbox with no
// provider code. Runs after `trace` in sandbox.js has cut each response down to what the
// provider reads, so it only has to catch identifying strings among those: emails,
// UUIDs, long tokens, ID-like URL path segments and query values. Numbers are kept.
(() => {
  const EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g;
  const UUID = /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi;
  // 16+ characters mixing letters and digits: API keys, session ids, hashes.
  const TOKEN = /\b(?=[A-Za-z0-9_-]*\d)(?=[A-Za-z0-9_-]*[A-Za-z])[A-Za-z0-9_-]{16,}\b/g;
  // Values under these keys go whatever they look like.
  const SECRET_KEY = /(^|_)(email|token|secret|password|session|cookie|phone|api_?key)($|_)/i;
  const ZERO_UUID = "00000000-0000-4000-8000-000000000000";

  function redactText(s, count) {
    let out = s;
    out = out.replace(EMAIL, () => (count.n++, "user@example.com"));
    out = out.replace(UUID, () => (count.n++, ZERO_UUID));
    out = out.replace(TOKEN, () => (count.n++, "redacted"));
    return out;
  }

  // A path segment that names an account object: a UUID, a token, or letters mixed with
  // digits (org_ab12cd34, proj_9xYz, 123456789). Keeps a `prefix_` so the shape stays.
  function redactSegment(seg, count) {
    if (UUID.test(seg)) { UUID.lastIndex = 0; count.n++; return ZERO_UUID; }
    UUID.lastIndex = 0;
    const m = seg.match(/^([a-z]+[_-])?([A-Za-z0-9]+)$/);
    if (m && /\d/.test(m[2]) && m[2].length >= 6) { count.n++; return `${m[1] || ""}example`; }
    return redactText(seg, count);
  }

  function redactUrl(url, count) {
    const m = url.match(/^([a-z]+:\/\/[^/?#]+)([^?#]*)(\?[^#]*)?/i);
    if (!m) return redactText(url, count);
    const path = m[2].split("/").map((seg) => redactSegment(seg, count)).join("/");
    const query = m[3]
      ? "?" + m[3].slice(1).split("&").map((kv) => {
          const i = kv.indexOf("=");
          if (i < 0) return kv;
          count.n++;
          return `${kv.slice(0, i)}=redacted`;
        }).join("&")
      : "";
    return m[1] + path + query;
  }

  function redactValue(v, key, count) {
    if (typeof v === "string") {
      if (key && SECRET_KEY.test(key)) { count.n++; return "redacted"; }
      return /^https?:\/\//i.test(v) ? redactUrl(v, count) : redactText(v, count);
    }
    if (Array.isArray(v)) return v.map((x) => redactValue(x, null, count));
    if (v && typeof v === "object") {
      const out = {};
      for (const [k, x] of Object.entries(v)) out[k] = redactValue(x, k, count);
      return out;
    }
    return v;
  }

  // responses: [{ name, url, body }]. Returns { responses: [{ url, body }], redacted, errors }.
  // A redacted URL must still match its capture's pattern, or the fixture would test
  // nothing; those are reported for the author to fix by hand.
  function redact(metricJson, responsesJson) {
    const metric = JSON.parse(metricJson);
    const count = { n: 0 };
    const errors = [];
    const responses = JSON.parse(responsesJson).map((r) => {
      const url = redactUrl(r.url, count);
      if (!new RegExp(metric.captures[r.name]).test(url)) {
        errors.push(`${url} no longer matches capture "${r.name}" (${metric.captures[r.name]})`);
      }
      return { url, body: redactValue(r.body, null, count) };
    });
    return JSON.stringify({ responses, redacted: count.n, errors });
  }

  return { redact };
})()
