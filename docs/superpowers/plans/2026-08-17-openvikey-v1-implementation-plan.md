# OpenViKey v1 Headless Brain — Implementation Plan

- **Ngày:** 2026-08-17
- **Trạng thái:** In progress — M0–M3 + Wave 0 + **Wave 1 (M4, M5 + closure review)** xong. Wave 2 tiếp theo: M6 + 7A + M8.
- **Governing spec:** [`../specs/2026-08-17-openvikey-design.md`](../specs/2026-08-17-openvikey-design.md)
- **Phạm vi:** chỉ v1 headless brain; không TSF, CGEventTap/InputMethodKit, OS keyring, sync/CRDT, GUI settings hay phrase-level diacritics.
- **Cách làm:** TDD; mỗi checkpoint là một commit nhỏ, build xanh và không trộn refactor ngoài phạm vi. Phần còn lại chia **3 nhóm / 4 sóng** (§4.1), không chia 1 milestone = 1 nhóm.

---

## 1. Outcome & definition of done

Sau plan này, repo có một Cargo workspace chạy offline gồm:

1. `openvikey-core`: engine Telex/VNI, 4 candidate generators, rank/decision/model/feedback, semantic edit + undo, encrypted model store.
2. `openvikey-lab`: CLI quan sát composition/candidate/decision, chạy user-script học, corpus metrics, perf và model dump.
3. Dữ liệu/corpus có manifest provenance, hash và split cố định; không nhúng asset chưa rõ quyền redistribution.
4. Tất cả acceptance gate §3, §8 và §11 của spec có executable test/report.

V1 chỉ yêu cầu **CLI**. TUI là enhancement sau khi mọi gate xanh; không được làm chậm v1.

### Global verification

Chạy ở repo root sau mỗi milestone:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Release gate bổ sung:

```powershell
cargo test -p openvikey-core --test golden_engine
cargo test -p openvikey-core --test learning_state_machine
cargo test -p openvikey-core --test store_recovery
cargo run -p openvikey-lab -- corpus verify --manifest data/corpus-manifest.toml
cargo run -p openvikey-lab -- corpus evaluate --manifest data/corpus-manifest.toml --out target/evaluation.json
cargo run -p openvikey-lab -- perf --out target/perf.json
cargo deny check
```

Prerequisite một lần trên máy dev: cài `cargo-deny` **0.20.2** (`cargo install cargo-deny --version 0.20.2 --locked`); máy review hiện chưa cài command này. CI dùng đúng version này, không tự trôi theo latest.

---

## 2. Decisions locked for v1

- Rust toolchain: **1.96.0**, edition 2024 (đã có trên máy review).
- Workspace tối thiểu: `openvikey-core` + `openvikey-lab`; không tách thêm crate nếu chưa có dependency boundary thật.
- Unicode: NFC cho matching; grapheme cluster cho core edit ranges; giữ `original` để undo chính xác.
- Time: core không đọc wall-clock; caller truyền `at_ms`/`evaluate_at_ms`.
- Learning default: Beta(1,1), promote confidence 0.95 sau canonical 18 explicit accepts; demote 2 undo/10 auto-emission.
- Diacritics v1: per-token, left context, suggestion-only.
- Persistence v1: passphrase file provider; in-memory provider chỉ cho test.
- Runtime network: không có network module/dependency/API.
- `vi-rs` là **candidate**, chưa là quyết định. Gate ở Milestone 2 phải kết luận adopt/wrap hoặc implement nội bộ.

---

## 3. Target repository shape

```text
openvikey/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── deny.toml
├── crates/
│   ├── openvikey-core/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── types.rs
│   │   │   ├── engine/
│   │   │   ├── lexicon.rs
│   │   │   ├── generate/
│   │   │   ├── rank.rs
│   │   │   ├── model.rs
│   │   │   ├── decision.rs
│   │   │   ├── feedback.rs
│   │   │   └── store/
│   │   └── tests/
│   └── openvikey-lab/
│       ├── Cargo.toml
│       └── src/
├── data/
│   ├── README.md
│   ├── provenance.toml
│   ├── corpus-manifest.toml
│   └── fixtures/
├── docs/
│   ├── decisions/
│   └── superpowers/
└── .github/workflows/ci.yml
```

Không commit raw corpus lớn hoặc model weights cho đến khi provenance gate cho phép. Generated artifacts phải reproducible từ manifest hoặc được pin hash rõ ràng.

---

