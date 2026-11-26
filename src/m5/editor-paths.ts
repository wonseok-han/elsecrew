/** Pure path helpers for the editor and comparison views. */
/** Documents shown drawn and in a proportional font by default. */
export function isProsePath(path: string): boolean {
  return /\.(?:md|markdown|mdx)$/i.test(path);
}

export function fileName(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}
