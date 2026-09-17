import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { prepareNativeDependencies } from "./prepare-native.mjs";
import { nativeBuildEnvironment } from "./tool-env.mjs";

prepareNativeDependencies();
const cli = join(process.cwd(), "node_modules", "@tauri-apps", "cli", "tauri.js");
const result = spawnSync(process.execPath, [cli, ...process.argv.slice(2)], {
  env: nativeBuildEnvironment(),
  stdio: "inherit",
});

if (result.error) {
  console.error(result.error.message);
}

process.exit(result.status ?? 1);
