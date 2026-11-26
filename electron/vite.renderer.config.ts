import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "src", "m5");
const appVersion = JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "..", "package.json"), "utf8")).version as string;
const outputDirectory = resolve(dirname(fileURLToPath(import.meta.url)), "..", ".vite", "renderer", "main_window");

export default defineConfig({
  root,
  plugins: [react()],
  // Shown in Settings so people can tell which build they run.
  define: { __ELSECREW_VERSION__: JSON.stringify(appVersion) },
  server: { headers: { "Content-Security-Policy": "frame-ancestors 'none'" } },
  preview: { headers: { "Content-Security-Policy": "frame-ancestors 'none'" } },
  build: {
    // Forge packages only the project-root .vite directory. `root` is the
    // isolated M5 renderer source directory, so this must remain absolute.
    outDir: outputDirectory,
    emptyOutDir: true,
  },
});
