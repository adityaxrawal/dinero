import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';

// K1 fix: no frontend unit-test layer existed at all — only Playwright E2E
// specs (e2e/), despite an 80%-line-coverage target. Kept as its own config
// (rather than merged into vite.config.ts) since that file's Tauri-specific
// dev-server settings (fixed port, HMR host) don't apply to the test runner.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    // `**/dist/**`: `licensing-backend/dist/` holds compiled `.js` output of
    // tests whose sources vitest already picks up. Without this, `npm test`
    // reports 20 failed files on a clean checkout — the compiled copies
    // resolve their `require("../api/...")` paths relative to `dist/` and
    // fail to load. They were never meant to be collected, and a permanently
    // red suite is a suite nobody reads.
    exclude: ['**/node_modules/**', '**/e2e/**', '**/src-tauri/**', '**/dist/**'],
  },
});