## 4. Gate map

| Gate | Điều kiện mở | Chặn milestone |
|---|---|---|
| **G0 Data provenance** | Mỗi asset có source, revision, code/data license, redistribution và SHA-256; không còn trạng thái `unknown` với asset sẽ ship | Lexicon production, corpus release gate |
| **G1 Engine backend** | ADR kết luận `vi-rs` pass/fail; golden compatibility + replay-backspace perf có bằng chứng | Engine production |
| **G2 Semantic contract** | Contract/action/undo/property tests xanh, stale revision bị từ chối | Learning + lab integration |
| **G3 Quality** | Corpus đủ minimum sample; metric đúng denominator; threshold spec đạt | V1 complete |
| **G4 Security/perf** | Recovery/rewrap/no-network/perf gates xanh | V1 complete |

---

## 4.1 Wave execution & mốc hoàn thành

Phần này ghi cách **triển khai phần còn lại**, không đổi phạm vi hay thứ tự kỹ thuật của M4–M10. Chi tiết red/green/verify vẫn nằm ở từng milestone bên dưới.

Chia theo **sóng phụ thuộc + quyền sở hữu file**. Không mở hai nhóm trên cùng một file nóng. `types.rs` và `engine/` đóng băng trừ khi cả ba nhóm đồng ý thêm field (không đổi nghĩa contract).

### Baseline đã xong (không còn trong wave)

| Milestone | Commit | Gate | Verify đã chạy |
|---|---|---|---|
| M0 workspace + provenance skeleton | `08b9e94` + follow-up `df31051` | G0 skeleton | `provenance_gate` |
| M1 semantic contract | `ea36b14` | G2 | `semantic_contract` |
| M2 `vi-rs` compatibility → **Adopt/wrap** (ADR 0001) | `a890af0` | G1 | `vi_rs_compatibility` |
| M3 engine Telex/VNI vertical slice | `139713e` + follow-up `cc41295` | engine slice | `golden_engine` |

Ghi chú M3: `golden_engine` xanh theo lệnh verify, nhưng ma trận §3.1 còn thiếu NFC/NFD, escape/reset đủ bộ, URL/code/mixed, VNI `oà`/`òa`, `InsertText`/revision đơn điệu. Vá tuỳ chọn ở Wave 0 hoặc gắn 7A — không chặn Wave 1.

### Ba nhóm

| Nhóm | Vai trò | Milestone | Sở hữu file (độc quyền) |
|---|---|---|---|
| **A — Data / G0** | Corpus, lexicon, metric đúng mẫu số | M4; phần evaluate/report của M9–M10 | `lexicon.rs`, `lab/corpus.rs`, `lab/report.rs`, `data/**`, `tests/corpus_manifest.rs` |
| **B — Brain** | Pipeline sửa + học | M5 → M6 → 7A/7B/7C | `generate/**`, `rank.rs`, `model.rs`, `decision.rs`, `feedback.rs` + test tương ứng |
| **C — Store + Lab** | Persist, CLI, cổng G4 | M8; session/CLI/perf; M10 | `store/**`, `lab/cli.rs`, `lab/session.rs`, `lab/perf.rs`, `tests/cli_smoke.rs` |

File **không được hai nhóm sửa cùng lúc:** `model.rs` (M5 stub → M6), `generate/mod.rs` (M5 → 7A/B/C), `openvikey-lab` CLI + `corpus.rs` (M4 verify → M9 evaluate), `data/provenance.toml`.

### Sơ đồ phụ thuộc

```text
M4 lexicon/corpus ──────────────► 7B fuzzy
        │                         7C diacritics
        │                         M9 evaluate / M10 G3
        ▼
M5 pipeline (generate/rank/decision + model stub)
        │
        ├──────────────────────► 7A telex_fix   (engine + generate/)
        ├──────────────────────► M6 learning    (mở rộng model.rs)
        │                              │
        │                              ▼
        └──────────────────────► M8 store  ──► M9 CLI ──► M10 đóng cổng
```

### Wave 0 — Khóa interface

- **Ai:** 1 người / orchestrator; chưa tách agent.
- **Làm:**
  1. Commit file plan này nếu vẫn untracked.
  2. Khóa chữ ký tối thiểu (chưa logic): `Lexicon`/bigram lookup; `Generator` (snapshot + left context → `Candidate`, không `&Model`); `ModelView` read-only (M5) vs event-backed (M6); `SecretProvider` + `Store` nhận bytes/JSON, không biết Beta; công thức metric precision ≠ FPR, Wilson.
  3. Tuỳ chọn: vá golden M3 còn thiếu (§8 Red / spec §3.1).
