export {};

declare global {
  interface Window {
    m5: Readonly<{
      version: 1;
    }>;
  }
}
