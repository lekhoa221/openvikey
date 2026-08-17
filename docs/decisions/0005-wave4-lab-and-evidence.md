# ADR 0005: Wave 4 lab harness and fail-closed release evidence

- **Date:** 2026-08-17
- **Status:** Accepted
- **Gate:** M9 complete; M10 implementation complete, release data blocked

## Context

The core contracts and generators need an observable integration path and machine-readable evidence. Wave 3 tests injected `CompositionSnapshot` directly and used tiny lexicons, so they did not prove the live `Engine → generate → rank → decide` path or the 15 ms candidate-generation budget. The authored held-out fixture is intentionally tiny and cannot satisfy release sample floors.

## Decisions

1. `openvikey-lab type` streams stdin through the real engine and emits JSONL observations containing snapshot, engine actions, ranked candidates, state, and chosen correction action. A committed token becomes the next token's `LeftContext`.
2. `script run` accepts deterministic JSONL accept/reject/auto-emission/undo operations and reports frozen script/model SHA-256 hashes. Duplicate feedback sequence numbers remain idempotent through the model contract.
3. `model dump` reads its passphrase from stdin, authenticates/decrypts first, validates the model payload, and only then writes model JSON to stdout. The CLI uses the production passphrase provider, not the in-memory test provider.
4. `corpus evaluate` verifies the manifest/provenance first, builds the lexicon only from train, and evaluates held-out through the live engine session. Reports include corpus/lexicon/config hashes, raw confusion/sample counts, exact denominator formulas, point estimates, Wilson 95% intervals, and per-taxonomy recall/coverage/top-k evidence.
5. Unit and release evaluation modes remain distinct. Release mode enforces the frozen sample floors before evaluation and fails closed; tiny authored fixtures can never produce a passing release report.
6. Lexicon construction builds immutable folded-form and folded-length indexes. Diacritics uses exact folded-form lookup; fuzzy runs DP only in the bounded folded-length range instead of folding/scanning every entry on each generation.
7. Duplicate-key deletion has cost `0.30`, lower than ordinary insertion/deletion cost `0.75`. Accented output must pass onset+nucleus+coda Vietnamese grammar; explicit ASCII lexicon entries remain the documented mixed-Latin exception.
8. `perf` combines the packaged artifact with a deterministic 6,000-entry syllable stress set and labels both counts in the report. Runtime timings are intentionally the only non-reproducible report fields. A release run on the review machine measured candidate P95 8.131 ms, per-key P95 4 µs, startup load 5.820 ms, and peak working set 12,357,632 bytes.
9. Encrypted saves can run through a lab-owned background debounce worker. Burst submissions coalesce to the newest payload; core stays synchronous and async-runtime-free.
10. Password, terminal, and denylist simulations map to `allow_transform=false` and `allow_learning=false`; tests prove no candidates or model mutation.

## Consequences

- M9 is complete and can reproduce unit reports and learning hashes.
- G3 and final Wave 4 closure remain open until a provenance-approved release corpus reaches 50,000 correct tokens, 1,000 labeled errors, and 200 per taxonomy. No synthetic/stress data may be presented as held-out quality evidence.
- The stress benchmark is valid algorithmic performance evidence but does not replace a future run against the final packaged production lexicon.
- `target/evidence` remains uncommitted; release notes must publish hashes and reproduction commands after all release gates pass.
