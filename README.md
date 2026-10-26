# Console Hub (POC)

One window with a sidebar of AI vendor consoles. Each console is a real child webview
(Tauri multiwebview, `unstable` feature), so logins work like normal and framing
restrictions don't apply. No keys are stored.

## Run (macOS)

Needs Rust (https://rustup.rs) and Node.

    npm install
    npm run dev

`npm run build` makes the release app and copies it to `/Applications/Console Hub.app`
(the `postbuild` script). Reopen the app if it was running.

## Providers

Each console is a provider plugin, a folder in `providers/`:

- `manifest.json`: the name, the console `url`, the `domains` the provider may touch, and
  one `metrics` entry for the balance: the `page` to load in the background and the
  `captures` (name -> regex) matching the URLs of the JSON responses that hold it.
  `required` lists the captures that must arrive first (all of them if left out).
- `extract.js`: a function named after the metric key, e.g.
  `function balance({ grants }, { money, number }) { return { value, detail } }`.
  Return `null` to keep waiting.
- `fixtures/*.json`: responses a page fetched and the `expect`ed result. `cargo test`
  (in `src-tauri`) runs every provider against its fixtures. Use made-up values only.

Debug builds read `providers/` from the source tree (restart to pick up edits); the
release app bundles a copy. A provider that fails to load is left out, and the sidebar
says why. A provider with no metric is just a link; `"external": true` opens its console
in the default browser.

## Credit balances

The sidebar shows each provider's credit balance next to its name. It refreshes on launch,
every 15 minutes, and when you click ↻ at the bottom of the sidebar. Hover a provider for details.

- For each provider with a metric, Rust opens an off-screen webview on its `page`. It shares
  the consoles' logins, records the JSON responses matching `captures`, then closes.
- `src-tauri/src/scrape.js` is injected into those background pages only, and holds no
  provider code. The provider's `extract.js` runs in a QuickJS sandbox in Rust
  (`src-tauri/src/providers.rs`): no network, no DOM, no app commands, 500 ms and 32 MB.
- "sign in" means the page redirected to a login. Open that console and log in again.
- "not found" means the page loaded but no balance came out. The provider probably changed
  its API. Find the new endpoint with the dump below and update the manifest or `extract.js`.
  The tooltip shows the provider's error, if it threw one.

To see what a billing page loads, run the binary with a dump directory:

    CONSOLEHUB_DUMP_DIR=/tmp/ch-dump src-tauri/target/debug/console-hub

Each provider writes `<id>.json` (page text, requests, all JSON responses, and which ones
matched `captures`) and `<id>.result.json`.
The dumps contain account data and session tokens. Delete them when done.

## Logins and Google sign-in

Google refuses to show its password page inside embedded web views ("This browser or
app may not be secure"). Once a Google session exists, sites that use Google sign-in
only show the account picker, which works.

The dev build and the installed app keep separate logins: the dev build under
`console-hub`, the installed app under its bundle ID `dev.consolehub.desktop`. If the
installed app ever needs Google's password page again, copy the dev build's logins over.
Quit the installed app first.

    rm -rf ~/Library/WebKit/dev.consolehub.desktop/WebsiteData
    cp -R ~/Library/WebKit/console-hub/WebsiteData ~/Library/WebKit/dev.consolehub.desktop/
    cp ~/Library/HTTPStorages/console-hub.binarycookies ~/Library/HTTPStorages/dev.consolehub.desktop.binarycookies

## Notes

- Layout lives in `src-tauri/src/main.rs` (`layout`, `show_vendor`). The sidebar is `ui/index.html`.
- All vendor webviews share the default web data store. Separate stores per vendor are not set up yet.
- The app icon is drawn in `src-tauri/icons/app-icon.svg`. After editing it, regenerate every
  size with `npx tauri icon src-tauri/icons/app-icon.svg`, then delete the `android` and `ios`
  folders it creates.
