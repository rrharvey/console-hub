---
name: setup
description: Install, build, run or troubleshoot Console Hub on macOS, Windows or Linux. Use this whenever someone wants to install Console Hub, get it running on their machine, set up a development environment for it, fix a failed `npm install` / `npm run dev` / `npm run build`, or figure out why the app won't start, shows a blank window, can't find WebView2, or warns that it's unsigned. Also use it to walk a teammate through first run (adding the sample providers) or the smoke test on a new OS.
---

# Set up Console Hub

The facts (commands, package lists, file locations) live in `INSTALL.md` at the repo
root. Read the section for the person's OS before acting; this skill is how to apply it.

## 1. Find out what they need

Ask, or infer from what they said:

- **Just use the app** → install a release (INSTALL.md "Install a release"). Quicker, and
  needs no toolchain.
- **Write providers or change the app** → build from source (INSTALL.md "Build from
  source"). The author tools (`npm run capture`, `fixture`, `test-provider`) only run from
  a source checkout.

Detect the OS rather than guessing: `uname -s` (Darwin / Linux) or, in PowerShell,
`$env:OS` (Windows_NT). On macOS, `uname -m` says `arm64` (Apple Silicon) or `x86_64`
(Intel), which decides which `.dmg` to download.

## 2. Check prerequisites before installing anything

Check first, so you only install what's missing:

| Check | Command |
|---|---|
| Node.js LTS | `node --version` |
| Rust 1.90+ | `rustc --version` and `cargo --version`; if older, `rustup update` |
| Windows: MSVC toolchain | `rustup show` lists a `-pc-windows-msvc` default |
| Windows: C++ Build Tools | `"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property displayName` prints something |
| Linux: WebKitGTK | `pkg-config --modversion webkit2gtk-4.1` |
| macOS: Command Line Tools | `xcode-select -p` |

Installing system packages (`sudo apt install`, `dnf`, the Visual Studio Build Tools,
`xcode-select --install`) changes the person's machine and may prompt for a password.
Show the exact command from INSTALL.md and let them run it, or ask before running it.

## 3. Build and run

From the repo root:

```
npm install
npm test        # proves the toolchain works: compiles the app and runs the unit tests
npm run dev     # opens the app
```

Run `npm test` before `npm run dev`. A failure there points at a toolchain problem
(missing C compiler, WebKitGTK headers) in plain compiler output, rather than at a window
that never appears.

`npm run build` makes installers in `src-tauri/target/release/bundle/`. On macOS it also
replaces `/Applications/Console Hub.app`; tell the person to reopen the app afterwards.

## 4. First run

The sidebar starts empty: no providers are built in. Have them click **+**, choose the
repo's `samples` folder and **Allow**, then sign in to each console in the app. Balances
appear within a minute. See INSTALL.md "First run".

## 5. When something fails

Match the symptom to INSTALL.md "Troubleshooting" first. Common ones:

| Symptom | Likely cause |
|---|---|
| Windows build: `link.exe` not found, or errors compiling C (QuickJS) | C++ Build Tools missing the "Desktop development with C++" workload |
| Windows build: `failed to run light.exe` | VBScript optional feature off (only the `.msi` needs it) |
| Linux build: `webkit2gtk-4.1` / `javascriptcoregtk` not found | WebKitGTK dev packages missing |
| Linux: window blank or flickering | GPU driver vs WebKitGTK; start with `WEBKIT_DISABLE_DMABUF_RENDERER=1` |
| Linux AppImage won't start | FUSE 2 missing (`libfuse2`, or `libfuse2t64` on Ubuntu 24.04) |
| macOS: "is damaged and can't be opened" | Unsigned (ad-hoc) build, quarantined on download: `xattr -dr com.apple.quarantine "/Applications/Console Hub.app"`. For "can't verify the developer", Privacy & Security > Open Anyway also works |
| Windows: "Windows protected your PC" | Unsigned build; More info > Run anyway |
| A balance says "sign in" | That console's session expired; open it in the app and sign in |
| A balance says "not found" | The provider needs updating; hover it for why, then see the write-provider skill |

## Things to be careful with

- The data folders in INSTALL.md "Where things live" hold the person's console logins and
  their approved providers. Don't delete them to "reset" anything without asking; signing
  back in to every console is a real cost.
- Don't run the dev build and the installed app at the same time. On Windows and Linux
  they share a login store.
- If you're the first to run Console Hub on an OS, go through INSTALL.md "Smoke test" with
  the person and record what passed and failed. CI only proves it compiles and the unit
  tests pass there; the windows, sign-in popups and background balance reading are only
  checked by a person.
