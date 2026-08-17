# ADR 0001: Vietnamese Input Engine Backend Decision

- **Date:** 2026-08-17
- **Status:** Accepted
- **Gate:** G1 (Engine Backend Compatibility Gate)

---

## 1. Context & Problem Statement

OpenViKey requires a deterministic, lightweight, cross-platform composition engine for Vietnamese Telex and VNI input methods. The engine must support:
- Standard Telex and VNI transformation rules.
- Modern (`hoá`, `oà`, `uý`) and Classic (`hóa`, `òa`, `úy`) tone placement profiles.
- Casing preservation (e.g. `Vieetj` → `Việt`, `VIEETJ` → `VIỆT`, `Dd` → `Đ`).
- Escape and restore on duplicate tone keys (e.g. `vieetj` + `j` → `viêtj`, `toanf` + `s` → `toán`).
- Deterministic backspace handling with latency $P95 < 5\text{ms}$.
- Permissive licensing compatible with MIT distribution.

We evaluated candidate `ZeroX-DG/vi-rs` (pinned at commit `192e246d37e83094a1228e798b160f3cee14c879`).

---

## 2. Decision & Evaluation Results

We ran exhaustive compatibility and performance gate tests in `crates/openvikey-core/tests/vi_rs_compatibility.rs`:

1. **Telex & VNI Transformations**: Passed 100% of tested basic and compound syllables.
2. **Tone Placement Styles**: Supported via `AccentStyle::New` (Modern: `hoá`, `oà`) and `AccentStyle::Old` (Classic: `hóa`, `òa`).
3. **Casing & Capitalization**: Correctly preserves initial capitalization and full uppercase words.
4. **Tone Key Modification & Escape**: Verified that repeated tone keys remove the diacritic mark (`vieetj` + `j` → `viêtj`), while switching tone keys adjusts the diacritic (`toanf` + `s` → `toán`).
5. **Backspace Semantics**: `vi-rs` lacks a native single-character backspace API on `IncrementalBuffer`. However, popping `raw_keys` and replaying against a buffer produces identical output to typing from scratch.
6. **Replay-Backspace Performance**: Replaying the full keystroke sequence across maximum composing tokens (`nghieengs`, `nghieeux`, `khuys`, `thuyeesn`) achieves $P95 \approx 35\text{–}45\mu\text{s}$, well below the $5\text{ms}$ ($5000\mu\text{s}$) quality threshold.
7. **Licensing & Dependencies**: MIT license confirmed; passes `cargo deny check` with zero copyleft/GPL issues.

---

## 3. Chosen Approach

**Adopt `vi-rs` 0.8 as the composition backend wrapped inside `openvikey-core::engine`**.

### Architectural Responsibilities of the Wrapper:
- Maintain `raw_keys: Vec<char>` to preserve full unparsed key history.
- Implement backspace via `raw_keys.pop()` and rapid buffer replay.
- Map OpenViKey's `TonePlacement` enum to `vi::processor::AccentStyle` (Modern → `AccentStyle::New`, Classic → `AccentStyle::Old`).
- Handle passthrough when `InputContext.allow_transform == false`.
- Emit `CompositionSnapshot` and self-contained `EngineAction` events carrying exact updated revisions.

---

## 4. Consequences & Tradeoffs

- **Positive**:
  - Reuses an existing, well-tested Vietnamese parsing table and state machine.
  - Zero performance penalty ($35\mu\text{s}$ vs $5000\mu\text{s}$ limit).
  - Keeps `openvikey-core` focused on candidate generation, ranking, adaptive learning, and privacy storage.
- **Tradeoff**:
  - Must pin `rev = "192e246d37e83094a1228e798b160f3cee14c879"` to prevent breaking upstream semantic changes.
