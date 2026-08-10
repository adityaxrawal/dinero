/**
 * Vite build configuration for the Dinero desktop frontend.
 *
 * This app ships as a Tauri desktop binary rather than a website, which shapes
 * three decisions below: the dev server runs on a fixed port that the Rust side
 * expects, it never clears the terminal (the Rust compiler's output shares that
 * terminal and must stay readable), and it ignores the src-tauri tree so that
 * Rust rebuilds don't trigger pointless frontend hot-reloads.
 *
 * Serving over the network is opt-in via TAURI_DEV_HOST, which is how mobile
 * and cross-machine development targets reach the dev server.
 */
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';

// Set by the Tauri CLI when developing against a device that is not localhost.
// Left undefined for ordinary desktop development.
// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react()],
  resolve: {
    // Path aliases mirrored in tsconfig.json -- both files must be kept in sync,
    // since Vite resolves these at bundle time and tsc resolves them at check
    // time, and neither reads the other's mapping.
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
      '@components': fileURLToPath(new URL('./src/components', import.meta.url)),
      '@hooks': fileURLToPath(new URL('./src/hooks', import.meta.url)),
      '@types': fileURLToPath(new URL('./src/types', import.meta.url)),
      '@stores': fileURLToPath(new URL('./src/stores', import.meta.url)),
    },
  },

  // Keep Rust compiler diagnostics on screen -- Vite and cargo share one
  // terminal during `npm run dev`, and clearing would eat build errors.
  clearScreen: false,
  server: {
    // Fixed port: the Tauri shell loads this exact URL, so silently failing over
    // to another port would leave the desktop window pointing at nothing.
    port: 1420,
    strictPort: true,
    host: host || false,
    // Over a network host the HMR socket needs an explicit address, since the
    // page is no longer served from localhost and cannot infer one.
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Rust sources are cargo's concern; watching them would fire frontend
      // reloads on every backend rebuild.
      ignored: ['**/src-tauri/**'],
    },
  },
}));
