// Edit this list to add or change vendors. Verify each URL once; consoles move pages around.
// The list is sorted by name below, so the sidebar and the info bar stay alphabetical.
// `external: true` opens the vendor in the user's default browser instead of an embedded webview.
// Login popups (window.open + postMessage) work in embedded webviews via `on_new_window` in main.rs.
// `balanceUrl` is the page the info bar loads in the background to read the credit balance.
// The matching extractor lives in `ui/scrape.js` (EXTRACTORS, keyed by `id`).
// `balanceNote` instead shows a dash in the bar with this text as its tooltip.
window.VENDORS = [
  { id: "anthropic", name: "Anthropic",   url: "https://platform.claude.com/",
    balanceUrl: "https://platform.claude.com/settings/billing" },
  { id: "deepseek",  name: "DeepSeek",    url: "https://platform.deepseek.com/usage",
    balanceUrl: "https://platform.deepseek.com/usage" },
  { id: "openai",    name: "OpenAI",      url: "https://platform.openai.com/usage",
    balanceUrl: "https://platform.openai.com/settings/organization/billing/overview" },
  { id: "tavily",    name: "Tavily",      url: "https://app.tavily.com/home",
    balanceUrl: "https://app.tavily.com/home" },
  { id: "typesafe",  name: "TypeSafe AI", url: "https://console.typesafe.ai/keys",
    balanceUrl: "https://console.typesafe.ai/keys" },
  { id: "voyage",    name: "Voyage AI",   url: "https://dashboard.voyageai.com/",
    balanceUrl: "https://dashboard.voyageai.com/organization/billing" },
].sort((a, b) => a.name.localeCompare(b.name, "en", { sensitivity: "base" }));
