# Writing a provider

A provider tells Console Hub about one AI vendor's console: where it is and how to read
your credit balance from it. It's a folder with two files:

```
my-provider/
  manifest.json     what to open, load and read
  extract.js        turns the console's JSON into the balance shown in the sidebar
  fixtures/         test cases (optional, but please add them)
```

`samples/` has eight providers to copy from. OpenAI and Tavily are good starting points;
Cloudflare shows one with no metric, which only opens its console.

## How it works

For each provider, the app loads your `page` in a hidden window, signed in with the
user's console login. It records the JSON responses the page fetches from the vendor's
own API whose URLs match your `captures`, and passes them to your function in
`extract.js`. That function returns the text to show.

Reading the console's API responses holds up better than reading the page's HTML: a
redesign rarely changes them. When the vendor does change them, the sidebar keeps the
last balance (dimmed), and its tooltip says what failed. Either no response matched your
`captures` (fix `manifest.json`), or one did but your function found no balance (fix
`extract.js`).

Your code never runs in the console page. It runs in a sandbox (QuickJS) with plain
JavaScript only: no network, no DOM, no `Intl`, no timers, no access to the app. Each call
gets 500 ms and 32 MB.

Before a provider runs, the user is shown its console, the page it will load signed in as
them, the domains it reads, and your `author` line, and must allow it. Approval covers the
exact contents of `manifest.json` and `extract.js`, so any change, including an update
from you, asks again.

## manifest.json

```json
{
  "$schema": "../../plugin-api/manifest.schema.json",
  "apiVersion": 1,
  "id": "openai",
  "name": "OpenAI",
  "author": "Your Name",
  "url": "https://platform.openai.com/usage",
  "domains": ["openai.com"],
  "metrics": [{
    "key": "balance",
    "page": "https://platform.openai.com/settings/organization/billing/overview",
    "captures": {
      "grants": "/dashboard/billing/credit_grants",
      "wallet": "/dashboard/billing/wallet"
    },
    "required": ["grants"]
  }]
}
```

| Field | |
|---|---|
| `apiVersion` | `1`. |
| `id` | Lowercase letters, digits and dashes. The folder is renamed to this when added. |
| `name` | Shown in the sidebar, up to 40 characters, one line. |
| `author` | Optional. Shown (as unverified) when someone allows the provider, up to 60 characters. |
| `url` | The console opened when the provider is selected. |
| `external` | Optional. `true` opens the console in the default browser instead. |
| `domains` | Hosts the provider may load and read, each including its subdomains: `openai.com` covers `platform.openai.com` and `api.openai.com`. At least two labels; no IP addresses. Keep it as narrow as works. |
| `metrics` | Zero or one entry. Leave it out for a provider that only opens its console. |
| `metrics[].key` | The name of your function in `extract.js`. |
| `metrics[].page` | Loaded in the background to read the balance. Pick a page that fetches it soon after loading. |
| `metrics[].captures` | Capture name -> JavaScript regular expression, matched against the full URL of each JSON response. Your function gets each capture's latest matching response body under that name. |
| `metrics[].required` | Captures that must arrive before your function runs. All of them if left out. |

All URLs must be `https` and on one of `domains`; responses from other hosts are dropped.
Pointing `$schema` at `plugin-api/manifest.schema.json` (or a copy) gets you validation
and completion in most editors. The app ignores it.

Responses are parsed as JSON. Next.js server responses (`text/x-component`) arrive as an
array of their rows (see `samples/typesafe`).

## extract.js

```js
/** @param {Captures} captures @param {Helpers} helpers @returns {MetricResult | null} */
function balance({ grants, wallet }, { money }) {
  if (grants.total_available == null) return null;
  const detail = [`${money(grants.total_used)} used of ${money(grants.total_granted)} granted`];
  if (wallet && Number(wallet.balance) > 0) detail.push(`Wallet: ${money(wallet.balance)}`);
  return { value: money(grants.total_available), detail: detail.join("\n") };
}
```

- Define a function named after the metric's `key`. It gets the captures by name and the
  helpers, and returns `{ value, detail? }`: `value` (1 to 40 characters) goes in the
  sidebar, `detail` (up to 1000, lines split with `\n`) in the tooltip.
- Return `null` while there's no balance in the responses yet. The app keeps polling for
  up to 45 seconds. A capture that isn't `required` may be `undefined`.
- If it throws, the error shows in the tooltip.
- Helpers, since there's no `Intl`:
  - `money(value, currency = "USD")` formats like `Intl.NumberFormat("en-US")`:
    `money(1234.5)` is `$1,234.50`, `money("9", "EUR")` is `€9.00`. Numeric strings work.
  - `number(value)` groups digits like `toLocaleString("en-US")`, with at most 3 decimals:
    `number(117500)` is `117,500`.

The comment above the function gives editors the types in `plugin-api/provider.d.ts`. To
check your code the way `samples/` is checked, copy `samples/jsconfig.json` next to your
provider and adjust its paths, or put your provider in `samples/` and run `npm run check-js`.

## Workflow

Run these from the repo, on any OS. They use the dev build's console logins: sign in to
the console in `npm run dev` first (on Windows and Linux, signing in in the installed app
works too), then quit the app, since two copies shouldn't use the same logins at once.

1. **Find the endpoint.** Capture what the billing page fetches:

       npm run capture -- https://console.example.com/billing

   A window opens on the page; sign in there if asked. When responses stop arriving, it
   writes `captures/<host>-<time>.json` and quits. Look through it for the response that
   holds the balance, and note its URL and shape.

2. **Write `manifest.json` and `extract.js`**, then capture with your provider, which
   waits until your function reads a balance:

       npm run capture -- path/to/my-provider

3. **Make a fixture** from that capture:

       npm run fixture -- captures/my-provider-<time>.json path/to/my-provider balance

   This writes `fixtures/balance.json` with only the responses your captures match, cut
   down to the fields your function reads. It replaces emails, UUIDs, long tokens and IDs
   in URLs, and fills in `expect` from what your function returns now. **Numbers are kept as
   captured.** Change balances and usage to made-up values, update `expect` to match, and
   read the whole file before committing it.

4. **Test:**

       npm run test-provider -- path/to/my-provider

   It checks the manifest the way the app does and runs every fixture. Add cases for what
   can vary: a second currency, an optional capture missing, a response with no balance
   (`"expect": null`).

5. **Try it in the app:** click **+** at the bottom of the sidebar, choose your folder and
   allow it. To change it later, edit your copy and add it again; it replaces the old one
   and asks again.

Captures hold account data and session tokens. `captures/` is in `.gitignore`. Never
commit, attach or paste one; delete them when you're done.

## Fixture format

```json
{
  "note": "optional",
  "responses": [
    { "url": "https://api.example.com/v1/billing/balance", "body": { "balance": 12.5 } }
  ],
  "expect": { "value": "$12.50", "detail": "optional" }
}
```

`responses` are what the page fetched, in order; the latest match per capture wins.
`expect` is what the sidebar should show, or `null` for no balance. Use made-up values only.

## Sharing

Share the provider folder: `manifest.json`, `extract.js` and `fixtures/`. People add it
with **+** and see what it loads and reads before allowing it. Set `author` so they know
who it's from, and keep `domains` and `page` as narrow as they can be.
