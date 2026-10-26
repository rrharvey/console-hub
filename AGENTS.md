# Console Hub: notes for agents

A Tauri 2 desktop app (Rust + a plain HTML sidebar) for macOS, Windows and Linux: one
window with a sidebar of AI vendor consoles, each showing its credit balance. Providers
(one per console) are plugins the user adds and allows; none are built in.

## Skills

Read the matching skill before starting one of these tasks. Each is a `SKILL.md` with the
workflow, the commands and the reasons behind its rules.

| Task | Skill |
|---|---|
| Install, build, run or troubleshoot the app on any OS; help a teammate set up | `.agent/skills/setup/SKILL.md` |
| Add, fix or review a provider (a console's `manifest.json` + `extract.js`) | `.agent/skills/write-provider/SKILL.md` |
| Version bump, installers, release workflow, local builds | `.agent/skills/release/SKILL.md` |

## Map

- `src-tauri/src/main.rs`: window layout, sidebar commands, balance scraping, approvals.
- `src-tauri/src/providers.rs`: loading, validating and sandboxing providers (QuickJS).
- `src-tauri/src/sandbox.js`, `fixture.js`: JS that runs inside the sandbox.
- `src-tauri/src/scrape.js`: injected into hidden scraper pages; records responses.
- `src-tauri/src/cli.rs`, `capture.rs`: author tools (`npm run test-provider`,
  `fixture`, `capture`).
- `ui/index.html`: the sidebar.
- `samples/`: eight sample providers, with fixtures for those that read a balance.
  `plugin-api/`: manifest schema, types.
- Docs: `README.md`, `INSTALL.md` (per OS), `PLUGINS.md` (provider authors).

## Commands

```
npm install
npm test                          # all Rust tests, incl. every sample's fixtures
npm run check-js                  # type-check samples/*/extract.js
npm run test-provider -- samples  # validate + run fixtures, like the app does
npm run dev                       # run the app
```

Run `npm test` after any change to `src-tauri/` or `samples/`. CI runs the same on all
three OSes.

## Ground rules

- Account data stays out of the repo: no captures, no real balances or IDs in fixtures,
  tests or commit messages (details in the write-provider skill).
- Provider code only ever runs in the sandbox, never in a webview. Keep it that way.
- Code must build on macOS, Windows and Linux. Gate OS-specific code with `cfg`, and don't
  shell out to OS tools when a Tauri plugin does the job.
- Ask before pushing tags, publishing releases, or anything that rewrites shared history.
