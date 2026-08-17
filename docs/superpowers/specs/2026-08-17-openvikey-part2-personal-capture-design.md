# OpenViKey Phần 2 — Personal Capture-and-Learn (Thiết kế)

- **Ngày:** 2026-08-17
- **Trạng thái:** v1 — implementation-ready (chờ writing-plans)
- **Governing spec (phần 1):** [`2026-08-17-openvikey-design.md`](./2026-08-17-openvikey-design.md)
- **Implementation plan phần 1:** [`../plans/2026-08-17-openvikey-v1-implementation-plan.md`](../plans/2026-08-17-openvikey-v1-implementation-plan.md)
- **License:** MIT (kế thừa)

---

## 0. Bối cảnh & động lực

Phần 1 (v1 headless brain) đã xong "bộ não": engine Telex/VNI, 4 generator, mô hình tự tin (promote/demote/decay), store mã hoá, lab CLI. Gate G1/G2/G4 và toàn bộ unit test xanh. **Chỗ duy nhất chặn "v1 complete" là G3 (quality gate)** — đòi held-out corpus thật (≥ 50.000 correct token, ≥ 1.000 ca lỗi, ≥ 200 ca/loại). Mọi nguồn tiếng Việt bên ngoài (`underthesea`, `restore_vietnamese_diacritics`) đều `blocked` vì data license "Unverified" / redistribution "prohibited"; spec cấm nhúng data không rõ quyền. → Phần 1 "dừng vì chưa thu thập thông tin": **không có corpus ngoài nào hợp lệ pháp lý.**

**Quyết định của chủ dự án:** thay vì tìm nguồn ngoài, hệ thống **thu thập chính dữ liệu gõ của người dùng** để mô hình cá nhân học trực tiếp. Điều này (a) provenance sạch (người dùng sở hữu dữ liệu của mình), và (b) đúng tầm nhìn cốt lõi — "mô hình gõ chữ cá nhân hoá, như nét chữ tay".

**Phần 2 là một năng lực mới, độc lập** — không nhằm đóng G3/"v1 complete" trong lần này. G3 và bộ lọc PII đầy đủ để defer.

### Quyết định đã chốt (brainstorm)

| Chủ đề | Quyết định |
|---|---|
| Mục tiêu chính | Học từ cách gõ **thật** của người dùng (đặc biệt gõ-xoá-gõ-lại) |
| Surface thu thập | **REPL tương tác trong `openvikey-lab`**, raw-mode, ghi sự kiện; **không** OS hook |
| Bộ lọc PII | **Defer** — chỉ chừa seam; raw data cô lập trong kho mã hoá |
| "Xong" nghĩa là gì | Năng lực độc lập: bắt gõ thật + học + **nhớ qua restart** + replay tất định |
| Hướng triển khai | Raw-mode live REPL (`crossterm`), tái dùng tối đa cơ chế phần 1 |

---

## 1. Phạm vi

### 1.1 Trong phạm vi (v1 phần 2)
- Subcommand `openvikey-lab session`: vòng lặp gõ tương tác raw-mode (Telex/VNI), render inline một dòng.
- Bắt **gõ-xoá-gõ-lại** (implicit correction) và **accept/reject/undo** tường minh; áp feedback vào mô hình cá nhân live.
- **Persist mã hoá** mô hình cá nhân qua passphrase; load lại ở phiên sau → nhớ qua restart.
- **Capture log** sự kiện thô (mã hoá); replay tất định tái tạo mô hình.
- Tôn trọng sensitive-context contract: password/terminal/denylist ⇒ không transform/learning/capture.
- Reducer phiên **tách khỏi I/O terminal** để test headless (không cần TTY).

### 1.2 Ngoài phạm vi (defer — không làm ở phần 2)
- **Bộ lọc PII / lệnh `session export|sanitize`**: chỉ tài liệu hoá seam; không implement thuật toán lọc.
- **OS keyboard hook** (TSF/CGEventTap): vẫn headless; chỉ raw-mode terminal.
- **Full-screen TUI**: chỉ render inline một dòng.
- **Đóng G3 / "v1 complete"**: giữ nguyên trạng thái blocked của phần 1.
- Sync/CRDT, OS-keyring thật, GUI settings, phrase-level diacritics.

