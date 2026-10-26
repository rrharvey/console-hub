# Console Hub

If you build with several AI APIs, you probably pay for them with prepaid credits, each in
its own vendor console: Anthropic, OpenAI, DeepSeek, Tavily and so on. Knowing how much you
have left means signing in to each console in turn, and it's easy to notice a balance ran
out only when requests start failing.

Console Hub puts those consoles in one window. A sidebar lists them with each one's credit
balance, refreshed in the background every 15 minutes, so you can see at a glance what's
running low. Click a vendor to use its console as usual, already signed in, to top up or
check usage.

It reads balances from the consoles you're signed in to, so it needs no API keys and stores
none. Each console is a small plugin (a "provider"); `samples/` has seven, and adding
another takes a manifest and a short script. Runs on macOS, Windows and Linux.

- **[Quickstart](#quickstart)**: build and run it from source in five commands.
- **[INSTALL.md](INSTALL.md)**: install a release or build from source, on each OS.
- **[PLUGINS.md](PLUGINS.md)**: write a provider for another console.
- **Agents**: [AGENTS.md](AGENTS.md) lists skills for setup, writing providers and
  releasing, in `.agent/skills/`.

## Quickstart

You need Node.js (LTS), Rust 1.90+ (`rustup update`) and Tauri's OS prerequisites:
Xcode Command Line Tools on macOS, the MSVC C++ Build Tools on Windows, the WebKitGTK 4.1
dev packages on Linux. The exact packages are in [INSTALL.md](INSTALL.md#build-from-source).

    git clone https://github.com/rrharvey/console-hub.git
    cd console-hub
    npm install
    npm test              # compiles everything; a missing toolchain piece fails here, readably
    npm run dev           # run the app

The first build downloads and compiles the crates, which takes a few minutes.

In the app, click **+** at the bottom of the sidebar, choose the repo's `samples` folder and
click **Allow**. Click a console and sign in as usual. Its balance shows up next to its name
within a minute.

Other commands:

    npm run build         # release app and installers (macOS: also installs to /Applications)
    npm run check-js      # type-check the samples' extract.js
    npm run test-provider -- <provider folder>   # validate a provider and run its fixtures

To add a console, see [PLUGINS.md](PLUGINS.md). To use the app without building it,
install it from [Releases](https://github.com/rrharvey/console-hub/releases); the builds
are unsigned, and [INSTALL.md](INSTALL.md#install-a-release) covers the OS warnings.

CI (`.github/workflows/ci.yml`) runs the tests on macOS, Windows and Linux for every push
and pull request. Releases are built by `.github/workflows/release.yml`; see
`.agent/skills/release`.

## Providers

None are built in. Each console is a provider plugin: a folder you add from the app.
`samples/` has providers for Anthropic, DeepSeek, OpenAI, Parallel, Tavily, TypeSafe AI
and Voyage AI.

### Adding them

Click **+** at the bottom of the sidebar and choose a provider folder (any name), or a
folder of them such as `samples/` to add several at once. You're asked once to allow them;
the prompt shows each one's console, the page it loads with your logins, the domains it
reads, and who says they wrote it. Allowed providers are copied to your providers folder
(right-click a provider > **Show Providers Folder**; the path for each OS is in
[INSTALL.md](INSTALL.md#where-things-live)). You can also drop folders there yourself
(restart to see them).

- A provider runs only after you allow it, and only while its `manifest.json` and
  `extract.js` are exactly what you allowed (a SHA-256 kept in `settings.json` next to the
  folder). Change either file and it shows **review** in the sidebar until you allow it again.
- To update one, add its folder again with **+**; it replaces your copy.
- Right-click a provider and choose **Remove Provider…** to delete it.
- The dev build and the installed app share this folder and `settings.json`.

### Writing one

See [PLUGINS.md](PLUGINS.md): the manifest and `extract.js`, and the tools for capturing a
console's responses, making redacted test fixtures and testing a provider:

    npm run capture -- <page URL | provider folder>
    npm run fixture -- <capture> <provider folder> <fixture name>
    npm run test-provider -- <provider folder>
    npm test            # all Rust tests, including every sample's fixtures
    npm run check-js    # type-check the samples' extract.js

## Credit balances

The sidebar shows each provider's credit balance next to its name. It refreshes on launch,
every 15 minutes, and when you click ↻ at the bottom of the sidebar. Hover a provider for details. **Reload page** at the top of the sidebar reloads the open
console, for a page that has gone stale.

- For each provider with a metric, Rust opens an off-screen webview on its `page`. It shares
  the consoles' logins, records the JSON responses matching `captures`, then closes.
- `src-tauri/src/scrape.js` is injected into those background pages only, and holds no
  provider code. The provider's `extract.js` runs in a QuickJS sandbox in Rust
  (`src-tauri/src/providers.rs`): no network, no DOM, no app commands, 500 ms and 32 MB.
- "sign in" means the page redirected to a login. Open that console and log in again.
- "not found" means no balance came out, and the tooltip says why: the page never fetched
  a response matching `captures` (the console's API moved: fix `manifest.json`), or it did
  but `extract.js` found no balance in it (the response changed: fix `extract.js`), or
  `extract.js` threw. Find the new endpoint or shape with the dump below.
- A provider that breaks keeps showing its last balance, dimmed, with when it was read.

To see what a provider's page loads, use `npm run capture` (PLUGINS.md). For debugging
the app itself, run the binary with a dump directory:

    CONSOLEHUB_DUMP_DIR=/tmp/ch-dump src-tauri/target/debug/console-hub

Each provider writes `<id>.json` (page text, requests, all JSON responses, and which ones
matched `captures`) and `<id>.result.json`.
The dumps contain account data and session tokens. Delete them when done.

## Logins and Google sign-in (macOS)

On macOS, Google refuses to show its password page inside embedded web views ("This
browser or app may not be secure"). Once a Google session exists, sites that use Google
sign-in only show the account picker, which works. (Not yet tried on Windows or Linux.)

On macOS, the dev build and the installed app keep separate logins: the dev build under
`console-hub`, the installed app under its bundle ID `dev.consolehub.desktop`. If the
installed app ever needs Google's password page again, copy the dev build's logins over.
Quit the installed app first.

    rm -rf ~/Library/WebKit/dev.consolehub.desktop/WebsiteData
    cp -R ~/Library/WebKit/console-hub/WebsiteData ~/Library/WebKit/dev.consolehub.desktop/
    cp ~/Library/HTTPStorages/console-hub.binarycookies ~/Library/HTTPStorages/dev.consolehub.desktop.binarycookies

## Notes

- Each console is a real child webview (Tauri multiwebview, the `unstable` feature), so
  logins work as they do in a browser and framing restrictions don't apply.
- Layout lives in `src-tauri/src/main.rs` (`layout`, `show_vendor`). The sidebar is `ui/index.html`.
- All vendor webviews share the default web data store. Separate stores per vendor are not set up yet.
- The app icon is drawn in `src-tauri/icons/app-icon.svg`. After editing it, regenerate every
  size with `npx tauri icon src-tauri/icons/app-icon.svg`, then delete the `android` and `ios`
  folders it creates.

## License

MIT. See [LICENSE](LICENSE).
