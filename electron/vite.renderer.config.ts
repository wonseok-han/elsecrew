import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "src", "m5");
const outputDirectory = resolve(dirname(fileURLToPath(import.meta.url)), "..", ".vite", "renderer", "main_window");

export default defineConfig({
  root,
  plugins: [react()],
  server: { headers: { "Content-Security-Policy": "frame-ancestors 'none'" } },
  preview: { headers: { "Content-Security-Policy": "frame-ancestors 'none'" } },
  build: {
    // Forge packages only the project-root .vite directory. `root` is the
    // isolated M5 renderer source directory, so this must remain absolute.
    outDir: outputDirectory,
    emptyOutDir: true,
  },
});
