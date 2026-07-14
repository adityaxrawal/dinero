# Dinero App

This is a local-first financial data extraction and reconciliation application built with Tauri, React, and Typescript.

## Local Development Setup

### Prerequisites

- **macOS ≥ 13 (Ventura)** — this is a macOS-only desktop app.
- **Rust**: `rustup update stable`.
- **Node.js ≥ 20**, **pnpm** 9.
- **Xcode Command Line Tools**: `xcode-select --install` — required for the macOS SDK headers `rusqlite`'s SQLCipher build needs.

### Installation

1. Navigate to the `dinero-app` directory.
2. Install dependencies:
   ```bash
   pnpm install
   ```

### Running the App

To run the application in development mode (which starts both the Vite server and the Tauri rust backend):

```bash
pnpm tauri dev
```

A macOS Keychain permission prompt on first run is expected, not an error — Dinero stores its encryption key and OAuth tokens exclusively in Keychain.

**See [`docs/dev-setup.md`](docs/dev-setup.md)** for: Google OAuth development credential setup, resetting the local database, and the full list of privacy invariants (what must never reach the network) that hold in every build, dev included.

### Formatting & Linting

To check the frontend codebase for errors:

```bash
pnpm lint       # Runs ESLint
pnpm typecheck  # Runs TypeScript compiler check
pnpm format     # Runs Prettier and formats code
```

To check the Rust backend:

```bash
cd src-tauri
cargo fmt
cargo clippy
```

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
- ESLint and Prettier extensions are highly recommended for VS Code to get real-time linting and formatting.
