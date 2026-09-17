import { copyFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

export function prepareNativeDependencies() {
  if (process.platform !== "win32") {
    console.log("Using the operating system SQLite dynamic library.");
    return;
  }

  const kitsRoot = "C:\\Program Files (x86)\\Windows Kits\\10";
  const libraryRoot = join(kitsRoot, "Lib");
  const versions = readdirSync(libraryRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort((left, right) => right.localeCompare(left, undefined, { numeric: true }));

  const version = versions.find((candidate) =>
    existsSync(join(libraryRoot, candidate, "um", "x64", "winsqlite3.lib")),
  );
  if (!version) {
    throw new Error("Windows SDK winsqlite3.lib was not found.");
  }

  const sourceLibrary = join(libraryRoot, version, "um", "x64", "winsqlite3.lib");
  const sourceHeader = join(
    kitsRoot,
    "Include",
    version,
    "um",
    "winsqlite",
    "winsqlite3.h",
  );
  if (!existsSync(sourceHeader)) {
    throw new Error(`Windows SDK winsqlite3.h was not found for ${version}.`);
  }

  const destinationRoot = join(
    process.cwd(),
    "native",
    "sqlite",
    "windows-x64",
  );
  const libraryDirectory = join(destinationRoot, "lib");
  const includeDirectory = join(destinationRoot, "include");
  mkdirSync(libraryDirectory, { recursive: true });
  mkdirSync(includeDirectory, { recursive: true });

  // The renamed import library still points to the precompiled winsqlite3.dll.
  copyFileSync(sourceLibrary, join(libraryDirectory, "sqlite3.lib"));
  copyFileSync(sourceHeader, join(includeDirectory, "sqlite3.h"));
  console.log(`Prepared precompiled Windows SQLite from SDK ${version}.`);
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  prepareNativeDependencies();
}
