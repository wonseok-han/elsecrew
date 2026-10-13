// Bundle the synthetic bridge before measurement, in a separate process so
// bundler/fixture allocations do not inflate the measured Electron process.
const { resolve, dirname, basename } = require('node:path');
const { execFileSync } = require('node:child_process');

function buildPerformancePreload(output) {
  execFileSync(process.execPath, [__filename, output], {
    env: { ...process.env, ELECTRON_RUN_AS_NODE: '1' }, stdio: 'pipe',
  });
  return output;
}
module.exports = { buildPerformancePreload };

if (require.main === module) {
  (async () => {
    const output = resolve(process.argv[2]);
    const { createHash } = require('node:crypto');
    const { getFileText } = require('./m5-renderer-performance-fixture.cjs');
    const version = 'sha256:' + createHash('sha256').update(getFileText()).digest('hex');
    const { build } = await import('vite');
    await build({ configFile: false, logLevel: 'error',
      define: { __M5_PERF_FILE_VERSION__: JSON.stringify(version), 'Buffer.byteLength': '__m5Utf8Length' },
      build: { outDir: dirname(output), emptyOutDir: false, minify: false,
        lib: { entry: resolve(__dirname, 'm5-renderer-performance-preload.cjs'), formats: ['cjs'], fileName: () => basename(output) },
        rollupOptions: { external: ['electron'], output: { inlineDynamicImports: true,
          banner: 'const __m5Utf8Length = text => new TextEncoder().encode(text).byteLength;' } },
      },
    });
  })().catch(error => { console.error(error); process.exitCode = 1; });
}
