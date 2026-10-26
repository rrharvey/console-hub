---
name: write-provider
description: Create, fix or review a Console Hub provider plugin (a folder with manifest.json and extract.js that reads an AI vendor console's credit balance). Use this whenever someone wants to add a new console or vendor to Console Hub, a balance shows "not found" or a wrong number, a vendor changed their billing page or API, someone asks to capture a console's responses, make a fixture, run test-provider, or review a provider someone else wrote before allowing it.
---

# Write a Console Hub provider

`PLUGINS.md` at the repo root is the reference: manifest fields, the `extract.js`
contract, helpers, sandbox limits, the fixture format. Read it first. `samples/` has
eight sample providers; `samples/openai` and `samples/tavily` are the clearest models.

## How a provider works (enough to reason about it)

The app loads the manifest's `page` in a hidden window with the user's console login,
records the JSON responses whose URLs match `captures`, and passes their bodies by name to
the function in `extract.js` named after the metric `key`. The function returns
`{ value, detail? }`, or `null` to keep waiting. It runs in a QuickJS sandbox: plain
JavaScript only, no network, DOM, `Intl` or timers.

## Workflow

Run from the repo root. These use the dev build's console logins, so the person should
sign in to the console in `npm run dev` first and then quit it.

1. **Find the endpoint**: `npm run capture -- https://<console>/<billing page>`.
   A window opens; the person may need to sign in there. It writes
   `captures/<host>-<time>.json`. Look for the response holding the balance by its URL
   and shape. Describe what you find (URL, field names, types), not the values.
2. **Write the provider** in a new folder: copy a sample, then set `id`, `name`, `url`,
   `domains` (as narrow as works), the metric's `page` and `captures`. Write the
   function. Prefer the vendor's internal JSON API over anything in the page's HTML;
   it survives redesigns.
3. **Capture with the provider**: `npm run capture -- <folder>`. It waits until the
   function reads a balance, and says what it read.
4. **Make a fixture**: `npm run fixture -- captures/<file>.json <folder> <name>`. Then edit
   it: change every number to a made-up one, update `expect` to match, and read the whole
   file. Add cases for what varies: another currency, a missing optional capture, a
   response with no balance (`"expect": null`).
5. **Test**: `npm run test-provider -- <folder>`. If the provider is in `samples/`, also
   `npm test` and `npm run check-js`.
6. **Try it**: in the app, **+** > choose the folder > **Allow**.

## Fixing a broken provider

Hover the balance in the app; the tooltip says which part broke:

- "never fetched the response this provider reads": the URL moved. Capture the page and
  update the pattern in `captures` (`manifest.json`).
- "no balance came out of it": the response shape changed. Capture, compare with the
  fixture, and update `extract.js`.
- "Provider error: …": the function threw; the message says where.

Then add a fixture for the new shape, so the fix stays fixed.

## Account data: the rules, and why

A capture is a full recording of a signed-in console: names, emails, org and project
IDs, API key prefixes, session tokens, real balances. Fixtures and samples get committed
and shared with the team. Real account data has leaked into this repo's history before,
which is why these rules exist.

- Never commit, attach, paste or print a capture. `captures/` is gitignored; keep them
  there and delete them when done. When describing what's in one, give URLs and field
  names, not values.
- Fixtures use made-up numbers. `npm run fixture` strips identifying strings and keeps
  only the fields the function reads, but it keeps numbers on purpose; you replace them.
- Before committing, search the staged diff for anything real you saw while working:
  org/project IDs, emails, the actual balances. For example:
  `git diff --cached | grep -nE '<id fragment>|<email>|<real balance>'`.
- Don't send capture contents to any outside service.

## Reviewing someone else's provider

Before the person allows it, check:

- `domains` and `page` are what the provider needs, and no broader. The page loads with
  their login.
- `captures` only match the responses `extract.js` uses.
- `extract.js` only reads captures and returns text. The sandbox blocks network access, but
  odd code is still a reason to ask.
- `author` says who it's from. It's unverified, so trust comes from how they got it.
- `npm run test-provider -- <folder>` passes.
