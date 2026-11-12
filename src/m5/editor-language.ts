/** Local syntax selection only; this does not start language servers or tools. */
export function languageFor(path: string): string {
  const name = path.split(/[\\/]/).pop()?.toLowerCase() ?? "";
  if (name === "dockerfile" || name.startsWith("dockerfile.")) return "dockerfile";
  if ([".bashrc", ".bash_profile", ".zshrc", ".zprofile"].includes(name)) return "shell";
  const extension = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1) : "";
  return ({
    ts: "typescript", tsx: "typescript", mts: "typescript", cts: "typescript",
    js: "javascript", jsx: "javascript", mjs: "javascript", cjs: "javascript",
    json: "json", jsonc: "json", md: "markdown", markdown: "markdown",
    py: "python", pyi: "python", rs: "rust", go: "go", sh: "shell", bash: "shell", zsh: "shell",
    yaml: "yaml", yml: "yaml", css: "css", html: "html", htm: "html", sql: "sql",
  } as Record<string, string>)[extension] ?? "plaintext";
}
