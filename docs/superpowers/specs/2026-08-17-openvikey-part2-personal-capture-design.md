# OpenViKey Phần 2 — Personal Capture-and-Learn (Thiết kế)

- **Ngày:** 2026-08-17
- **Trạng thái:** v2 — **implemented** (lab `session` reducer + REPL; 2026-08-17)
- **Governing spec (phần 1):** [`2026-08-17-openvikey-design.md`](./2026-08-17-openvikey-design.md)
- **Implementation plan phần 1:** [`../plans/2026-08-17-openvikey-v1-implementation-plan.md`](../plans/2026-08-17-openvikey-v1-implementation-plan.md)
- **License:** MIT (kế thừa)

---

## 0. Bối cảnh & động lực

Phần 1 (v1 headless brain) đã xong "bộ não": engine Telex/VNI, 4 generator, mô hình tự tin (promote/demote/decay), store mã hoá, lab CLI. Gate G1/G2/G4 và toàn bộ unit test xanh. **Chỗ duy nhất chặn "v1 complete" là G3 (quality gate)** — đòi held-out corpus thật (≥ 50.000 correct token, ≥ 1.000 ca lỗi, ≥ 200 ca/loại). Mọi nguồn tiếng Việt bên ngoài đều `blocked` vì data license "Unverified" / redistribution "prohibited"; spec cấm nhúng data không rõ quyền. → Phần 1 "dừng vì chưa thu thập thông tin".

**Quyết định của chủ dự án:** thay vì tìm nguồn ngoài, hệ thống **thu thập chính dữ liệu gõ của người dùng** để mô hình cá nhân học trực tiếp — provenance sạch (người dùng sở hữu) và đúng tầm nhìn "mô hình gõ cá nhân hoá". **Phần 2 là năng lực mới, độc lập** — không nhằm đóng G3 lần này; G3 và bộ lọc PII đầy đủ đều **defer**.

### Quyết định đã chốt (brainstorm)

| Chủ đề | Quyết định |
|---|---|
| Mục tiêu chính | Học từ cách gõ **thật** (đặc biệt gõ-xoá-gõ-lại) |
| Surface thu thập | **REPL raw-mode trong `openvikey-lab`**; ghi sự kiện; **không** OS hook |
| Bộ lọc PII | **Defer** — chỉ chừa seam; raw data cô lập trong kho mã hoá |
| "Xong" nghĩa là gì | Năng lực độc lập: bắt gõ thật + học + **nhớ qua restart** + replay tất định |
| Hướng triển khai | Raw-mode live REPL (`crossterm`); **tái dùng core, không sửa core** |

---

## 0.5 Ràng buộc kiến trúc từ review (B1–B5) — *phải theo, không để TDD đoán*

Năm ràng buộc dưới đây đã verify với code thật; chúng là điều kiện đúng-đắn của phần 2. **Toàn bộ nằm trong `openvikey-lab`; KHÔNG sửa `engine/`, `types.rs`, `model.rs`.**

### B1 — LabSession phải sở hữu "document buffer" (committed text)
Engine **không lưu token sau `Commit`** (`Boundary`/boundary-key → `Commit` rồi `reset()`), và **`Backspace` khi composition rỗng trả `[]`** (`engine/mod.rs`). Vì vậy kịch bản học chính `teh␣ → xoá → the` **không mine được nếu chỉ bám `EngineAction`**.
→ `LabSession` giữ một **document buffer** (text đã commit trước caret, theo **token** + delimiter) + một **revision** riêng của lab. Quy tắc:
- `Backspace` khi composition **rỗng** ⇒ lab tự pop grapheme/token cuối khỏi document buffer (engine trả `[]`).
- `ReplaceRange` (auto) và inverse (undo) **áp vào document buffer của lab**, không phải state ẩn nào của engine.
- **Render = document buffer + composition hiện tại + gợi ý** (một dòng inline).

