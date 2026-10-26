// Runs after `npm run build`. On macOS, installs the app into /Applications (replacing
// any earlier copy); on Windows and Linux, lists the installers the build made.
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";

const bundle = join("src-tauri", "target", "release", "bundle");

if (process.platform === "darwin") {
  const app = join(bundle, "macos", "Console Hub.app");
  const dest = "/Applications/Console Hub.app";
  rmSync(dest, { recursive: true, force: true });
  execFileSync("ditto", [app, dest]);
  console.log(`Installed ${dest}. Reopen Console Hub if it was running.`);
} else {
  const found = [];
  for (const kind of existsSync(bundle) ? readdirSync(bundle) : []) {
    for (const f of readdirSync(join(bundle, kind))) {
      if (/\.(msi|exe|deb|rpm|AppImage)$/.test(f)) found.push(join(bundle, kind, f));
    }
  }
  console.log(found.length ? "Installers:\n  " + found.join("\n  ") : `No installers found in ${bundle}.`);
  console.log("See INSTALL.md for how to install one.");
}