- **Mốc xong:** trait/API đã ghi trong repo hoặc ADR ngắn; `types.rs`/`engine/` không còn “sẽ đổi lúc làm M5”; workspace test vẫn xanh.

### Wave 1 — A ∥ B

| Nhóm | Làm | Được phép bắt đầu khi |
|---|---|---|
| A | M4 TDD: reject license/hash/overlap; Wilson; lexicon authored nhỏ; `corpus verify` | Wave 0 xong. Production data vẫn `blocked` cũng được. |
| B | M5 TDD: `ko → không`, dedupe, tie-break NFC, hysteresis 0.70/0.60, `allow_transform=false` | Wave 0 xong. Abbrev seed trong repo; không đợi lexicon production. |

- **Mốc xong:** A đã merge `lexicon.rs` + `corpus verify` xanh; B đã merge `generate/mod.rs` + `ModelView` rỗng + `abbrev_slice` xanh.
- **Verify:**

```powershell
cargo run -p openvikey-lab -- corpus verify --manifest data/corpus-manifest.toml
cargo test -p openvikey-lab --test corpus_manifest
cargo test -p openvikey-core --test abbrev_slice
cargo test --workspace --all-features
```

### Wave 2 — B nội bộ + C bắt đầu

| Việc | Nhóm | Song song? |
|---|---|---|
| M6 learning (promote 18, demote 2/10, decay, undo) | B | Sau M5; độc quyền `model.rs` |
| 7A `telex_fix` (`ch2ao`) | B, commit riêng | Song song M6 nếu không sửa `model.rs` |
| M8 store (envelope, rewrap, recovery) | C | Song song M6 nếu store chỉ mã hoá payload JSON versioned |

7B/7C **chưa** mở nếu lexicon của A chưa có API lookup.

- **Mốc xong:** `learning_state_machine` + `undo_properties` + `telex_fix` + `store_encryption` + `store_recovery` xanh.
- **Verify:**

```powershell
cargo test -p openvikey-core --test learning_state_machine
cargo test -p openvikey-core --test undo_properties
cargo test -p openvikey-core --test telex_fix
cargo test -p openvikey-core --test store_encryption
cargo test -p openvikey-core --test store_recovery
```

### Wave 3 — Generators còn lại

| Việc | Nhóm | Điều kiện |
|---|---|---|
| 7B `fuzzy` | B | Wave 1 A xong (lexicon lookup) |
| 7C `diacritics` (`max_action=Suggest`) | B | Wave 1 A xong (lexicon + bigram) |

Hai commit độc lập, không gộp một PR — đúng §12.

- **Mốc xong:** cả bốn generator có test độc lập; diacritics không promote auto.
- **Verify:**

```powershell
cargo test -p openvikey-core --test telex_fix
cargo test -p openvikey-core --test fuzzy
cargo test -p openvikey-core --test diacritics
```

### Wave 4 — Lab + đóng cổng v1

Chỉ sau khi A có evaluate/report và B có 4 generator + learning:

1. M9: `type`, `script run`, `model dump`, `corpus evaluate`, `perf`
2. M10: G3 (mẫu tối thiểu + ngưỡng) và G4 (P95, no-network, debounce ~2s, `cargo deny`)

M10 không song song: calibrate chỉ trên calibration split; held-out chạy **đúng một lần**.

- **Mốc xong:** `cli_smoke` xanh; `target/evidence/*` reproduce được (không commit); checklist §17 đủ.
- **Verify:** toàn bộ lệnh *Release gate* ở §1.

### Lối 1 người / 1 agent

Không giả lập 3 nhóm. Đi tuần tự:

`M4 → M5 → M6 → 7A → 7B → 7C → M8 → M9 → M10`

7A có thể lên trước 7B/7C. M8 có thể lên ngay sau M6. Không nhảy M5 trước M4 nếu muốn 7B/7C liền tay.

### Việc không được song song

- Hai người cùng sửa `model.rs`, `generate/mod.rs`, lab CLI, `provenance.toml`.
- M9 `script run` trước khi M6 ra model hash ổn định.
- M10 calibrate trên held-out (dừng, tách split mới — §16).
- Nhét GPL / data `unknown` / transformer / TSF / TUI — dừng, sửa spec.

