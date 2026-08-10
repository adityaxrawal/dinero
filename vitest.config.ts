/**
 * Vitest configuration for the frontend unit and component test suite.
 *
 * Kept separate from vite.config.ts because the two builds want different
 * things: the app build targets the Tauri webview, while tests run headless in
 * jsdom with a setup file that stubs the Tauri IPC bridge.
 *
 * The exclusion list is the important part. Three other test systems live in
 * this repo -- Playwright specs under e2e/, Rust tests inside src-tauri/, and
 * whatever is already built into dist/ -- and none of them can execute under
 * Vitest, so all three are filtered out of both the run and the coverage report.
 */
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';

export default defineConfig({
  plugins: [react()],
  resolve: {
    // Only the '@' root alias is needed here; tests import through it rather
    // than the finer-grained aliases the app build defines.
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  test: {
    // Components render against a simulated DOM -- there is no real browser.
    environment: 'jsdom',
    // Exposes describe/it/expect without a per-file import.
    globals: true,
    // Installs shared fakes (notably the Tauri IPC layer) before any test runs.
    setupFiles: ['./tests/setup.ts'],
    exclude: ['**/node_modules/**', '**/e2e/**', '**/src-tauri/**', '**/dist/**'],
    coverage: {
      provider: 'v8',
      reporter: ['text-summary', 'json'],
      // Config files and the test harness itself are excluded so the percentage
      // reflects application code only.
      exclude: ['**/node_modules/**', '**/e2e/**', '**/src-tauri/**', '**/dist/**', '**/*.config.*', 'tests/**'],
    },
  },
});
