/**
 * Ambient type declarations for Vite's client environment.
 *
 * The reference directive below is load-bearing rather than decorative: it pulls
 * in Vite's client types, which is what gives `import.meta.env` a type at all.
 * Removing it does not fail here -- it fails at every call site that reads
 * `import.meta.env.DEV`, with an error that points at those files rather than at
 * this one.
 *
 * A triple-slash directive must precede any statement in its file. Comments are
 * explicitly permitted before it, so this block is safe; code above it would not
 * be.
 */
/// <reference types="vite/client" />
