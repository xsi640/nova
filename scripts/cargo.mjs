import { spawnSync } from "node:child_process";
import { prepareNativeDependencies } from "./prepare-native.mjs";
import { nativeBuildEnvironment } from "./tool-env.mjs";

prepareNativeDependencies();
const result = spawnSync("cargo", process.argv.slice(2), {
  env: nativeBuildEnvironment(),
  stdio: "inherit",
});

if (result.error) {
  console.error(result.error.message);
}

process.exit(result.status ?? 1);
