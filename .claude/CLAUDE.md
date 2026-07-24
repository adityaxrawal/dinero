# Project Tooling

This project has three Claude Code integrations installed. Their hooks/plugins already do the actual enforcement — this file documents the intended workflow so plans, answers, and searches stay consistent with it.

## Codebase analysis — graphify

- Before reading raw source files to answer a structure/architecture/dependency question, run `graphify query <topic>` first and lead with that result instead of grepping cold.
- For broad questions ("how does X work", "what depends on Y", "where does this data flow"), check `graphify-out/GRAPH_REPORT.md` before exploring the repo manually.
- The knowledge graph refreshes itself via git hooks after each commit — you shouldn't normally need to run `graphify update` yourself. If a query looks stale relative to very recent uncommitted changes, say so rather than trusting it silently.

## Code style — ponytail

- Default to the most minimal implementation that satisfies the requirement. No speculative abstraction, no config options nobody asked for, no wrapper layers "for future flexibility."
- Before adding a new component, dependency, or utility, check whether something already in the codebase (or native language/stdlib) already covers it.
- Available commands: `/ponytail-review` (diff-level over-engineering check), `/ponytail-audit` (whole-repo audit), `/ponytail-debt` (harvest deferred shortcuts into a ledger), `/ponytail-gain` (impact report), `/ponytail-help`.

## Token/output compression — headroom

- Session traffic already routes through the local headroom proxy — no action needed for this to take effect.
- If a tool result looks compressed in a way that's missing detail you actually need, call `headroom_retrieve` to pull the original uncompressed content back rather than re-running the command or guessing.
