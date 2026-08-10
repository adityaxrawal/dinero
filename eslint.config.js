/**
 * ESLint flat configuration for the TypeScript/React frontend.
 *
 * Scope is deliberately narrow -- only .ts and .tsx files are linted, and build
 * output under dist/ is ignored. Formatting is not handled here at all; Biome
 * owns that (see biome.json and the `format` npm script), so this config is
 * concerned purely with correctness rules.
 *
 * The rule block layers three sources: the recommended JS and TypeScript sets
 * pulled in via `extends`, the React Hooks rules spread in below, and finally a
 * handful of project-specific overrides that tighten or relax those defaults.
 */
import js from '@eslint/js';
import globals from 'globals';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  { ignores: ['**/dist/**'] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2020,
      // Frontend runs inside the Tauri webview, so browser globals apply.
      globals: globals.browser,
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      // Baseline hook correctness: dependency arrays and call-order rules.
      ...reactHooks.configs.recommended.rules,

      // Fast Refresh only preserves state when a module exports components
      // alone; constant exports are permitted since they cannot break it.
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true }],

      // Disabled deliberately -- this codebase uses effect-driven setState in
      // places where the value genuinely derives from an external subscription.
      'react-hooks/set-state-in-effect': 'off',

      // `any` is an error, not a warning: the IPC boundary to Rust is where
      // types would otherwise quietly erode.
      '@typescript-eslint/no-explicit-any': 'error',

      // Unused bindings stay a warning so work in progress is not blocked.
      '@typescript-eslint/no-unused-vars': 'warn',
    },
  }
);
