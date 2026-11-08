# Agent entry point

This repository follows the shared standards in `shuyra-arche/.github`.

At the start of every new session, before substantive analysis or edits:

1. Resolve the current `main` revision of
   https://github.com/shuyra-arche/.github and read, at that revision:
   `CONTRIBUTING.md`, `docs/engineering/README.md`,
   `docs/engineering/agent.md`, `docs/engineering/code-organization.md`
   and `docs/engineering/testing.md`.
2. Follow the startup procedure in `agent.md`; changes and publication also
   require `branch.md` and `pull-request.md` from that revision.
3. Read this repository's `README.md` and applicable nested instructions.

Read actual contents, not remembered summaries. Report retrieval limitations.
Use the README's build and test commands. Keep test code under `tests/`;
host harnesses do not prove RV64 or QEMU behavior. Keep production warnings
enabled and preserve the target flags in `.cargo/config.toml`.
