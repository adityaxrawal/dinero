/**
 * PostCSS pipeline, consumed automatically by Vite for every stylesheet.
 *
 * Order matters: Tailwind runs first to expand its directives and generate the
 * utility classes actually used in the source, then autoprefixer adds vendor
 * prefixes to whatever CSS came out the other side.
 */
export default {
  plugins: {
    tailwindcss: {},
    autoprefixer: {},
  },
};
