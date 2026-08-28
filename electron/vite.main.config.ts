import { defineConfig } from "vite";

export default defineConfig(({ forgeConfigSelf }) => ({
  build: {
    lib: {
      entry: forgeConfigSelf.entry,
      formats: ["cjs"],
      fileName: (_format, entryName) => `${entryName}.cjs`,
    },
  },
}));
