# ADR 0008: Open plaintext persistence for the Windows development host

- **Date:** 2026-08-18
- **Status:** Accepted — temporary development policy
- **Gate:** Post-GĐ2a learning iteration

## Context

The first GĐ2a checkpoint required a hidden passphrase before starting `openvikey-win` and persisted model/capture as encrypted `.ovk` envelopes. That remains the correct production-oriented storage architecture, but the prompt slows repeated host launches and the envelope makes it difficult to inspect whether learning changes during active development.

Using an empty or hard-coded passphrase would remove the prompt while presenting only cosmetic encryption: anyone with the binary or source would have the key. Disabling persistence entirely would make learning experiments less useful.

## Decision

1. `openvikey-win` starts without passphrase authentication.
2. Its development model and capture stores are inspectable plaintext JSON at `%LOCALAPPDATA%\OpenViKey\model.ovkdev.json` and `capture.ovkdev.json` by default. Custom CLI names must end in `.ovkdev.json`, and Git ignores that suffix plus recovery sidecars.
3. One debounced worker takes one coherent session snapshot and saves the model/capture pair. Each file replacement is atomic; a validated previous pair is preserved in sibling `.bak` files so interruption between the two replacements is recoverable. Model/capture hashes and cursor validation remain enforced.
4. The former encrypted `.ovk` paths are not reused, migrated, overwritten, or deleted.
5. Encryption code and encrypted persistence remain intact in `openvikey-core` and `openvikey-lab`; this ADR changes only the Windows development adapter.
6. Terminal, denylist, and English-mode learning/capture remain disabled. Electron apps (Chrome/Discord/Slack) may learn and capture when transform is on. Cursor/VS Code still need `--allow_terminal` to transform; if transform is on, learning is on. Open storage does not weaken executable/password/credential deny rules.
7. Before a production release, storage policy must be reviewed and replaced with a non-interactive OS-backed key or an explicit user-selected mode. A hard-coded development key is not acceptable as production security.

## Consequences

- Repeated Windows host launches no longer block on console input.
- Developers can inspect model/capture state directly while redesigning learning.
- The JSON files can contain typed/correction history and are not safe to share. Documentation must state this plainly rather than claiming encryption.
- Existing encrypted checkpoints remain recoverable through the lab/core APIs and are isolated by their `.ovk` names.
