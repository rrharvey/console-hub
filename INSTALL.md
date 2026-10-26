# Installing Console Hub

Console Hub runs on macOS, Windows and Linux. Most people should install a release.
Build from source if you write providers or change the app.

- [Install a release](#install-a-release): [macOS](#macos) · [Windows](#windows) · [Linux](#linux)
- [First run](#first-run)
- [Where things live](#where-things-live)
- [Build from source](#build-from-source)
- [Update](#update) · [Uninstall](#uninstall) · [Troubleshooting](#troubleshooting)
- [Smoke test](#smoke-test) (for anyone trying a new build on their OS)

An agent can walk you through any of this: point it at `.agent/skills/setup/SKILL.md`.

## Install a release

Download the installer for your OS from the repo's **Releases** page on GitHub. The builds
aren't code-signed, so each OS warns the first time you open the app. The steps below get
past that.

### macOS

1. Download `Console.Hub_<version>_aarch64.dmg` for Apple Silicon (M1 and later), or
   `..._x64.dmg` for an Intel Mac. Apple menu > About This Mac shows which you have.
2. Open the `.dmg` and drag **Console Hub** to **Applications**.
3. Open it. The app isn't signed with an Apple Developer ID, so macOS blocks the
   downloaded copy, usually saying it "is damaged and can't be opened". It isn't damaged.
   Run this once in Terminal, then open it again:

       xattr -dr com.apple.quarantine "/Applications/Console Hub.app"

   If macOS instead says it can't verify the developer, you can also go to System
   Settings > Privacy & Security and click **Open Anyway**.

### Windows

1. Download `Console.Hub_<version>_x64-setup.exe` (or the `.msi`, if you prefer).
2. Run it. If Windows shows "Windows protected your PC", click **More info**, then
   **Run anyway**.
3. Console Hub uses Microsoft Edge WebView2, which Windows 10 and 11 already have. If the
   app says WebView2 is missing, the installer offers to get it; or install the
   "Evergreen Bootstrapper" from Microsoft's WebView2 download page.

### Linux

Pick one:

- **AppImage** (any distro): `chmod +x Console.Hub_<version>_amd64.AppImage`, then run it.
  If it won't start, install FUSE 2: `libfuse2` on most distros (`libfuse2t64` on
  Ubuntu 24.04).
- **Debian/Ubuntu**: `sudo apt install ./Console.Hub_<version>_amd64.deb`
- **Fedora/RHEL**: `sudo dnf install ./Console.Hub-<version>-1.x86_64.rpm`

Then start **Console Hub** from your app launcher.

## First run

1. The sidebar is empty: no providers are built in. Click **+** at the bottom of the
   sidebar and choose the `samples` folder of a copy of this repo (or any provider
   folder), then **Allow**. To get the samples without git, download the repo as a ZIP
   from GitHub and unzip it.
2. Click each provider and sign in to its console as you normally would. Console Hub keeps
   you signed in, and reads each balance from the console in the background.
3. Balances refresh on launch, every 15 minutes, and when you click ↻. Hover one for
   details. "sign in" means that console's session expired; open it and sign in again.

Signing in with Google: on macOS, Google refuses to show its password page inside apps
("This browser or app may not be secure"). Sign in to Google once in a console that
allows it, or see "Logins and Google sign-in" in the README for copying a session over.
This hasn't been tried on Windows or Linux yet.

## Where things live

| | macOS | Windows | Linux |
|---|---|---|---|
| Your providers | `~/Library/Application Support/dev.consolehub.desktop/providers` | `%APPDATA%\dev.consolehub.desktop\providers` | `~/.local/share/dev.consolehub.desktop/providers` |
| Approvals (`settings.json`) | `~/Library/Application Support/dev.consolehub.desktop` | `%APPDATA%\dev.consolehub.desktop` | `~/.config/dev.consolehub.desktop` |
| Console logins | `~/Library/WebKit/dev.consolehub.desktop` (installed app), `~/Library/WebKit/console-hub` (dev build) | `%LOCALAPPDATA%\dev.consolehub.desktop` | `~/.local/share/dev.consolehub.desktop` |

Right-click any provider and choose **Show Providers Folder** to open it.

The dev build (`npm run dev`) and the installed app share providers and approvals on every
OS. They share console logins on Windows and Linux, but not on macOS. Don't run both at
once.

## Build from source

You need Git, [Node.js](https://nodejs.org) (LTS) and [Rust](https://rustup.rs), plus:

**macOS**: the Xcode Command Line Tools: `xcode-select --install`

**Windows**:
- [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  with the **Desktop development with C++** workload.
- Rust with the MSVC toolchain: `winget install --id Rustlang.Rustup`, then
  `rustup default stable-msvc`. Restart your terminal afterwards.
- WebView2 (already on Windows 10 and 11).
- Building the `.msi` needs VBScript: Settings > Apps > Optional features > More Windows
  features. Only needed if `npm run build` fails at `light.exe`.

**Linux** (from Tauri's prerequisites):

    # Debian/Ubuntu
    sudo apt update
    sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev \
      libssl-dev libayatana-appindicator3-dev librsvg2-dev

    # Fedora
    sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
      libappindicator-gtk3-devel librsvg2-devel libxdo-devel
    sudo dnf group install "c-development"

    # Arch
    sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl \
      appmenu-gtk-module libappindicator-gtk3 librsvg xdotool

Other distros: see https://v2.tauri.app/start/prerequisites/.

Then:

    git clone <this repo>
    cd console-hub
    npm install
    npm run dev       # run it
    npm test          # run the tests

`npm run build` makes the release app and its installers in
`src-tauri/target/release/bundle/`. On macOS it also installs the app into
`/Applications`; on Windows and Linux it prints where the installers are.

## Update

- **Release**: install the new version over the old one. Your providers, approvals and
  logins are kept.
- **From source**: `git pull`, `npm install`, then `npm run dev` or `npm run build`.
- **Providers**: they don't update with the app. To update one, click **+** and choose
  its folder again (for example `samples`), and allow it.

## Uninstall

- **macOS**: drag `/Applications/Console Hub.app` to the Trash.
- **Windows**: Settings > Apps > Installed apps > Console Hub > Uninstall.
- **Linux**: `sudo apt remove console-hub` or `sudo dnf remove console-hub`, or delete
  the AppImage.

Then delete the folders in [Where things live](#where-things-live) to remove your
providers, approvals and console logins.

## Troubleshooting

- **A balance says "not found".** Hover it: the tooltip says whether the console's API
  moved or its response changed. The provider needs updating (see PLUGINS.md).
- **A provider shows "review".** Its files changed since you allowed it, or it was
  dropped into the providers folder by hand. Click it to review and allow it.
- **Nothing happens when adding a folder.** Check the red message under the list. The
  folder needs a `manifest.json`, or folders inside it that each have one.
- **Linux: the window is blank or flickers.** Some GPU drivers (often NVIDIA) and WebKitGTK
  don't get along. Start it with `WEBKIT_DISABLE_DMABUF_RENDERER=1 console-hub`.
- **Windows: "WebView2 Runtime not found".** Install the Evergreen Bootstrapper from
  Microsoft's WebView2 page.
- **Building on Windows fails compiling C code.** The Desktop development with C++
  workload is missing; run the Build Tools installer again and add it.

## Smoke test

CI compiles and tests Console Hub on all three OSes, but only a person can check the
windows. When you try a build on an OS for the first time, run through this and report
what fails (with your OS version):

1. The app opens and shows the sidebar on the left; with no providers it says how to
   add them.
2. **+** > choose `samples` > the dialog lists seven providers > **Allow**. They appear in
   the sidebar.
3. Click OpenAI (or any console you use): its console fills the area right of the
   sidebar. Nothing is hidden under the window's title bar.
4. Sign in to that console. If it opens a sign-in popup (Google, GitHub), the popup
   appears, and closes once you're signed in.
5. Within a minute, that console's balance appears next to its name. Click ↻: it
   refreshes.
6. Right-click a provider: **Copy Link** copies the console's address, **Show Providers
   Folder** opens the folder, **Remove Provider…** removes it after asking.
7. Quit and reopen: you're still signed in, and the providers are still there.
