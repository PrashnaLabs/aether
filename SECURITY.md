# Security Policy

## Supported versions

Aether is pre-1.0. Security fixes land on `main` and ship in the next release.
Only the latest release is supported.

## Reporting a vulnerability

Please do not open a public issue, discussion, or pull request for a security
problem.

Report it privately by either:

- [Opening a private security advisory](https://github.com/PrashnaLabs/aether/security/advisories/new) on GitHub, or
- Emailing **security@prashnalabs.com**.

Include what you can:

- The Aether version or commit, OS, and CPU architecture
- The provider CLI and version involved, if any
- Steps to reproduce, a proof of concept, or logs
- The impact you expect

We aim to acknowledge reports within 3 business days and to share an
assessment and fix plan within 14 days. We will keep you updated, coordinate
a disclosure date with you, and credit you in the advisory unless you prefer
not to be named.

## Scope

Aether runs coding agents with your user account's permissions, so we treat
these as security bugs:

- Bypassing a tool approval, permission prompt, or plan gate
- Unauthorized access to the orchestration socket or MCP server (`aether mcp`),
  or reuse of a lead token outside its chat
- Leaking API keys, keyring entries, `.env` values, or credentials into logs,
  transcripts, packages, or network requests
- Code execution from opening a project, file, link, image, or web page in
  Aether
- Path traversal or writes outside the intended folder from attachments,
  notes, or the explorer
- Packaging or signing flaws in the published installers

Out of scope:

- Vulnerabilities in the provider CLIs themselves (Claude Code, Codex,
  OpenCode, Pi, Antigravity) or in model behavior. Report these to the
  provider. A flaw in how Aether launches or wires them is in scope.
- Actions an agent takes after you approve them, or while you run it in a
  mode that skips approvals
- Attacks that need an already compromised local account or physical access
