import { existsSync } from "node:fs";
import { join } from "node:path";

export function nativeBuildEnvironment() {
  const env = { ...process.env };

  if (process.platform === "win32") {
    const dependencyRoot = join(process.cwd(), "native", "sqlite", "windows-x64");
    const libraryDirectory = join(dependencyRoot, "lib");
    const includeDirectory = join(dependencyRoot, "include");

    if (existsSync(libraryDirectory)) {
      env.SQLITE3_LIB_DIR = libraryDirectory;
      env.SQLITE3_INCLUDE_DIR = includeDirectory;
      env.SQLITE3_STATIC = "0";
    }
  }

  return env;
}
