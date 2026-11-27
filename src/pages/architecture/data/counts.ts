/**
 * Every number the architecture section states about the code, in one place.
 * model.test.ts recomputes each from the source (migrations, capabilities,
 * events, db/ files) and fails if one drifts (audit G).
 */
export const COUNTS = {
  /** Base tables after all migrations (excluding the FTS5 index and its shadow tables). */
  tables: 46,
  migrations: 81,
  triggers: 17,
  /** Entries in capabilities/app-commands.json (generated from generate_handler!). */
  commands: 152,
  debugCommands: 8,
  /** Commands that call assert_write_allowed (every other one is on UNGATED with a reason). */
  gatedCommands: 51,
  dbSourceFiles: 63,
  appEvents: 26,
  /** TanStack query and mutation hook call sites in src/ (excluding tests). */
  queryHooks: 25,
  mutationHooks: 14,
} as const;

/** Commands available in release builds (debug_* commands are debug-only). */
export const RELEASE_COMMANDS = COUNTS.commands - COUNTS.debugCommands;