### 1.3 Non-goals kế thừa (vĩnh viễn)
Zero backend, không tài khoản/telemetry/**automatic upload**. Raw capture log và mô hình plaintext **không rời máy**; chỉ ciphertext do người dùng chủ động export mới có thể rời máy (bộ lọc PII sẽ gắn vào đúng seam export ở phần sau).

---

## 2. Tiêu chí thành công (đo được)

Tất cả bằng **executable test**; ưu tiên test headless không cần TTY.

- **T1 — Học gõ-xoá-gõ-lại:** chuỗi `Key…Backspace…retype` sinh đúng `FeedbackEvent::ImplicitCorrection{original→replacement}` và đẩy evidence dương vào **đúng rule-context key**; caret/selection break ở giữa ⇒ **không** mine.
- **T2 — Feedback tường minh:** hotkey Accept/Reject/Undo tạo đúng `FeedbackEvent::{Accept, ExplicitReject, Undo}` và áp vào mô hình đúng ngữ nghĩa (undo phát inverse `ReplaceRange` + evidence âm).
- **T3 — Nhớ qua restart:** phiên A học một phép sửa → save mã hoá → phiên B load cùng passphrase → bằng chứng học **còn nguyên** (evidence/confidence cho rule-context khớp) và mô hình tiếp tục học tiếp được.
- **T4 — Replay tất định:** capture log của phiên → hàm replay tái dựng mô hình có **`to_json_payload()` SHA-256 == ** payload mô hình phiên sống (loại trừ trường timing runtime nếu có). Cùng log + cùng `evaluate_at_ms` ⇒ cùng trạng thái (property test).
- **T5 — Sensitive context:** với `allow_transform=false, allow_learning=false`, phiên **không** sinh candidate, **không** mutate mô hình, **không** ghi capture entry (hoặc chỉ ghi marker ranh giới không chứa nội dung).
- **T6 — Adapter key-mapping:** bảng ánh xạ phím→`InputEvent` (Key/Backspace/Enter=Boundary/Reset và hotkeys) đúng, kiểm bằng unit test thuần, không cần TTY.
- **T7 — Rào chất lượng:** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, `cargo deny check` xanh; `crossterm` nằm trong allowlist license + provenance.

Không có tiêu chí về sample-floor/precision corpus (đó là G3, defer).

---

## 3. Kiến trúc

### 3.1 Nguyên tắc tái dùng (không đụng contract lõi)
`types.rs` và `engine/` **đóng băng** (không đổi nghĩa contract). Phần 2 **không** thêm logic lớn vào `openvikey-core`; nó nối các thành phần đã có:

| Thành phần có sẵn | Vai trò trong phần 2 |
|---|---|
| `feedback::ImplicitCorrectionMiner` | Mine gõ-xoá-gõ-lại `X→Y` (đã có; **chỉ cần được gọi**) |
| `model::AdaptiveModel::apply_feedback` | Đã xử lý `ImplicitCorrection` (dương 1.0) + Accept/Reject/Undo/SuggestionSettled |
| `feedback::LearningSession` | Undo log, auto-settle, record_auto_edit, invalidate caret-break |
| `store::file::FileModelStore` + `store::passphrase::PassphraseProvider` | Load/save mã hoá (envelope, atomic, recovery) |
| `model::AdaptiveModel::{to,from}_json_payload` | Serialize/deserialize mô hình |
| `persistence::DebouncedSaver::spawn_encrypted` | Save mã hoá background debounce 2s (`submit`/`flush`) |
| `correction::run_learning_correction_slice` | Đường generate→rank→decide+learning đã dùng trong `LabSession` |

### 3.2 File thay đổi/mới (Nhóm C sở hữu: lab + store là core đã ổn định)

**Sửa:**
- `crates/openvikey-lab/src/session.rs` — mở rộng `LabSession` (xem §3.3).
- `crates/openvikey-lab/src/cli.rs` — thêm subcommand `Session`.
- `crates/openvikey-lab/src/lib.rs` — export module mới.
- `deny.toml` / `data/provenance.toml` — thêm `crossterm`.
- `crates/openvikey-lab/Cargo.toml` — thêm dependency `crossterm`.

**Mới:**
- `crates/openvikey-lab/src/repl.rs` — adapter terminal raw-mode (crossterm): key→`InputEvent`, render inline, hotkeys, vòng lặp.
- `crates/openvikey-lab/src/capture.rs` — schema capture log + replay tất định.
- `crates/openvikey-lab/tests/session_capture.rs` — T1–T5 (headless, không TTY).
- `crates/openvikey-lab/tests/repl_keymap.rs` — T6.

**Không** đụng: `engine/`, `types.rs`, các generator, `rank.rs`, `decision.rs` logic.

### 3.3 `LabSession` mở rộng — "reducer thuần" (không dính terminal)

`LabSession` hiện: engine + lexicon + abbrev + `LearningSession` + left_context; chỉ `type_text`/`process_event`, luôn `AdaptiveModel::default()`, không feedback, không capture.

Mở rộng (giữ API cũ ổn định; thêm mới):

```text
LabSession
  + new_with_model(engine_config, lexicon, model)      // khởi tạo từ model đã load
  + miner: ImplicitCorrectionMiner                     // drive khi backspace→retype
  + capture: Vec<CaptureRecord>                         // buffer sự kiện (in-memory)
  process_event(&InputEvent) -> SessionObservation      // (đã có) + ghi capture + drive miner
  apply_feedback(FeedbackKind, at_ms) -> ...            // accept/reject/undo tường minh
  accept_current_suggestion(candidate_id, at_ms)
  reject_current_suggestion(candidate_id, at_ms)
  undo_last_auto(at_ms) -> Option<UndoOutcome>
  model_payload() -> Result<Vec<u8>, ModelError>        // (đã có) để save
  drain_capture() -> Vec<CaptureRecord>                 // để flush ra log
```

Quy tắc mining gõ-xoá-gõ-lại trong `process_event`:
- Khi engine commit/xoá token X rồi người dùng gõ lại token Y (không caret-break ở giữa) → gọi `miner.record_deleted_token(X)` … `miner.finish_replacement(Y)` → nếu ra `ImplicitCorrection` thì `LearningSession`/model.apply_feedback với rule-context `X→Y`.
- `CursorMoved`/`SelectionChanged`/`Reset` → `miner.invalidate_due_to_caret_break()` **và** `learning.invalidate_due_to_caret_break()`.
- Ranh giới rule-context key cho implicit: `{input_method, correction_type=ImplicitRetype, original_nfc=X, candidate_nfc=Y, prev_token?, source_rule_id}` — nhất quán §6.1 phần 1.

> Ghi chú wiring: cách xác định "token X vừa bị xoá" bám vào `EngineAction` (Commit/UpdateComposition) + backspace; chi tiết đường nối X/Y để plan chốt bằng TDD. Không đổi ngữ nghĩa engine.

### 3.4 Adapter raw-mode (`repl.rs`)
- Dùng `crossterm` (MIT, hỗ trợ Windows tốt) để bật raw mode và đọc `KeyEvent`.
- Ánh xạ phím → `InputEvent`:

| Phím | `InputKind` / hành động |
|---|---|
| ký tự in được | `Key{logical, physical:None}` |
| Backspace | `Backspace` |
| Enter | `Boundary{delimiter:"\n"}` (hoặc space cho word-boundary — chốt ở plan) |
| Space | `Boundary{delimiter:" "}` hoặc `Key{' '}` (chốt ở plan theo hành vi engine hiện tại) |
| Esc | ExplicitReject suggestion đang hiển thị |
| Tab | Accept suggestion top-1 đang hiển thị |
| Ctrl+Z | Undo auto-edit gần nhất |
| Ctrl+D / Ctrl+C | Thoát: flush model + capture log |

- `seq` tăng đơn điệu; `at_ms` = wall-clock (lab sở hữu timing — core vẫn không đọc clock).
- Render **inline một dòng**: `rendered` hiện tại + `[gợi ý: …]` + trạng thái decision. Không alternate-screen, không panel.
- Adapter **mỏng**: mọi quyết định học/sửa nằm trong `LabSession` (reducer). Adapter chỉ: đọc key → tạo `InputEvent`/gọi feedback API → in.

### 3.5 Capture log (`capture.rs`)
- Schema JSONL, versioned:

```text
CaptureRecord =
    Input(InputEvent)
  | Feedback(FeedbackEvent)
# mỗi dòng: {"v":1,"kind":"input"|"feedback", ...payload}
```

- Ghi theo đúng thứ tự thời gian; `at_ms` lưu trong record → replay tất định.
- `replay(log) -> AdaptiveModel`: dựng lại `LabSession` (model rỗng) và phát lại từng record qua đúng pipeline → mô hình cuối. Dùng cho T4 và cho việc "học lại từ log" sau khi (tương lai) lọc PII.
- Log được **mã hoá** khi ghi ra đĩa (dùng cùng `FileModelStore`/passphrase như model, hoặc một file envelope riêng — chốt ở plan; mặc định: file riêng, cùng provider).

---

## 4. Luồng dữ liệu (phiên sống)

```
key raw (crossterm)
  → InputEvent{seq, at_ms(wall), kind, context}          [lab timing]
  → capture.push(Input)
  → LabSession.process_event:
        engine.process → snapshot + actions
        (backspace→retype?) → ImplicitCorrectionMiner → model.apply_feedback(X→Y)
        generate → rank → decide (auto/suggest/ignore)
           auto    → ReplaceRange + learning.record_auto_edit (undo log)
           suggest → giữ candidates để render
  → render inline: rendered + [suggestions] + state

hotkey:
  Tab    → accept_current_suggestion  → capture.push(Feedback::Accept)  → model
  Esc    → reject_current_suggestion  → capture.push(Feedback::ExplicitReject) → model
  Ctrl+Z → undo_last_auto             → inverse ReplaceRange + Feedback::Undo → model
  Enter  → Boundary(commit)           → advance left-context

thoát (Ctrl+D):
  → DebouncedSaver.flush(model_payload)         [mã hoá, atomic + backup]
  → flush capture log                            [mã hoá]

khởi động:
  → prompt passphrase → FileModelStore.load(model) nếu tồn tại (else default)
  → load lexicon → vòng lặp
```

---

## 5. Persistence & tất định

- Model + capture log mã hoá bằng **cùng passphrase** (một `PassphraseProvider`). Argon2id/XChaCha20-Poly1305 theo store hiện có; atomic write + `.bak` recovery.
- Save model qua `DebouncedSaver::spawn_encrypted` (debounce 2s, `submit` mỗi khi model đổi, `flush` lúc thoát) — **không block đường gõ** (đúng §11 phần 1).
- `at_ms` stamp lúc capture và **lưu trong log**; replay dùng đúng `at_ms` ⇒ tái tạo mô hình y hệt (core không đọc wall-clock — giữ guarantee convergence §3.3 phần 1).
- Determinism gate: `to_json_payload()` của (phiên sống lúc thoát) và (replay từ log) **bằng nhau** → T4.

---

## 6. Riêng tư (defer bộ lọc, giữ seam)

- **Bất biến:** raw capture log + model plaintext **không tự rời máy**; chỉ tồn tại dưới dạng ciphertext mã hoá local.
- **Sensitive context:** `session --context password|terminal|denylist` ⇒ `allow_transform=false, allow_learning=false` ⇒ không candidate/mutate/**capture** (T5). Tuỳ chọn: ghi một marker ranh giới không nội dung để giữ tính liền mạch của replay.
- **Seam tương lai (không implement):** lệnh `session export --sanitize <out>` sẽ là chỗ duy nhất dữ liệu (đã lọc PII) rời kho mã hoá. Phần 2 chỉ:
  - Cô lập raw data trong kho mã hoá (đã đạt qua §5).
  - Tài liệu hoá seam + đặt `// TODO(privacy-filter)` ở ranh giới export.
  - **Không** có đường nào xuất plaintext ra ngoài trong phần 2.

---

## 7. Xử lý lỗi

- Sai passphrase / header hỏng / version không hỗ trợ → lỗi rõ ràng (tái dùng `StoreError`), **không** vào phiên, không tạo file mới đè.
- **stdin không phải TTY** (piped/CI): adapter raw-mode chỉ chạy khi có TTY. Reducer (`LabSession`) tách rời nên test bơm chuỗi sự kiện tổng hợp; khi không TTY, CLI báo lỗi rõ ("session cần terminal tương tác") — không tự rơi vào vòng lặp treo. (Chế độ non-raw fallback: **không** làm ở phần 2 trừ khi plan thấy cần cho smoke-test.)
- Lỗi save lúc thoát → surface lỗi (exit code ≠ 0), giữ `.bak`.
- Capture log ghi lỗi → không được làm mất model đã save; hai flush độc lập, báo lỗi riêng.

---

## 8. Test (TDD)

| ID | Test | File | Cần TTY? |
|---|---|---|---|
| T1 | Implicit gõ-xoá-gõ-lại → evidence đúng rule-context; caret-break huỷ mine | `session_capture.rs` | Không |
| T2 | Accept/Reject/Undo tường minh → đúng FeedbackEvent + ngữ nghĩa undo | `session_capture.rs` | Không |
| T3 | Nhớ qua restart: save→reload, bằng chứng học còn | `session_capture.rs` | Không |
| T4 | Replay log → model SHA-256 khớp phiên sống (property) | `session_capture.rs` | Không |
| T5 | Sensitive context: không candidate/mutate/capture | `session_capture.rs` | Không |
| T6 | Bảng key→InputEvent + hotkeys đúng | `repl_keymap.rs` | Không |
| T7 | fmt/clippy/test/deny xanh; crossterm allowlisted | CI | — |

Nguyên tắc: adapter terminal mỏng đến mức không cần integration test TTY; toàn bộ hành vi học kiểm qua reducer headless.

---

## 9. Dependencies, provenance & deny

- Thêm `crossterm` (license **MIT**) vào `crates/openvikey-lab/Cargo.toml`.
- `deny.toml`: đảm bảo MIT nằm trong allowlist (đã có cho MIT); thêm `crossterm` + phụ thuộc transitive nếu deny báo.
- `data/provenance.toml`: thêm record `crossterm` (`kind = code_dependency`, revision pin theo `Cargo.lock`, `code_license = MIT`, `redistribution = allowed`, `purpose = "Interactive raw-mode capture REPL (phần 2)"`, `status = approved`).
- **Không** thêm bất kỳ runtime network stack nào (giữ gate no-network của phần 1).

---

## 10. Mốc triển khai (giao writing-plans chi tiết hoá)

Tuần tự, mỗi mốc một commit nhỏ, TDD, build xanh:

1. **P2-M1 — Capture reducer & implicit mining:** mở rộng `LabSession` (miner + capture buffer + apply_feedback tường minh) + `capture.rs` schema/replay. Test T1, T2, T4, T5. *Chưa cần terminal.*
2. **P2-M2 — Persist qua restart:** load/save model mã hoá + `DebouncedSaver` trong đường session; flush capture log mã hoá. Test T3.
3. **P2-M3 — Adapter raw-mode + CLI:** `repl.rs` (crossterm), subcommand `Session`, prompt passphrase, render inline, hotkeys. Test T6; smoke thủ công.
4. **P2-M4 — Provenance/deny + đóng gói:** thêm `crossterm` vào deny/provenance; chạy full rào chất lượng (T7); cập nhật README/AGENTS nếu cần.

Ghi chú: P2-M1 và P2-M2 hoàn toàn headless (không phụ thuộc crossterm) → phần rủi ro-thấp, giá-trị-cao làm trước; adapter terminal (P2-M3) tách sau, không chặn logic học.

---

## 11. Stop conditions (dừng, cập nhật spec/ADR trước khi tiếp)

- Cần implement thuật toán lọc PII / xuất dữ liệu ra ngoài (đó là phần sau — dừng nếu định làm ở đây).
- Cần đụng `types.rs`/`engine/` để đổi nghĩa contract.
- Cần thêm OS hook, full-screen TUI, sync/CRDT, OS-keyring, hay LLM/transformer.
- Muốn dùng dữ liệu capture để tick G3 (đổi phạm vi → quay lại brainstorm).
- `crossterm` hoặc transitive dep kéo theo license không tương thích MIT → dừng, tìm thay thế.

---

## 12. Bước tiếp theo

Kích hoạt **writing-plans** để biến §10 thành implementation plan chi tiết (red/green/verify cho từng mốc), rồi triển khai theo TDD.