### B2 — Key học implicit phải là `rule_key()` của một candidate có thật
`RuleContextKey.source` là `CandidateSource {TelexFix | Fuzzy | Abbreviation | Diacritics}` — **không có `ImplicitRetype`**. `apply_feedback` **không** đọc `original/replacement` trong event để suy ra identity; identity nằm ở **key caller đưa vào**. Key bịa ⇒ evidence mồ côi, rank/decision không bao giờ thấy.
→ Cơ chế đúng:
1. Khi bắt đầu xoá token X, **chụp lại danh sách candidates** đã sinh cho X (từ `CorrectionSlice` tại thời điểm đó).
2. Khi Y được commit (không caret-break), nếu **Y khớp `candidate.text`** của một candidate trong ảnh chụp → áp `FeedbackKind::ImplicitCorrection{original:X, replacement:Y}` với **đúng `rule_key()` của candidate đó** (dùng đúng `source`, `original_nfc`, `candidate_nfc`, `source_rule_id`).
3. Nếu **không candidate nào** khớp Y → **bỏ**, không học (đừng thêm `source` mới, đừng bịa key).

### B3 — Auto/undo trên đường sống phải truyền `AutoEditContext`
`LabSession` hiện gọi `run_learning_correction_slice(..., None)` ⇒ mọi Auto **tụt xuống Suggest** và `undo` luôn `None`.
→ Khi decision = Auto, lab phải dựng `AutoEditContext{ edit_id, range, delimiter }` với:
- `range.revision == snapshot.revision`, và
- `range.length_grapheme == số grapheme của `snapshot.rendered``,
rồi truyền `Some(edit)` vào `run_learning_correction_slice`. Lab **theo dõi revision** của document buffer để gọi `LearningSession::undo(expected_revision, seq, at_ms, allow_learning)` — `pop_undo` từ chối nếu revision không khớp.

### B4 — Khôi phục con trỏ seq/edit_id sau restart
`apply_feedback` dedup theo `handled_feedback_seqs` (nằm trong payload). Nếu load model rồi **reset `next_seq = 1`**, feedback mới trùng seq cũ ⇒ **no-op** ⇒ "học tiếp" hỏng (phá T3).
→ Kho cá nhân lưu **cursor `next_seq`, `next_edit_id`** trong **header của capture log** (mã hoá). Khi load: khôi phục cursor từ header (không reset về 1). **Bất biến:** mọi `InputEvent`/`FeedbackEvent` áp vào model đều được ghi vào capture log trước/khi áp, nên header cursor luôn ≥ mọi seq trong model.
→ **Ghi rõ giới hạn:** undo log của `LearningSession` **không** persist → **Ctrl+Z không hoàn tác được auto-edit của phiên trước** (chỉ trong phiên hiện tại). Tài liệu hoá, không cố sửa ở phần 2.

### B5 — Ghi X một lần, finish Y đúng lúc
`ImplicitCorrectionMiner.record_deleted_token` **ghi đè** mỗi lần gọi; xoá từng ký tự `teh→te→t` mà gọi mỗi lần sẽ mất token đầy đủ. Engine không báo "token đã commit vừa bị xoá".
→ Hợp đồng: lab gọi `record_deleted_token(X)` **một lần** khi **bắt đầu chuỗi xoá liên tục** ăn vào một token đã commit (X = token đầy đủ lấy từ document buffer, NFC); gọi `finish_replacement(Y)` khi **Y được commit** và **không có caret-break** ở giữa. `CursorMoved/SelectionChanged/Reset` ⇒ `miner.invalidate_due_to_caret_break()` **và** `learning.invalidate_due_to_caret_break()`.

### Lock API (đúng tên code, để TDD không đoán)
- Feedback: `FeedbackKind::ImplicitCorrection { original, replacement }` (biến thể của `FeedbackEvent.kind`).
- Áp feedback: `AdaptiveModel::apply_feedback(&key, &event, allow_learning)` — **không có** `LearningSession::apply_feedback`; lab dùng `session.model_mut().apply_feedback(...)`.
- Auto/undo: `run_learning_correction_slice(..., Some(AutoEditContext{edit_id, range, delimiter}))`; `LearningSession::{undo, record_auto_edit, invalidate_due_to_caret_break}`.
- AutoSettled: lab gọi `LearningSession::observe_input_or_edit(first_feedback_seq, at_ms, allow_learning)` **mỗi input/edit event kế tiếp** để phát `AutoSettled` (+0.3 sau 10 event) — Part 1 vẫn bắt hành vi này.
- Tab/Esc phải giữ **`CorrectionSlice` cuối** để biết candidate; **Accept phải sửa text hiển thị** (áp replacement vào document buffer) chứ không chỉ ghi mass.
- `left_context.prev_token_nfc` phải là **token cuối của document sau auto-replace**, không phải text engine `Commit` thô.

