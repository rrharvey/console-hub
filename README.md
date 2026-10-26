# Console Hub (POC)

One window with a sidebar of AI vendor consoles. Each console is a real child webview
(Tauri multiwebview, `unstable` feature), so logins work like normal and framing
restrictions don't apply. No keys are stored.

## Run (macOS)

Needs Rust (https://rustup.rs) and Node.

    npm install
    npm run dev

## Edit vendors

`ui/vendors.js`. Verify each URL; consoles move pages around.

## Credit balances

The sidebar shows each vendor's credit balance next to its name. It refreshes on launch,
every 15 minutes, and when you click ↻ at the bottom of the sidebar. Hover a vendor for details.

- For each vendor with a `balanceUrl` (`ui/vendors.js`), Rust opens an off-screen webview
  on that page. It shares the consoles' logins, reads the balance, then closes.
- `ui/scrape.js` is injected into those background pages only. It records the JSON the page
  fetches from the vendor's own API, and an extractor per vendor picks out the balance.
- "sign in" means the page redirected to a login. Open that console and log in again.
- "not found" means the page loaded but no extractor matched. The vendor probably changed
  its API. Find the new endpoint with the dump below and update the extractor.
- A balance that only loads after a click can open it from `PREPARE` in `ui/scrape.js`.

To see what a billing page loads, run the binary with a dump directory:

    CONSOLEHUB_DUMP_DIR=/tmp/ch-dump src-tauri/target/debug/console-hub

Each vendor writes `<id>.json` (page text, requests, captured JSON) and `<id>.result.json`.
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
