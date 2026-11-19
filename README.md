# CodeGraph-MCP (Codename: *Antidote*)
> **The Structural AST, Semantic Reuse, and Integrity Engine for AI Coding Agents**

`CodeGraph-MCP` is an ultra-fast, local-first engine written in Rust that provides **Tree-sitter AST structural navigation**, **semantic reuse interception (Dense + BM25)**, and an **in-memory blast-radius impact analyzer** exposed via the Model Context Protocol (MCP).

Specifically designed to stop **token burn**, **code duplication**, and **context amnesia** in **Claude Code, Google Antigravity, Cursor, and Windsurf**.

---

## ⚡ The Problems It Solves

1. **The Grep/Token-Burn Trap:** Coding agents burn 50,000+ tokens running `grep` and dumping whole files into context. `CodeGraph` resolves call hierarchies, type definitions, and caller graphs in **< 100 tokens** without reading raw source files.
2. **"Refactoring Oceans" & Duplication Bloat:** AI agents persistently reinvent existing helper functions because they lack global codebase awareness. `CodeGraph` intercepts utility creation before code is written, guiding agents to reuse existing project utilities.
3. **Prompt Injection & Bidi Attacks:** Sanitizes docstrings and comments before sending them to the LLM agent, stripping Unicode Bidi Trojan overrides (CVE-2021-42574) and neutralizing prompt injection attacks.
4. **Assertion Weakening Auditor:** Pre-commit guard detecting when agents quietly relax strict test assertions (`toBe(200)` -> `toBeDefined()`) to fake green tests.

---

## 🚀 Quickstart & Installation

### 1. Build & Install Locally
```bash
git clone https://github.com/khalidfarooq/codegraph-mcp.git
cd codegraph-mcp
cargo build --release

# Install binary to ~/.cargo/bin (or your PATH)
cp target/release/codegraph ~/.cargo/bin/
```

### 2. Index Your Repository
```bash
codegraph index --repo /path/to/project
```

### 3. Connect to Claude Code / Antigravity via MCP
Add to your `claude_desktop_config.json` or `.gemini/antigravity/mcp.json`:
```json
{
  "mcpServers": {
    "codegraph": {
      "command": "codegraph",
      "args": ["mcp", "--repo", "/path/to/your/project"]
    }
  }
}
```

---

## 🛠️ MCP Tool Reference

Once connected, your AI coding agent has access to two unified primitives:

### 1. `codegraph_explore`
Navigates the AST graph without reading raw files:
* **`action: "symbol_context"`**: Returns symbol definition location, signature, sanitized docstrings, callers, and callees in < 100 tokens.
* **`action: "call_hierarchy"`**: Returns full recursive caller/callee trees up to a specified depth.
* **`action: "impacted_files"`**: Computes blast radius of changing a symbol or interface across the entire repository.
* **`buffer_content`**: Optional in-memory unsaved buffer text to inspect dirty agent edits instantly via the Ephemeral Shadow Overlay.

### 2. `codegraph_find_utility`
Semantic & lexical deduplication engine:
* Searches the repository for existing reusable functions, helpers, or classes matching plain-English intent (e.g. `"format currency with locale"`, `"exponential backoff retry"`).
* Returns similarity scores, line locations, signatures, and repo usage counts.

---

## 💻 CLI Commands

```bash
# Index current repository
codegraph index

# Quick CLI structural symbol lookup
codegraph explore formatMoney

# Search for existing reusable helpers
codegraph find "exponential retry"

# Install Git pre-commit assertion guard
codegraph hook install

# Start background IPC daemon
codegraph daemon
```

---

## 🛡️ Security & Hardening

* **Local-First & Zero Egress:** Runs 100% locally on your machine. Zero code or telemetry is ever transmitted over the network.
* **IPC Socket Isolation:** Uses authenticated Unix domain sockets with peer credential verification (`SO_PEERCRED` / `getpeereid`) restricted to your user account.
* **Path Jailing:** All path accesses are canonicalized beneath the repository root, blocking path traversal (`../../etc/passwd`).

---

## 🧪 Test Suite

Run all workspace tests:
```bash
cargo test --workspace
```
* **`codegraph-security`**: Jail path traversal, Bidi Trojan neutralization, prompt injection redaction.
* **`codegraph-core`**: Tree-sitter parsers (TS/JS, Python, Rust, Go), BM25 lexical search, petgraph traversal, ephemeral shadow overlay, test assertion auditor.
* **`codegraphd`**: Singleton daemon request handling and advisory locking.
* **`codegraph-mcp`**: Model Context Protocol JSON-RPC loop and fallback resilience.

---

## 📜 License
Apache 2.0 / MIT © Khalid Farooq
EOF