---

## 1. Phạm vi

### 1.1 Trong phạm vi (phần 2)
- Subcommand `openvikey-lab session`: vòng lặp gõ tương tác raw-mode (Telex/VNI), render inline một dòng, có document buffer (B1).
- Bắt **gõ-xoá-gõ-lại** (B2/B5) và **accept/reject/undo** tường minh (B3); áp feedback vào mô hình cá nhân live; wiring AutoSettled.
- **Persist mã hoá** model + capture log qua passphrase; load lại + khôi phục cursor (B4) → nhớ qua restart.
- **Capture log** sự kiện thô (mã hoá); replay tất định tái tạo model.
- Tôn trọng sensitive-context: password/terminal/denylist ⇒ không transform/learning/capture.
- Reducer phiên **tách khỏi I/O terminal** để test headless (không cần TTY).

### 1.2 Ngoài phạm vi (defer)
- **Bộ lọc PII / lệnh `session export|sanitize`**: chỉ tài liệu hoá seam.
- **OS keyboard hook**; **full-screen TUI**; **đóng G3 / "v1 complete"**.
- Sync/CRDT, OS-keyring thật, GUI settings, phrase-level diacritics.
- **Không sửa core** (`engine/`, `types.rs`, `model.rs`, generators, rank, decision).

### 1.3 Non-goals kế thừa (vĩnh viễn)
Zero backend, không tài khoản/telemetry/**automatic upload**. Raw capture log và model plaintext **không rời máy**; chỉ ciphertext do người dùng chủ động export mới có thể rời máy (bộ lọc PII gắn vào seam export ở phần sau).

---

## 2. Tiêu chí thành công (đo được)

Tất cả bằng executable test; ưu tiên headless không cần TTY.

- **T1 — Học gõ-xoá-gõ-lại (B2/B5):** `teh␣ → Backspace… → the␣` sinh `FeedbackEvent::ImplicitCorrection{teh→the}` gắn vào **`rule_key()` của candidate khớp**; evidence dương vào đúng entry; Y không khớp candidate nào ⇒ **không** học; caret-break ở giữa ⇒ **không** mine.
- **T2 — Feedback tường minh (B3):** Tab/Esc/Ctrl+Z tạo đúng `FeedbackKind::{Accept, ExplicitReject, Undo}`; Accept **sửa document buffer**; Undo phát inverse `ReplaceRange` (revision khớp) + evidence âm.
- **T2b — AutoSettled:** sau một auto-edit, gọi `observe_input_or_edit` qua 10 event kế tiếp phát đúng một `AutoSettled` (+0.3); undo/caret-break huỷ pending.
- **T3 — Nhớ qua restart (B4):** phiên A học → save (model + log) → phiên B load cùng passphrase, **khôi phục cursor** → bằng chứng học còn nguyên **và** feedback mới ở phiên B tiếp tục cộng dồn (không bị dedup nuốt).
- **T4 — Replay tất định:** replay capture log → model `to_json_payload()` **SHA-256 == ** model phiên sống. Cùng log ⇒ cùng trạng thái (property).
- **T5 — Sensitive context:** `allow_transform=false, allow_learning=false` ⇒ không candidate, không mutate model, **không** capture entry (hoặc chỉ marker ranh giới không nội dung).
- **T6 — Adapter key-mapping:** bảng phím→`InputEvent` + hotkeys đúng, unit test thuần, không TTY.
- **T7 — Rào chất lượng:** `cargo fmt --check`, `clippy -D warnings`, `test --workspace`, `deny check` xanh; `crossterm` (MIT/Apache-2.0) qua deny allowlist.

Không tiêu chí sample-floor/precision corpus (G3, defer).

---

## 3. Kiến trúc

### 3.1 Tái dùng core, không sửa core
`engine/`, `types.rs`, `model.rs`, generators, `rank.rs`, `decision.rs` **giữ nguyên**. Phần 2 chỉ nối các API đã có:

| API có sẵn | Vai trò |
|---|---|
| `feedback::ImplicitCorrectionMiner` | Mine `X→Y` (được lab gọi theo B5) |
| `model::AdaptiveModel::apply_feedback` | Đã xử lý `ImplicitCorrection` (dương 1.0) + Accept/Reject/Undo/AutoSettled/SuggestionSettled |
| `feedback::LearningSession::{undo, record_auto_edit, observe_input_or_edit, invalidate_due_to_caret_break, model_mut, model}` | Undo log, auto-settle, decay |
| `correction::{run_learning_correction_slice, AutoEditContext, rule_key(private)→dựng lại ở lab}` | generate→rank→decide + auto/undo (B3) |
| `store::file::FileModelStore` + `store::passphrase::PassphraseProvider` + `store::envelope` | Load/save mã hoá (whole-blob, atomic, `.bak` recovery) |
| `persistence::DebouncedSaver::spawn_encrypted` | Save mã hoá background debounce 2s |
| `model::AdaptiveModel::{to,from}_json_payload` | Serialize/deserialize model |

> Ghi chú: `correction::rule_key` là private. Lab dựng `RuleContextKey` từ candidate (đúng field: `input_method, source, original_nfc, candidate_nfc, left_token_nfc, source_rule_id = evidence.split('+').next()`), khớp cách `correction.rs` làm — **không** đổi core.

### 3.2 File thay đổi/mới (chỉ `openvikey-lab`)

**Sửa:** `session.rs` (mở rộng — §3.3), `cli.rs` (subcommand `Session`), `lib.rs` (export), `Cargo.toml` (+`crossterm`), `deny.toml` (allowlist crossterm nếu cần).

**Mới:** `repl.rs` (adapter raw-mode crossterm), `capture.rs` (schema log + cursor header + replay), `document.rs` *(hoặc gộp trong session.rs)* — document buffer B1, `tests/session_capture.rs` (T1–T5, T2b), `tests/repl_keymap.rs` (T6).

**Không** đụng: `crates/openvikey-core/**`.

### 3.3 `LabSession` mở rộng — reducer thuần (không dính terminal)

```text
LabSession
  + document: DocumentBuffer            // committed tokens + delimiter + revision (B1)
  + miner: ImplicitCorrectionMiner
  + last_slice: Option<CorrectionSlice> // giữ candidates cho Tab/Esc + ảnh chụp cho B2
  + next_seq, next_edit_id: u64         // khôi phục từ capture header (B4)
  + capture: Vec<CaptureRecord>
  new_with_model(engine_config, lexicon, model, cursors)   // load model + cursor
  process_key/backspace/boundary(event) -> Observation      // engine + B1 pop + miner + slice
       - Auto ⇒ truyền AutoEditContext (B3), áp ReplaceRange vào document
       - mỗi event ⇒ observe_input_or_edit (AutoSettled)
  accept_top(at_ms)   // áp replacement vào document + Accept feedback (B2 key)
  reject_top(at_ms)   // ExplicitReject
  undo_last(at_ms)    // LearningSession::undo(document.revision) + áp inverse vào document
  model_payload() ; capture_header()->{next_seq,next_edit_id} ; drain_capture()
```

- `left_context.prev_token_nfc` = token cuối **document sau auto-replace**.
- Reducer không đọc wall-clock; `at_ms` do adapter truyền vào.

### 3.4 Adapter raw-mode (`repl.rs`)
- `crossterm` bật raw mode **sau khi** đã prompt passphrase (ẩn input).
- Ánh xạ phím → `InputEvent`:

| Phím | Hành động |
|---|---|
| ký tự in được | `Key{logical}` |
| Backspace | `Backspace` (composition rỗng ⇒ lab pop document — B1) |
| Enter / Space | `Boundary{delimiter}` (chốt Space-vs-Enter ở plan; B1/B5 commit-behavior đã cố định) |
| Esc | `reject_top` |
| Tab | `accept_top` |
| Ctrl+Z | `undo_last` |
| Ctrl+D / Ctrl+C | thoát: flush model + capture log |

- `seq`/`edit_id` cấp từ cursor lab (khởi từ header); `at_ms` = wall-clock (lab sở hữu timing).
- Render **inline một dòng**: `document + composition + [gợi ý] + state`. Không alternate-screen/panel.
- Adapter **mỏng**: mọi quyết định học nằm trong `LabSession`.

### 3.5 Capture log (`capture.rs`) — tách hẳn `script run`
- Envelope store **ghi cả blob, không append** → lab **buffer JSONL trong RAM** rồi flush **file thứ hai** (khác file model), **cùng passphrase**, **`DebouncedSaver` riêng**.
- Blob = header + records:

```text
CaptureLog { header: { v:1, next_seq, next_edit_id }, records: [ CaptureRecord ] }
CaptureRecord = Input(InputEvent) | Feedback(FeedbackEvent)
```

- `replay(log) -> AdaptiveModel`: dựng lại `LabSession` (model rỗng) và phát lại từng record qua **đúng pipeline reducer** → model cuối. Dùng cho T4 và (tương lai) học-lại-sau-khi-lọc-PII.
- **Không** trộn với `script run` (ops cấp thấp, tự khai `RuleContextKey`). Hai hệ độc lập.

---

## 4. Luồng dữ liệu (phiên sống)

```
key raw (crossterm) → InputEvent{seq=cursor, at_ms=wall, kind, context}
  → capture.push(Input)
  → LabSession.process:
        engine.process → actions
        Backspace & composition rỗng → document.pop ; miner.record_deleted_token(X) [một lần, B5]
        (generate → rank → decide) trên snapshot hiện tại
           Auto  → AutoEditContext (B3) → ReplaceRange áp vào document + record_auto_edit
           Suggest → giữ last_slice để render
        commit token Y (Boundary) → document.push(Y) ; miner.finish_replacement(Y) → nếu khớp candidate ⇒ apply_feedback(rule_key, ImplicitCorrection) [B2]
        observe_input_or_edit(...) → AutoSettled nếu tới hạn
  → render inline: document + composition + [suggestions] + state
hotkey: Tab=accept_top(áp document) · Esc=reject_top · Ctrl+Z=undo_last(áp inverse)
thoát (Ctrl+D): saver_model.flush(model_payload) ; saver_log.flush(capture_blob{header cursor})
khởi động: prompt passphrase → nếu file tồn tại: load model + log(header→cursor) ; else default+cursor=1 → load lexicon → loop
```

---

## 5. Persistence & tất định

- **Hai file mã hoá** cùng passphrase: `model` (payload JSON) và `capture-log` (blob header+records). Mỗi file một `DebouncedSaver`. Whole-blob rewrite (envelope), atomic + `.bak` recovery (tái dùng `store`).
- **Khởi động (B4 + lock):** `FileModelStore::load` khi **thiếu file** trả `StoreError::Io` (không phải "Empty") → lab **kiểm `path.exists()` trước**; không tồn tại ⇒ `AdaptiveModel::default()` + cursor=1. **Sai passphrase** trên file có thật ⇒ `StoreError::WrongPassphrase`, **không** tạo file mới đè, không reset. Model có mà log thiếu (bất nhất) ⇒ **báo lỗi rõ**, không đoán cursor.
- `at_ms` stamp lúc capture, lưu trong log ⇒ replay tất định (core không đọc wall-clock; giữ guarantee §3.3 phần 1).
- Save qua debounce 2s, không block đường gõ (§11 phần 1).

---

## 6. Riêng tư (defer bộ lọc, giữ seam)
- **Bất biến:** raw capture log + model plaintext **không tự rời máy**; chỉ tồn tại dạng ciphertext local.
- **Sensitive context:** `--context password|terminal|denylist` ⇒ `false/false` ⇒ không candidate/mutate/**capture** (T5).
- **Seam tương lai (không implement):** `session export --sanitize` là chỗ **duy nhất** dữ liệu (đã lọc PII) rời kho; phần 2 chỉ cô lập raw data trong kho mã hoá + đặt `// TODO(privacy-filter)` ở ranh giới export. Không có đường xuất plaintext nào trong phần 2.

---

## 7. Xử lý lỗi
- Sai passphrase / header hỏng / version không hỗ trợ → lỗi rõ (`StoreError`/`ModelError`), **không** vào phiên, không đè file.
- **stdin không TTY** (piped/CI): adapter raw-mode chỉ chạy khi có TTY; reducer tách rời nên test bơm sự kiện tổng hợp. Không TTY ⇒ CLI báo lỗi rõ, không treo. Non-raw fallback: **không** làm ở phần 2 trừ khi plan thấy cần cho smoke.
- Lỗi save khi thoát → exit code ≠ 0, giữ `.bak`. Model và log flush độc lập, báo lỗi riêng; lỗi ghi log **không** làm hỏng model đã save.

---

## 8. Test (TDD)

| ID | Test | File | TTY? |
|---|---|---|---|
| T1 | Implicit `X→Y` gắn `rule_key()` candidate; không khớp ⇒ bỏ; caret-break huỷ | `session_capture.rs` | Không |
| T2 | Accept(sửa document)/Reject/Undo(revision khớp) đúng ngữ nghĩa | `session_capture.rs` | Không |
| T2b | AutoSettled +0.3 sau 10 event; undo/caret-break huỷ pending | `session_capture.rs` | Không |
| T3 | Restart: khôi phục cursor, học tiếp không bị dedup | `session_capture.rs` | Không |
| T4 | Replay log → model SHA-256 khớp phiên sống (property) | `session_capture.rs` | Không |
| T5 | Sensitive context: không candidate/mutate/capture | `session_capture.rs` | Không |
| T6 | Bảng key→InputEvent + hotkeys | `repl_keymap.rs` | Không |
| T7 | fmt/clippy/test/deny xanh; crossterm allowlisted | CI | — |

---

## 9. Dependencies, provenance & deny
- Thêm `crossterm` (MIT/Apache-2.0 dual) vào `crates/openvikey-lab/Cargo.toml`.
- **Provenance:** verifier hiện chỉ hỗ trợ artifact **repo-file** hoặc **`cargo-git:`**; các crate crates.io thường (serde, clap, sha2, thiserror…) **không** có record provenance — chúng do **`cargo deny`** quản license. → `crossterm` xử lý **y hệt**: đảm bảo MIT/Apache-2.0 trong `deny.toml` allowlist; **KHÔNG** thêm record vào `provenance.toml` (thêm sẽ khiến `provenance verify` fail vì không có đường hash registry crate). Provenance registry giữ nguyên vai trò: git-dep đặc biệt (`vi-rs`) + data asset.
- **Không** thêm runtime network stack nào (giữ gate no-network của phần 1).

---

## 10. Mốc triển khai (writing-plans chi tiết hoá TDD)

Tuần tự, mỗi mốc một commit nhỏ, TDD, build xanh:

1. **P2-M1 — Document buffer + reducer + implicit mining (B1/B2/B5):** mở rộng `LabSession` (document buffer, miner theo hợp đồng B5, apply_feedback qua candidate rule_key B2, last_slice), `capture.rs` schema/replay. Test T1, T4, T5. *Headless, không crossterm.*
2. **P2-M2 — Auto/undo sống + AutoSettled + persist restart (B3/B4):** truyền `AutoEditContext`, undo theo revision, `observe_input_or_edit`; hai `DebouncedSaver` + cursor header + load/khôi phục cursor. Test T2, T2b, T3.
3. **P2-M3 — Adapter raw-mode + CLI:** `repl.rs` (crossterm), subcommand `Session`, prompt passphrase (ẩn) trước raw mode, render inline, hotkeys. Test T6; smoke thủ công.
4. **P2-M4 — Deny + đóng gói:** thêm `crossterm` vào deny allowlist; **ADR 0006** ghi surface lab mới (để không ghi đè thầm ADR 0005); full rào chất lượng (T7); cập nhật README/AGENTS nếu cần.

P2-M1/M2 hoàn toàn headless (không phụ thuộc crossterm) → phần lõi rủi-ro-thấp làm trước; adapter terminal tách sau, không chặn logic học.

---

## 11. Stop conditions (dừng, cập nhật spec/ADR trước khi tiếp)
- Cần implement thuật toán lọc PII / xuất dữ liệu ra ngoài.
- Cần đụng `engine/`, `types.rs`, `model.rs` (đổi contract/thêm field) — dừng, mở ADR.
- Cần OS hook, full-screen TUI, sync/CRDT, OS-keyring, LLM/transformer.
- Muốn dùng dữ liệu capture để tick G3 (đổi phạm vi → brainstorm lại).
- `crossterm`/transitive kéo license không tương thích → dừng, tìm thay thế.

---

## 12. Bước tiếp theo
Sau khi chủ dự án duyệt bản v2 này: kích hoạt **writing-plans** để biến §10 (kèm ràng buộc §0.5) thành implementation plan TDD chi tiết (red/green/verify từng mốc), lập **ADR 0006** cho surface lab mới, rồi triển khai.
