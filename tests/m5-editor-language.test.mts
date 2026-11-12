import assert from "node:assert/strict";
import test from "node:test";
import { languageFor } from "../src/m5/editor-language.ts";

test("editor selects local syntax by exact filename or extension", () => {
  for (const [path, expected] of [
    ["src/main.rs", "rust"], ["src/app.PY", "python"], ["types.pyi", "python"],
    ["src/app.mts", "typescript"], ["index.cts", "typescript"], ["view.tsx", "typescript"],
    ["lib.mjs", "javascript"], ["main.cjs", "javascript"], ["view.jsx", "javascript"],
    ["config.json", "json"], ["tsconfig.jsonc", "json"], ["README.MD", "markdown"],
    ["main.go", "go"], [".github/workflows/test.yml", "yaml"], ["a.yaml", "yaml"],
    ["script.sh", "shell"], ["home/.zshrc", "shell"], ["Dockerfile", "dockerfile"],
    ["build/Dockerfile.dev", "dockerfile"], ["C:\\src\\main.rs", "rust"],
    ["style.css", "css"], ["index.html", "html"], ["schema.sql", "sql"],
    ["Cargo.toml", "plaintext"], ["data.bin", "plaintext"], ["README", "plaintext"],
    ["directory.rs/file", "plaintext"], ["script.py.bak", "plaintext"],
  ]) assert.equal(languageFor(path!), expected, path);
});
