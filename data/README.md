# OpenViKey Data & Provenance Directory

This directory manages dataset provenance, corpus manifests, and test fixtures.

## Provenance Policy

1. **No unverified assets**: Every corpus or external asset must be recorded in `provenance.toml` with its source URL, pinned revision, SHA-256 digest, code license, data license, and redistribution terms.
2. **Deterministic splits**: Corpus splits (train, calibration, test/held-out) are specified in `corpus-manifest.toml` and verified by `openvikey-lab`.
3. **No GPL/Copyleft data in shipped artifacts**: Only permissive licenses (MIT, Apache-2.0, CC0, Unicode-DFS) are approved for binary distribution.
