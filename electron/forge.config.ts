import { FuseV1Options, FuseVersion } from "@electron/fuses";
import { MakerZIP } from "@electron-forge/maker-zip";
import { FusesPlugin } from "@electron-forge/plugin-fuses";
import { VitePlugin } from "@electron-forge/plugin-vite";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { assertPackageNotRunning } from "../scripts/m5-package-live-guard.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export default {
  packagerConfig: {
    asar: true,
  },
  makers: [new MakerZIP({}, ["darwin", "win32", "linux"])],
  plugins: [
    new VitePlugin({
      build: [
        { entry: "electron/main.ts", config: "electron/vite.main.config.ts" },
        { entry: "electron/preload.ts", config: "electron/vite.preload.config.ts" },
      ],
      renderer: [{ name: "main_window", config: "electron/vite.renderer.config.ts" }],
    }),
    new FusesPlugin({
      version: FuseVersion.V1,
      strictlyRequireAllFuses: true,
      [FuseV1Options.RunAsNode]: false,
      [FuseV1Options.EnableCookieEncryption]: false,
      [FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false,
      [FuseV1Options.EnableNodeCliInspectArguments]: false,
      [FuseV1Options.OnlyLoadAppFromAsar]: true,
      [FuseV1Options.EnableEmbeddedAsarIntegrityValidation]: true,
      [FuseV1Options.LoadBrowserProcessSpecificV8Snapshot]: false,
      [FuseV1Options.GrantFileProtocolExtraPrivileges]: false,
      [FuseV1Options.WasmTrapHandlers]: true,
    }),
  ],
  hooks: {
    prePackage: async (_config: unknown, platform: string, arch: string) => {
      assertPackageNotRunning(resolve(root, "out", `hello-agent-${platform}-${arch}`));
    },
    packageAfterCopy: async (_config: unknown, buildPath: string) => {
      const resourcesDirectory = resolve(buildPath, "..");
      const stage = spawnSync(process.execPath, ["scripts/m5-stage-sidecar.mjs", resourcesDirectory], {
        cwd: root,
        stdio: "inherit",
      });
      if (stage.status !== 0 || stage.error) {
        throw stage.error ?? new Error(`sidecar staging failed with exit ${String(stage.status)}`);
      }
    },
  },
};
