# ADR 0001: Vietnamese Input Engine Backend Decision

- **Date:** 2026-08-17
- **Status:** Accepted
- **Gate:** G1 (Engine Backend Compatibility Gate)

---

## 1. Context & Problem Statement

OpenViKey requires a deterministic, lightweight, cross-platform composition engine for Vietnamese Telex and VNI input methods. The engine must support:
- Standard Telex and VNI transformation rules.
- Modern (`hoá`, `oà`, `uý`) and Classic (`hóa`, `òa`, `úy`) tone placement profiles.
- Casing preservation (e.g. `Vieetj` → `Việt`, `VIEETJ` → `VIỆT`).
- Escape and restore on duplicate modifier keys.
- Deterministic backspace handling with latency $P95 < 5\text{ms}$.
- Permissive licensing compatible with MIT distribution.

We evaluated candidate `ZeroX-DG/vi-rs` (pinned at commit `192e246d37e83094a1228e798b160f3cee14c879`).

---

## 2. Decision & Evaluation Results

We ran compatibility and performance gate tests in `crates/openvikey-core/tests/vi_rs_compatibility.rs`:

1. **Telex & VNI Transformations**: Hand-checked golden subset passed (`vieetj`/`vie6t5` → `việt`, `dduowngf`/`d9u7o7ng2` → `đường`, and related cases).
2. **Tone Placement Styles**: `AccentStyle::New` (Modern: `hoá`, `oà`) and `AccentStyle::Old` (Classic: `hóa`, `òa`).
3. **Casing**: `Vieetj` → `Việt`, `VIEETJ` → `VIỆT`, `Dd`/`DD` → `Đ`.
4. **Escape / restore**: Duplicate modifiers restore the raw pair (`ass` → `as`, `aaa` → `aa`, `ww` → `w`). `z` removes tone (`hafz` → `ha`). Switching tone keys replaces the mark (`toanfs` → `toán`). Repeating the *same* tone key on an already-toned syllable strips the mark and keeps the extra letter (`vieetjj` → `viêtj`) — not a full restore to `viet`.
5. **Invalid syllable**: Non-syllable clusters such as `zzzjjjjhhhkkk` stay raw.
6. **English / URL**: `vi-rs` is a syllable transformer, not an IME policy layer. Tokens with tone keys transform (`case` → `cáe`, `casse` → `case`). Observed URL/code strings `https://example.com`, `github.com`, and `foo->bar` currently pass through unchanged. **Wrapper owns `allow_transform=false` passthrough** and any later English/URL heuristics; do not claim `vi-rs` is an English IME.
7. **Raw keys**: `IncrementalBuffer::input()` retains the unparsed key sequence alongside `view()`.
8. **Backspace**: No native backspace API. `raw_keys.pop()` + incremental replay matches one-shot `transform_buffer` of the remaining prefix, with hand-derived expected strings (e.g. `dduowngf` → `đường`, pop 1 → `đương`).
9. **Replay-backspace performance**: P95 on `nghieengs` / `nghieeux` / `khuys` / `thuyeesn` is tens of microseconds, well below 5ms.
10. **Licensing**: MIT; git source allowlisted in `deny.toml`. Content checksum of `Cargo.toml` + `LICENSE` + `src/` is recorded in `data/provenance.toml` (not a hash of the git SHA string).

---

## 3. Chosen Approach

**Adopt `vi-rs` 0.8 as the composition backend wrapped inside `openvikey-core::engine`**.

No upstream fork is required for Telex/VNI composition. Missing native backspace is handled by replay, which meets the perf gate.

### Architectural Responsibilities of the Wrapper:
- Maintain `raw_keys: Vec<char>` to preserve full unparsed key history.
- Implement backspace via `raw_keys.pop()` and rapid buffer replay.
- Map OpenViKey's `TonePlacement` enum to `vi::processor::AccentStyle` (Modern → `AccentStyle::New`, Classic → `AccentStyle::Old`).
- Handle passthrough when `InputContext.allow_transform == false`.
- Emit `CompositionSnapshot` and self-contained `EngineAction` events.

---

## 4. Consequences & Tradeoffs

- **Positive**:
  - Reuses an existing Vietnamese parsing table and state machine.
  - Replay-backspace is far under the 5ms P95 budget.
  - Keeps `openvikey-core` focused on candidate generation, ranking, adaptive learning, and privacy storage.
- **Tradeoff**:
  - Must pin `rev = "192e246d37e83094a1228e798b160f3cee14c879"` to prevent breaking upstream semantic changes.
  - Duplicate same-tone-key leaves a leftover letter (`viêtj`); wrapper/product policy may later decide whether to treat that as escape.
  - English/URL policy is **not** implemented by `vi-rs`; mixed typing depends on the wrapper.
