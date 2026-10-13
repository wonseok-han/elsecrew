// Bundle the synthetic bridge before measurement, in a separate process so
// bundler/fixture allocations do not inflate the measured Electron process.
const { resolve, dirname, basename } = require('node:path');
const { execFileSync } = require('node:child_process');

// Match Buffer/TextEncoder for strings, without allocating a second full file.
function fixtureUtf8ByteLength(text) {
  let bytes = 0;
  for (let index = 0; index < text.length; index++) {
    const unit = text.charCodeAt(index);
    if (unit <= 0x7f) bytes++;
    else if (unit <= 0x7ff) bytes += 2;
    else if (unit >= 0xd800 && unit <= 0xdbff
      && text.charCodeAt(index + 1) >= 0xdc00 && text.charCodeAt(index + 1) <= 0xdfff) {
      bytes += 4; index++;
    } else bytes += 3;
  }
  return bytes;
}

function buildPerformancePreload(output) {
  execFileSync(process.execPath, [__filename, output], {
    env: { ...process.env, ELECTRON_RUN_AS_NODE: '1' }, stdio: 'pipe',
  });
  return output;
}
module.exports = { buildPerformancePreload, fixtureUtf8ByteLength };

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
          banner: `const __m5Utf8Length = ${fixtureUtf8ByteLength.toString()};` } },
      },
    });
  })().catch(error => { console.error(error); process.exitCode = 1; });
}
