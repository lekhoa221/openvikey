# OpenViKey Data & Provenance Directory

This directory manages dataset provenance, corpus manifests, and test fixtures.

## Provenance Policy

1. **No unverified assets**: Every shipped data asset must be recorded in `provenance.toml` with its source URL, pinned revision (never moving `HEAD`), SHA-256 digest, code license, data license, and redistribution terms. `sha256` must be a digest of artifact bytes (file or crate `src/` tree). Hashing the git revision string is rejected. Blocked assets with no local copy use `sha256 = "pending"` until a snapshot exists.
2. **Deterministic splits**: Corpus splits (train, calibration, test/held-out) and a non-zero split seed are specified in `corpus-manifest.toml` and verified by `openvikey-lab corpus verify`. Default mode is `unit` (authored fixtures allowed). `release` enforces spec sample floors (50k correct tokens, 1k errors, 200 per type) and is not expected to pass until a licensed production corpus exists.
3. **Data license ≠ code license**: a corpus asset is rejected if `data_license` is missing, `unknown`, `unverified`, or `none`, even when the code repo is MIT.
4. **Lexicon artifact**: `data/fixtures/lexicon/authored.json` is reproducibly built from NFC-normalized train gold tokens and adjacent-token bigrams, and records `source_manifest_hash` of `corpus-manifest.toml`. Rebuild it with `cargo run -p openvikey-lab -- corpus build-lexicon --manifest data/corpus-manifest.toml --out data/fixtures/lexicon/authored.json`.
5. **No GPL/Copyleft data in shipped artifacts**: Only permissive licenses (MIT, Apache-2.0, CC0, Unicode-DFS) are approved for binary distribution.
