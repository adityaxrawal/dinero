# Dinero

<div align="center">

  **Privacy-Native Financial Data Extraction & Reconciliation Engine**

  *Privacy by Architecture, Not Policy*

  [![Platform](https://img.shields.io/badge/Platform-macOS%20%E2%89%A5%2013-black?style=flat-square&logo=apple)](https://www.apple.com/macos)
  [![Framework](https://img.shields.io/badge/Shell-Tauri%20v2-blue?style=flat-square&logo=tauri)](https://tauri.app)
  [![Backend](https://img.shields.io/badge/Backend-Rust%202021-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
  [![Frontend](https://img.shields.io/badge/Frontend-React%2019%20%7C%20TypeScript-61dafb?style=flat-square&logo=react)](https://react.dev)
  [![Security](https://img.shields.io/badge/Storage-SQLCipher%20AES--256-green?style=flat-square&logo=sqlite)](https://www.zetetic.net/sqlcipher/)
  [![Privacy](https://img.shields.io/badge/Privacy-100%25%20Offline%20Data%20Plane-brightgreen?style=flat-square)](#-privacy-model--security-invariants)

</div>

---

## 📌 Executive Summary

**Dinero** is a privacy-first, local-first financial data extraction, statement parsing, tracking, and reconciliation desktop application built for macOS.

Unlike cloud-dependent personal finance tools that require uploading sensitive banking details to third-party servers, Dinero operates on a **zero-cloud financial data path philosophy**. All financial data—transactions, balances, bank statements, and derived analytics—remains encrypted locally on your Mac inside a **SQLCipher-encrypted SQLite database**, with encryption keys anchored in **macOS Keychain**.

Dinero automatically ingests transaction alerts and bank PDF statements from read-only Gmail integration or manual uploads, extracts structured financial data using fast regex rules with local LLM fallback (`llama.cpp`), auto-discovers financial instruments, and reconciles all data into a canonical transaction timeline.

---

## 🗺️ Release Roadmap & Delivery Milestones

Dinero follows a structured 7-phase delivery model, progressing systematically from low-level data layer foundations to production-grade macOS desktop distribution:

| Phase | Milestone | Focus & Core Deliverables | Status | Target Release |
| :---: | :--- | :--- | :---: | :---: |
| **Phase 1** | **Foundation & Data Layer** | Tauri v2 desktop shell, Rust IPC layer, SQLCipher AES-256 database, `deadpool-sqlite` connection pool, macOS Keychain secret storage. | `Completed` | **Alpha v0.1** |
| **Phase 2** | **Ingestion & Extraction** | Gmail smart polling (OAuth2 PKCE, up to 10 accounts), 4-worker Transaction Queue, 6-layer extraction ladder, local LLM fallback (`llama.cpp` sidecar). | `Completed` | **Alpha v0.2** |
| **Phase 3** | **Reconciliation Engine** | Deterministic matching engine, candidate scoring, ambiguity clustering, canonical transaction ledger, statement-over-email precedence rules. | `Completed` | **Alpha v0.3** |
| **Phase 4** | **Statement Coverage** | PDFium render sidecar, in-memory PDF statement parsing, password-protected PDF handler, mandatory Statement Instrument Gate. | `Completed` | **Beta v0.8** |
| **Phase 5** | **Frontend Experience** | React 19 UI, Dashboard analytics, Transactions list, Statement staging & review modal (`statement_drafts`), Instrument picker, Settings console. | `Completed` | **Beta v0.9** |
| **Phase 6** | **Security & Monetization** | Hardware UUID device binding, Razorpay subscription backend, 7-day offline grace mode, tamper-evident audit logs, PII sanitizer. | `Completed` | **Beta v0.9.5** |
| **Phase 7** | **Release & Delivery** | macOS DMG packaging, Apple Code Signing & Notarization, GitHub Release workflows, auto-update distribution (`tauri-plugin-updater`). | 🔄 `In Progress` | **Stable v1.0** |

---

## ✨ Key Capabilities & Architectural Highlights

### 🛡️ Privacy-First Architecture
- **Zero Cloud Financial Data Path**: 100% of your transactions, accounts, and PDF statements stay on your machine. Financial data is never sent to any cloud server or LLM API.
- **AES-256 Encryption at Rest**: Data is stored in SQLite encrypted via SQLCipher (`bundled-sqlcipher`).
- **macOS Keychain Integration**: Master encryption keys, Gmail OAuth tokens, and session credentials reside exclusively in macOS Keychain via native system bindings.

### 📬 Automated Gmail Financial Ingestion
- **Read-Only OAuth2 PKCE**: Secure Google OAuth authorization to poll transaction alerts, bank emails, and PDF statement attachments across up to **10 connected Gmail accounts**.
- **Sanitized HTML Rendering**: Email body previews inside password modals use `ammonia` HTML sanitization with mandatory image stripping to eliminate tracking pixels.

### ⚡ Dual-Queue Ingestion Engine
- **Transaction Queue**: 4 parallel Rust worker threads consuming classified transaction alert emails (SMS/UPI/Card spend notifications).
- **Statement Queue**: Bounded worker pool processing bank and credit card PDF statements in memory.

### 🤖 Local AI Extraction & In-Memory PDF Engine
- **In-Memory Statement Parsing**: PDFs are processed in memory using `pdfium-render` and regex rules—raw PDF statements are never persisted unencrypted to disk.
- **Apple Silicon Accelerated Local LLM**: When complex or unstructured statements require AI fallback, Dinero invokes a local `.gguf` quantized model via a loopback-bound `llama-server` (`llama.cpp`) sidecar, running directly on Apple Silicon / Metal GPU. Zero third-party LLM API calls!

### ⚖️ Unified Reconciliation Engine
- All ingestion streams—Transaction Queue, Statement Queue, and Manual Entry—pass through **one single, deterministic reconciliation entry point**.
- Prevents duplicate entries, cross-verifies email notifications against official bank statements, and maintains an auditable observation ledger.

### 🔍 Auto-Discovered Instruments & Statement Instrument Gate
- **Automatic Instrument Discovery**: Bank accounts and credit cards are auto-created and mapped on first sight using `(type, issuer_name, masked_identifier)`.
- **Mandatory Statement Instrument Gate**: Blocks statement ingestion when account identity cannot be resolved with high confidence, asking the user to confirm the instrument rather than making guessed linkages.

### 📝 Two-Phase Interactive Statement Review
- Ingestion extracts statement data into a transient `statement_drafts` staging table.
- Users review and edit extracted line items in an interactive UI modal prior to executing an atomic `commit_staged_draft` transaction into the canonical store.

---

## 🏗️ System Architecture & Data Flow

```text
               +-------------------------------------------------+
               |             React 19 WebView UI                 |
               +-------------------------------------------------+
                                        |
                             Tauri v2 IPC & Events
                                        |
               +-------------------------------------------------+
               |                Rust Core Process                |
               |  - Auth & Keychain Manager                      |
               |  - Gmail Read-Only Poller & Ingestion Queue     |
               |  - Transaction Queue (4 Parallel Workers)       |
               |  - Statement Queue (Bounded PDF Parse Workers)  |
               |  - Local LLM Sidecar (llama-server / Metal GPU) |
               |  - Unified Reconciliation Engine                |
               +-------------------------------------------------+
                         /              |              \
                        /               |               \
                       v                v                v
          +------------------+  +---------------+  +-------------------+
          | SQLite + SQLCipher|  | macOS Keychain|  | Staged Drafts UI  |
          | (Encrypted DB)   |  | (Secrets & K) |  | (Statement Review)|
          +------------------+  +---------------+  +-------------------+
```

### 🔒 Permitted Network Egress Boundaries

Dinero enforces a strict, fail-closed network egress model. Only **5 narrow, non-financial network boundaries** are permitted system-wide (all initiated strictly from the Rust core, never from the frontend WebView):

1. **Gmail API**: Read-only email and attachment polling (`https://gmail.googleapis.com`).
2. **Google OAuth Servers**: OAuth2 PKCE handshake (`https://oauth2.googleapis.com`).
3. **Licensing Backend**: Device activation and subscription state check (`https://api.dinero.app`).
4. **GitHub Releases API**: App update availability checks (`https://api.github.com`).
5. **Hugging Face**: One-time, user-initiated local LLM model download (`https://huggingface.co`).

> **Invariant**: No financial data ever crosses any of these 5 boundaries.

---

## 🛠️ Technology Stack

| Layer | Technologies / Libraries |
| :--- | :--- |
| **Desktop Framework** | [Tauri v2](https://tauri.app) (macOS Desktop Target) |
| **Backend Runtime** | [Rust 2021](https://www.rust-lang.org) (`tokio`, `anyhow`, `thiserror`, `tracing`) |
| **Storage & Security** | SQLite, [SQLCipher](https://www.zetetic.net/sqlcipher/) via `rusqlite` + `deadpool-sqlite` (WAL Mode), `keyring` (macOS Keychain API), `argon2`, `aes-gcm` |
| **PDF & Parsing** | `pdfium-render`, `regex`, `ammonia` (HTML sanitizer), `chrono` |
| **Local AI Inference** | `llama.cpp` (`llama-server` sidecar) running GGUF quantized models with Metal GPU acceleration |
| **Frontend Framework** | [React 19](https://react.dev), [TypeScript 5.8](https://www.typescriptlang.org), [Vite 7](https://vitejs.dev) |
| **UI Components & Styling** | [Tailwind CSS v3](https://tailwindcss.com), [Radix UI](https://www.radix-ui.com/), `lucide-react`, `class-variance-authority`, `clsx`, `tailwind-merge` |
| **State & Data Fetching** | [TanStack React Query v5](https://tanstack.com/query), [Zustand v5](https://zustand-demo.pmnd.rs), [React Router v7](https://reactrouter.com) |
| **Data Visualization** | [Recharts v3](https://recharts.org) |
| **Testing & Quality** | Vitest, React Testing Library, Playwright, `@axe-core/playwright`, Biome, ESLint, `cargo clippy` |

---

## 📂 Repository Structure

```text
dinero-app/
├── src/                        # React 19 Frontend Application
│   ├── components/             # Reusable UI components (Radix primitives, charts, modals)
│   │   ├── instruments/        # Financial instrument picker & select components
│   │   ├── statements/         # PDF statement review modal & upload components
│   │   └── ui/                 # Base design system primitives (buttons, dialogs, inputs)
│   ├── hooks/                  # React custom hooks (Tauri IPC wrappers, queries, state)
│   ├── pages/                  # Top-level view routes (Dashboard, Transactions, Statements)
│   ├── types/                  # Shared TypeScript interfaces & API contracts
│   └── main.tsx                # React app entry point & routing configuration
├── src-tauri/                  # Rust Core Process & Tauri v2 Backend
│   ├── src/
│   │   ├── commands/           # Tauri IPC command handlers (auth, data, statements, settings)
│   │   ├── db/                 # SQLCipher database initialization, migrations & deadpool pool
│   │   ├── ingestion/          # Gmail poller, OAuth, Transaction Queue, Statement Queue
│   │   ├── sidecar/            # Local LLM llama-server process lifecycle & health checks
│   │   └── lib.rs              # Tauri application initialization & plugin setup
│   ├── Cargo.toml              # Rust crate manifest & dependencies
│   └── tauri.conf.json         # Tauri v2 configuration (CSP, window, capabilities)
├── e2e/                        # End-to-End Playwright test suites
├── public/                     # Static assets & public web resources
├── package.json                # Node.js dependencies & scripts
├── biome.json                  # Biome code formatter configuration
└── vite.config.ts              # Vite bundle & dev server configuration
```

---

## 🚀 Getting Started

### Prerequisites

- **Operating System**: macOS ≥ 13 (Ventura)
- **Rust Toolchain**: Stable channel (`rustup update stable`)
- **Node.js**: ≥ 20.x (`node -v`)
- **Package Manager**: `pnpm` 9.x (`corepack enable` or `npm install -g pnpm@9`)
- **Xcode Command Line Tools**: Required for macOS SDK headers (`xcode-select --install`)

### Installation & Running Locally

1. **Clone the repository**:
   ```bash
   git clone https://github.com/adityaxrawal/dinero.git
   cd dinero/dinero-app
   ```

2. **Install dependencies**:
   ```bash
   pnpm install
   ```

3. **Start the development server**:
   ```bash
   pnpm tauri dev
   ```
   > **Note**: On the first run, macOS will prompt for **Keychain access permission**. This is expected behavior as Dinero initializes its local SQLCipher encryption keys in Keychain.

### Google OAuth Setup (Dev Mode Gmail Integration)

To test Gmail email ingestion locally:

1. Create a Desktop application OAuth 2.0 Client ID in the [Google Cloud Console](https://console.cloud.google.com/).
2. Enable the **Gmail API** (read-only scopes: `https://www.googleapis.com/auth/gmail.readonly`).
3. Export your Client ID prior to starting the dev server:
   ```bash
   export GOOGLE_CLIENT_ID="your-client-id.apps.googleusercontent.com"
   pnpm tauri dev
   ```

---

## 💻 Developer Tooling & Verification Commands

### Frontend Verification

```bash
pnpm lint           # Run ESLint check across TSX/TS files
pnpm typecheck      # Run TypeScript compiler check (tsc --noEmit)
pnpm format         # Run Biome formatting write
pnpm format:check   # Verify formatting via Biome
pnpm test           # Run unit tests via Vitest
pnpm test:coverage  # Generate code coverage report
```

### Backend (Rust) Verification

```bash
cd src-tauri
cargo fmt --check                            # Verify Rust formatting
cargo clippy --all-targets --all-features   # Run Clippy lints
cargo test                                   # Run Rust unit & integration tests
```

### End-to-End Testing

```bash
pnpm exec playwright test   # Execute Playwright E2E suite
```

### Resetting Dev Environment State

To clear the local encrypted database and test onboarding cleanly:

```bash
rm -rf ~/Library/Application\ Support/com.dinero.app/finance.db*
```

---

## 🔒 License & Copyright

Copyright © 2026 Aditya Rawal. All rights reserved.