### Bảng theo dõi mốc

- [x] Baseline M0–M3 xanh trên `main`
- [x] Wave 0 — khóa interface (+ vá golden M3 tuỳ chọn); ADR 0002
- [x] Wave 1 — M4 (A), M5 (B) và closure review merge; verify sóng 1 xanh
- [ ] Wave 2 — M6 + 7A + M8 xanh
- [ ] Wave 3 — 7B + 7C xanh
- [ ] Wave 4 — M9 + M10; G3/G4 có evidence; v1 complete

---

## 5. Milestone 0 — Workspace, quality rails & provenance skeleton — **DONE**

### Goal

Tạo baseline có thể build/test/lint trước khi thêm domain logic; đưa license/data uncertainty thành gate hữu hình.

### Files

- Create: `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `deny.toml`
- Create: `crates/openvikey-core/Cargo.toml`, `crates/openvikey-core/src/lib.rs`
- Create: `crates/openvikey-lab/Cargo.toml`, `crates/openvikey-lab/src/main.rs`
- Create: `crates/openvikey-lab/src/provenance.rs`
- Create: `.github/workflows/ci.yml`
- Create: `data/README.md`, `data/provenance.toml`, `data/corpus-manifest.toml`
- Update: `.gitignore`

### Red

1. Add workspace smoke test expecting both packages and pinned toolchain metadata.
2. Add provenance validation test with a deliberately missing license/hash fixture; confirm fail.

### Green

1. Configure workspace lints: forbid unsafe by default; deny warnings in CI.
2. Add only minimum dependencies needed for the scaffold.
3. Define provenance schema:

```text
id, kind, source_url, revision, sha256,
code_license, data_license, redistribution,
purpose, split_role, status
```

4. Seed provenance with the reviewed candidates, but mark data as `blocked` until the dataset-specific license is verified:

| Candidate | Pinned evidence | Initial status |
|---|---|---|
| `vi-rs` | local commit `192e246d37e83094a1228e798b160f3cee14c879`, MIT | code candidate; G1 required |
| `underthesea` | local commit `0fd222c1d892604949c5bad434967211bff82f9b`, Apache-2.0 code | data blocked pending dataset license |
| `restore_vietnamese_diacritics` | local commit `7985e9d1dc92f09b012c70db4b2414c5a438391c`, MIT code | model/corpus blocked; code study only |
| Project-authored fixtures | committed in this repo | approved for tests |

5. `cargo deny` allowlist only licenses compatible with MIT distribution; GPL prior art never enters dependency graph.

### Verify

```powershell
cargo test --workspace
cargo deny check
```

### Commit checkpoint

`chore: scaffold Rust workspace and provenance gates`

---

## 6. Milestone 1 — Core semantic types & inverse edit contract — **DONE**

### Goal

Đóng G2 trước khi engine/correction phụ thuộc vào contract chưa ổn định.

### Files

- Create: `crates/openvikey-core/src/types.rs`
- Create: `crates/openvikey-core/tests/semantic_contract.rs`
- Update: `crates/openvikey-core/src/lib.rs`

### Red

Viết test trước cho:

1. `InputEvent` round-trip gồm `seq`, `at_ms`, key/boundary/backspace/cursor/reset và context flags.
2. NFC normalization nhưng vẫn giữ `original`.
3. `EditRange` phân biệt `ActiveComposition` và `CommittedBeforeCaret`.
4. `ReplaceRange` inverse đổi `original/replacement`, giữ `edit_id`, sinh range đúng.
5. Undo sau `CursorMoved`/`SelectionChanged` bị từ chối vì stale revision.
6. Multi-grapheme/multi-word replacement + delimiter hoàn nguyên đúng.

### Green

Implement value types tối thiểu; không thêm OS type, UTF-16 offset hoặc global state. `EngineAction` phải tự chứa payload.

### Verify

```powershell
cargo test -p openvikey-core --test semantic_contract
```

### Commit checkpoint

`feat(core): define deterministic input and edit contracts`

---

## 7. Milestone 2 — `vi-rs` compatibility gate (decision, not production feature) — **DONE**

### Goal

Quyết định dựa trên test liệu dùng `vi-rs` 0.8 làm engine backend có đơn giản và an toàn hơn tự viết.

### Known evidence

- Local candidate commit: `192e246d37e83094a1228e798b160f3cee14c879`.
- Crate khai báo Telex/VNI, edition 2024, Rust 1.96.
- `IncrementalBuffer` có `push/input/clear` nhưng không có native `backspace`; spike phải thử raw-buffer replay sau khi pop.

### Files

- Create: `crates/openvikey-core/tests/vi_rs_compatibility.rs`
- Create: `docs/decisions/0001-engine-backend.md`
- Temporary/update: `crates/openvikey-core/Cargo.toml` với git dependency pin đúng `rev=192e246d37e83094a1228e798b160f3cee14c879` cho spike; ADR quyết định dependency production sau gate.

### Red / gate cases

1. Golden subset cho Telex và VNI.
2. Modern/classic tone profile đúng theo expected matrix.
3. Casing, escape/reset, invalid syllable, English/URL passthrough.
4. Raw keys và rendered output không mất thông tin.
5. Backspace bằng `raw_keys.pop()` + replay cho kết quả như compose từ đầu.
6. Replay-backspace P95 < 5ms trên maximum composing token fixture.
7. Dependency license/checksum xuất hiện trong lock/provenance.

### Decision rule

- **Adopt/wrap** nếu 100% compatibility cases pass, không cần fork patch và perf đạt gate.
- **Reject** nếu semantics khác golden matrix hoặc cần sửa upstream logic đáng kể. Gỡ dependency ngay và implement engine nội bộ từ Milestone 3.
- Nếu chỉ thiếu native backspace nhưng replay đúng/đủ nhanh, chấp nhận wrapper; ghi tradeoff trong ADR.

### Verify

```powershell
cargo test -p openvikey-core --test vi_rs_compatibility -- --nocapture
```

### Commit checkpoint

`docs: decide Vietnamese engine backend from compatibility gate`

---

## 8. Milestone 3 — Telex/VNI engine vertical slice — **DONE** (golden §3.1 còn gap, vá Wave 0 hoặc 7A)

### Goal

Hoàn thành deterministic engine theo contract, bất kể ADR chọn wrapper hay internal implementation.

### Files

- Create: `crates/openvikey-core/src/engine/mod.rs`
- Create as needed: `engine/backend.rs`, `engine/telex.rs`, `engine/vni.rs`
- Create: `crates/openvikey-core/tests/golden_engine.rs`
- Create: `data/fixtures/engine/*.jsonl`

### Red

Golden matrix phủ toàn bộ §3.1:

- Telex/VNI.
- Hai profile tone placement với expected strings ghi trực tiếp.
- Casing, double-key/reset/restore.
- NFC/NFD input/output.
- Backspace ở mọi vị trí của composing token.
- English, URL, code, mixed token passthrough.
- Boundary commit và revision tăng tất định.

### Green

1. Engine nhận `InputEvent`, trả snapshot + self-contained actions.
2. Không correction/model/store trong engine.
3. Context `allow_transform=false` passthrough và không giữ sensitive buffer.
4. Không tối ưu sớm ngoài replay/backspace nếu benchmark chưa đỏ.

### Verify

```powershell
cargo test -p openvikey-core --test golden_engine
```

### Commit checkpoint

`feat(engine): implement deterministic Telex and VNI composition`

---

## 9. Milestone 4 — Corpus harness, lexicon & frozen data gate — Wave 1 / Nhóm A

### Goal

Có data path tái lập được trước khi viết fuzzy/diacritics; mở G0 cho những asset thực sự ship.

### Files

- Create: `crates/openvikey-core/src/lexicon.rs`
- Create: `crates/openvikey-lab/src/corpus.rs`
- Create: `crates/openvikey-lab/src/provenance.rs`
- Create/update: `data/provenance.toml`, `data/corpus-manifest.toml`
- Create: `data/fixtures/corpus/*.jsonl`
- Create: `crates/openvikey-lab/tests/corpus_manifest.rs`

### Red

1. Reject asset with missing/unknown license, revision, hash or redistribution.
2. Reject train/calibration/test overlap by stable item ID.
3. Reject manifest whose file hash differs.
4. Metric fixture proves precision and FPR use different denominators.
5. Wilson interval fixture matches known hand-calculated cases.
6. Enforce minimum sample counts only in `release` evaluation mode; small authored fixtures remain valid for unit tests.

### Green

1. Implement deterministic manifest loader and split verifier.
2. Approve sources only after dataset-specific license review; code-repo license alone is insufficient.
3. Build compact lexicon/bigram artifact deterministically; artifact records source manifest hash.
4. Keep a tiny project-authored lexicon for unit tests so development is not blocked by production data acquisition.

### Verify

```powershell
cargo run -p openvikey-lab -- corpus verify --manifest data/corpus-manifest.toml
cargo run -p openvikey-lab -- corpus build-lexicon --manifest data/corpus-manifest.toml --out target/lexicon.json
cargo test -p openvikey-lab --test corpus_manifest
```

### Commit checkpoint

`feat(data): add licensed corpus and deterministic lexicon pipeline`

---

## 10. Milestone 5 — First correction slice: abbreviation end-to-end — Wave 1 / Nhóm B

### Goal

Dùng generator đơn giản nhất để nối `generate → rank → decision → suggestion` trước khi thêm thuật toán fuzzy.

### Files

- Create: `crates/openvikey-core/src/generate/mod.rs`
- Create: `crates/openvikey-core/src/generate/abbrev.rs`
- Create: `crates/openvikey-core/src/rank.rs`
- Create: `crates/openvikey-core/src/model.rs` với empty/default read-only model view; Milestone 6 mở rộng thành event-backed model.
- Create: `crates/openvikey-core/src/decision.rs`
- Create: `crates/openvikey-core/tests/abbrev_slice.rs`

### Red

1. `ko → không` sinh candidate có source/evidence/base score.
2. Dedupe cùng output từ hai evidence path.
3. Stable lexical NFC tie-break.
4. Cold-start abbreviation mơ hồ chỉ suggestion.
5. `ignore→suggest` ở 0.70; `suggest→ignore` dưới 0.60.
6. Suggestion phát `ShowSuggestions{revision,candidates}`.
7. Rule-context giữ input method do caller truyền và source rule thắng sau dedupe.
8. `allow_transform=false` không generate/rank/decide.

### Green

Implement interface và versioned score/decision config tối thiểu. Calibration fixture nhỏ được authored trong repo; production config chỉ được fit trên calibration split.

### Verify

```powershell
cargo test -p openvikey-core --test abbrev_slice
```

### Commit checkpoint

`feat(correction): deliver abbreviation suggestion vertical slice`

---

## 11. Milestone 6 — Adaptive model, feedback, promotion/demotion & undo — Wave 2 / Nhóm B

### Goal

Chứng minh điểm khác biệt F6 bằng state machine tất định, không dựa wall-clock.

### Files

- Update: `crates/openvikey-core/src/model.rs`
- Create: `crates/openvikey-core/src/feedback.rs`
- Create: `crates/openvikey-core/tests/learning_state_machine.rs`
- Create: `crates/openvikey-core/tests/undo_properties.rs`

### Red

1. Positive/negative mass không âm; weighted Beta formula đúng.
2. Hai cặp `original→candidate` khác nhau không dùng chung evidence/confidence dù cùng generator và left context.
3. Canonical 17 accept vẫn suggest; accept thứ 18 promote auto.
4. Hai undo trong 10 auto-emission demote; undo cũ ngoài cửa sổ không tính.
5. Confidence decay theo injected `evaluate_at_ms`; age âm clamp 0.
6. Cùng event stream + evaluate time cho cùng serialized model/order.
7. `AutoSettled` ghi positive yếu đúng một lần; `SuggestionSettled` ghi negative yếu đúng một lần.
8. Cursor/selection break ngăn implicit `X→Y` mining.
9. Auto edit tạo edit log; undo phát inverse replace và negative evidence.
10. Diacritics source policy không thể promote auto.
11. `allow_learning=false` không đổi model.

### Green

Implement event records + query-time decay. Không mutate toàn bộ count theo timer; không background decay task. Bound undo log theo số entry cấu hình để tránh tăng vô hạn.

### Verify

```powershell
cargo test -p openvikey-core --test learning_state_machine
cargo test -p openvikey-core --test undo_properties
```

### Commit checkpoint

`feat(learning): add deterministic confidence feedback and undo`

---

## 12. Milestone 7 — Remaining generators

Mỗi generator là một commit độc lập; không gộp cả ba thành một patch lớn. 7A thuộc Wave 2; 7B và 7C thuộc Wave 3 (cần lexicon của M4).

### 7A. `telex_fix` — Wave 2 / Nhóm B

- Files: `generate/telex_fix.rs`, `tests/telex_fix.rs`
- Red: raw-key misplacement fixtures như `ch2ao`, VNI/Telex separation, no-change valid input.
- Green: rule-based reconstruction; no model access.
- Commit: `feat(correction): add raw-key Telex and VNI fixes`

### 7B. `fuzzy` — Wave 3 / Nhóm B

- Files: `generate/fuzzy.rs`, `tests/fuzzy.rs`
- Red: weighted adjacency, transposition (`khọgn`), duplicate key, valid-syllable constraint, bounded candidate count.
- Green: simplest weighted edit-distance that meets perf; do not port full SymSpell until benchmark requires it.
- Commit: `feat(correction): add weighted fuzzy candidates`

### 7C. `diacritics` — Wave 3 / Nhóm B

- Files: `generate/diacritics.rs`, `tests/diacritics.rs`
- Red: per-token top-k, left bigram context, stable ordering, ambiguous input suggestion-only, no phrase delayed edit.
- Green: lexicon/bigram scoring only; no transformer/model runtime in v1.
- Commit: `feat(correction): add suggestion-only token diacritics`

### Combined verify

```powershell
cargo test -p openvikey-core --test telex_fix
cargo test -p openvikey-core --test fuzzy
cargo test -p openvikey-core --test diacritics
```

---

## 13. Milestone 8 — Encrypted model store — Wave 2 / Nhóm C

### Goal

Persistence qua restart bằng passphrase, crash-safe và không block input path.

### Files

- Create: `crates/openvikey-core/src/store/mod.rs`
- Create: `store/envelope.rs`, `store/file.rs`, `store/passphrase.rs`
- Create: `crates/openvikey-core/tests/store_encryption.rs`
- Create: `crates/openvikey-core/tests/store_recovery.rs`

### Red

1. Plaintext model không xuất hiện trong blob.
2. Wrong passphrase, corrupt header, corrupt ciphertext và unsupported version là error khác nhau.
3. Unique CSPRNG nonce per write; tampered AAD fail closed.
4. Process restart mở lại model bằng passphrase wrapper.
5. Rewrap đổi passphrase không đổi encrypted model payload.
6. Temp-write/flush/replace/backup recovery qua injected failure points.
7. In-memory provider deterministic cho tests; production lab không dùng nó.

### Green

1. XChaCha20-Poly1305 + Argon2id; KDF parameters nằm trong authenticated header.
2. Container tách immutable payload AAD/ciphertext khỏi authenticated wrapped-key slots; rewrap chỉ thay slot và giữ nguyên payload ciphertext/tag.
3. Argon2id mặc định [RFC 9106](https://www.ietf.org/rfc/rfc9106.html) low-memory `64 MiB / t=3 / p=4`; chỉ tune nếu startup gate đỏ và không thấp hơn [OWASP floor](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html) `19 MiB / t=2 / p=1`. Persist tham số để migrate/rehash về sau.
4. Serialize decrypted model bằng versioned human-readable JSON; envelope là binary.
5. Core store API đồng bộ và nhỏ; lab sở hữu debounce worker/background I/O. Không đưa async runtime vào core chỉ để save.
6. Zeroize passphrase/DEK buffers khi khả thi; không log secret/plaintext model.

### Verify

```powershell
cargo test -p openvikey-core --test store_encryption
cargo test -p openvikey-core --test store_recovery
```

### Commit checkpoint

`feat(store): persist model with passphrase envelope encryption`

---

## 14. Milestone 9 — Lab CLI & corpus evaluation — Wave 4 / Nhóm C (evaluate/report: Nhóm A)

### Goal

Biến core thành harness quan sát được và tạo machine-readable acceptance evidence.

### Files

- Create: `crates/openvikey-lab/src/cli.rs`
- Create: `src/session.rs`, `src/corpus.rs`, `src/report.rs`, `src/perf.rs`
- Create: `crates/openvikey-lab/tests/cli_smoke.rs`

### Required commands

```text
openvikey-lab type --method telex
openvikey-lab script run <user-script.jsonl>
openvikey-lab model dump <encrypted-model>
openvikey-lab corpus verify --manifest <manifest>
openvikey-lab corpus evaluate --manifest <manifest> --out <report.json>
openvikey-lab perf --out <report.json>
```

### Red

1. Interactive session shows snapshot, candidates, score, state and chosen action.
2. Accept/reject/undo script produces expected model hash.
3. Dump requires passphrase and never dumps before successful authentication.
4. Report includes corpus/config hashes, sample counts, confusion counts, exact formulas, point estimates and Wilson CI.
5. Repeat same script/corpus yields byte-identical JSON report except explicitly excluded runtime timing fields.

### Green

Implement CLI only. No full-screen TUI, installer or OS hook.

### Verify

```powershell
cargo test -p openvikey-lab --test cli_smoke
cargo run -p openvikey-lab -- --help
```

### Commit checkpoint

`feat(lab): add deterministic learning and corpus harness`

---

## 15. Milestone 10 — Quality, security & performance closure — Wave 4 / Nhóm C + A

### Goal

Chỉ gọi v1 complete khi có evidence cho G3/G4, không chỉ unit tests.

### Tasks

1. Chạy release corpus với minimum sample counts.
2. Đóng metric gates:
   - auto precision ≥ 0.99 trên combined labeled stream;
   - correct-token FPR ≤ 0.1%;
   - suggestion top-1 ≥ 0.85, top-3 ≥ 0.95;
   - report coverage/recall theo taxonomy.
3. Benchmark release build:
   - per-key P50 <1ms, P95 <5ms;
   - candidate generation P95 <15ms;
   - startup <300ms;
   - peak memory <150MB;
   - packaged lexicon+model <50MB.
4. Prove save work off typing path and debounce khoảng 2s.
5. Run process in network-disabled release environment after dependencies/assets are present; all lab functions must pass. `cargo deny`/dependency audit confirms không có runtime network stack được chủ động thêm.
6. Test password/terminal/denylist simulation flags.
7. Run full fmt/clippy/test/deny suite from §1.
8. Save evidence:

```text
target/evidence/
├── evaluation.json
├── perf.json
├── dependency-audit.txt
├── provenance-report.json
└── test-summary.txt
```

Không commit `target/evidence`; release note ghi hash và cách reproduce. Nếu metric fail, calibrate chỉ trên calibration split, freeze config version mới, rồi chạy lại held-out đúng một lần cho candidate release.

### Commit checkpoint

`test: close OpenViKey v1 acceptance gates`

---

## 16. Stop conditions & deferred work

Pause và cập nhật spec/ADR trước khi tiếp tục nếu gặp một trong các điều kiện:

- Cần đưa GPL code/data hoặc asset chưa rõ redistribution vào binary/repo.
- Muốn thêm transformer/LLM runtime cho diacritics.
- Core contract cần OS-specific type hoặc direct TSF/CGEvent call.
- Muốn thêm sync, CRDT, OS keyring, GUI/TUI hoặc phrase-level restoration.
- Held-out test đã bị dùng để calibrate; phải tạo split/version mới và ghi provenance.
- Metric chỉ đạt bằng cách tăng auto coverage làm FPR/precision vi phạm gate.

Những việc defer sau v1: Windows TSF adapter, macOS IMK/CGEventTap spike, OS keyring wrappers, cross-device sync/CRDT, phrase/câu diacritics và settings GUI.

---

## 17. Completion checklist

Theo dõi theo **wave** ở §4.1; tick gate khi evidence đủ, không chỉ khi file đã tạo.

**Wave / mốc**

- [x] Baseline M0–M3 xanh trên `main`
- [x] Wave 0 — khóa interface (+ vá golden M3 tuỳ chọn); ADR 0002
- [x] Wave 1 — M4 + M5 + closure review (provenance/split seed, reproducible bigrams, correction action/rule identity)
- [ ] Wave 2 — M6 + 7A + M8
- [ ] Wave 3 — 7B + 7C
- [ ] Wave 4 — M9 + M10; v1 complete

**Gate / acceptance (spec §3, §8, §11)**

- [ ] G0 provenance mở cho mọi asset ship.
- [x] G1 engine ADR được commit (Adopt/wrap `vi-rs`); compatibility gate xanh. Golden closure phủ NFC/NFD matching form, reset, Telex/VNI escape, VNI oà/òa, revision, full backspace prefixes, URL boundary và code/mixed passthrough qua explicit context policy; English heuristics vẫn không thuộc engine (ADR 0002).
- [x] G2 semantic edit/undo contract property tests xanh.
- [ ] Bốn generators có test độc lập.
- [ ] Learning canonical promote/demote/convergence tests xanh.
- [ ] Encrypted store restart/recovery/rewrap tests xanh.
- [ ] Lab CLI và reports tái lập được.
- [ ] G3 corpus quality gates xanh với đúng minimum sample.
- [ ] G4 security/performance gates xanh.
- [ ] README/spec/provenance phản ánh đúng implementation cuối.
