# OpenViKey — Ghi nhận hiện trạng kiến trúc

- **Ngày khảo sát:** 2026-08-23
- **Checkout:** `brainstorm/next`
- **Phạm vi:** workspace Rust, luồng runtime Windows, learning model, persistence, lab và TSF optional
- **Tính chất:** khảo sát read-only; không thay đổi source production

## 1. Kết luận điều hành

OpenViKey hiện có kiến trúc phân lớp gần với mô hình **domain core + adapters**. Phần domain, learning và semantic edit được tách khỏi Windows tương đối rõ; adapter Windows chỉ phân loại input/context, gọi session và thi hành semantic action bằng `SendInput`.

Sản phẩm Windows hiện hành là một ứng dụng standalone kiểu UniKey:

- một `OpenViKey.exe` chạy nền;
- dùng low-level keyboard/mouse hook;
- inject text bằng `SendInput`;
- có tray, overlay và Settings trong cùng process;
- không đăng ký TSF, không xuất hiện trong `Win + Space` và không yêu cầu quyền Administrator.

TSF vẫn còn trong workspace như artifact nghiên cứu/compatibility, nhưng không nằm trong product package mặc định.

Đánh giá tổng thể:

- nền domain và learning có ranh giới tốt;
- các contract quan trọng được version hóa và kiểm thử;
- privacy/safety được đặt trước transform và learning;
- `openvikey-session` và Windows shell đang là hai điểm tập trung complexity;
- dự án vẫn ở trạng thái preview, chưa đủ điều kiện production release.

## 2. Bản đồ workspace

Workspace được khai báo tại [`Cargo.toml`](../../Cargo.toml) và gồm sáu crate.

| Crate | Trách nhiệm chính | Phụ thuộc nội bộ |
|---|---|---|
| `openvikey-core` | Engine Telex/VNI, candidate generation, ranking, intervention planner, adaptive model và encrypted store | Không |
| `openvikey-session` | Stateful reducer nối engine, document, correction, feedback, undo, capture và replay | `openvikey-core` |
| `openvikey-lab` | CLI/harness, corpus, evaluation, performance, provenance và encrypted REPL | `openvikey-core`, `openvikey-session` |
| `openvikey-win` | Product executable, hooks, context safety, injector, tray, overlay, Settings và plaintext preview persistence | `openvikey-core`, `openvikey-session`, `openvikey-win-context` |
| `openvikey-win-context` | Context/identity contracts thuần, codec và cache dùng chung | Không |
| `openvikey-win-tsf` | Read-only TSF context provider dùng cho research/compatibility | `openvikey-win-context`; dùng `openvikey-win` trong test/dev |

Quy mô source Rust tại thời điểm khảo sát:

| Crate | File Rust | Dòng Rust xấp xỉ |
|---|---:|---:|
| `openvikey-core` | 47 | 12.322 |
| `openvikey-lab` | 25 | 5.796 |
| `openvikey-session` | 6 | 2.801 |
| `openvikey-win` | 50 | 14.105 |
| `openvikey-win-context` | 3 | 599 |
| `openvikey-win-tsf` | 4 | 1.482 |

## 3. Kiến trúc runtime Windows

```text
Physical keyboard/mouse
          │
          ▼
WH_KEYBOARD_LL / WH_MOUSE_LL / WinEvent focus
          │
          ├─ FocusCache + focus generation
          ├─ sensitive-field ContextProjection
          └─ lock-free KeyDecision: Pass / Eat / Hotkey
                          │
                          ▼
                    TypingHost
                          │
                          ▼
                  openvikey-session
        engine → generators → rank → planner
                          │
                          ▼
                  semantic EngineAction
                          │
                          ▼
           sync commands → UTF-16 SendInput
                          │
             ┌────────────┴────────────┐
             ▼                         ▼
       focused application       overlay/tray/UI
                                       │
                                       ▼
                            debounced coherent save
                                       │
                                       ▼
                              %LOCALAPPDATA%\OpenViKey
```

### 3.1 Startup và shutdown

Coordinator chính nằm ở [`crates/openvikey-win/src/main.rs`](../../crates/openvikey-win/src/main.rs).

Startup hiện tại:

1. Acquire per-user single-instance mutex.
2. Parse development CLI options.
3. Load packaged/development lexicon.
4. Load và validate `settings.json`.
5. Load coherent model/capture pair.
6. Tạo đúng một `LabSession` và đặt nó trong `TypingHost`.
7. Tạo focus cache và context projection slot.
8. Start standalone sensitive-field worker và chờ initial verdict.
9. Start debounced persistence worker.
10. Tạo tray, control window và overlay.
11. Cài keyboard, mouse và focus hooks.
12. Chạy Win32 message loop.

Shutdown hiện tại:

1. Thoát message loop.
2. Drop keyboard/mouse/focus hooks.
3. Dừng context worker và UI host.
4. Flush coherent model/capture snapshot.
5. Đồng bộ runtime state về settings.
6. Lưu settings và release single-instance resources.

Thứ tự này tránh trạng thái hook còn sống trong khi session/persistence đã bị tháo.

### 3.2 Keyboard hot path

Luồng hot path được chia thành ba lớp:

1. [`policy.rs`](../../crates/openvikey-win/src/policy.rs) phân loại `RawKey` thành `KeyDecision`.
2. [`host.rs`](../../crates/openvikey-win/src/host.rs) đồng bộ focus/context và gọi session bằng `try_lock`.
3. [`sync.rs`](../../crates/openvikey-win/src/sync.rs) chuyển `EngineAction` thành `InjectCommand`; [`inject.rs`](../../crates/openvikey-win/src/inject.rs) thi hành bằng `SendInput`.

Các invariant đáng chú ý:

- hook callback không làm I/O hoặc serialize;
- focus/context cache được đọc bằng non-blocking access;
- session dùng `Mutex::try_lock` thay vì blocking lock;
- own `SendInput` event có marker `OVK_EXTRA` và luôn pass;
- khi injection thất bại, session được restore từ checkpoint;
- caret/focus break hủy composition, undo và reopen anchor đã stale;
- hai hook-based Vietnamese input method chạy đồng thời là unsupported.

### 3.3 Sensitive-field guard

Standalone context worker nằm tại [`sensitive.rs`](../../crates/openvikey-win/src/sensitive.rs). Worker phân loại field theo thứ tự an toàn:

1. executable denylist;
2. Win32 `ES_PASSWORD` style;
3. UI Automation `CurrentIsPassword`;
4. nếu UIA xác nhận không phải password thì `Normal`;
5. nếu không đủ bằng chứng thì `Unavailable`.

Verdict được gắn với PID, TID, HWND và focus generation. Khi focus/field thay đổi, projection bị invalidate thành `Pending`; phím vật lý được pass nguyên trạng cho đến khi worker publish verdict khớp identity mới.

Worker không đọc field value/text. Password, PIN và denylisted process không transform, không learning, không capture và không hiện overlay.

## 4. Domain core

### 4.1 Semantic contracts

Contract dùng chung nằm ở [`crates/openvikey-core/src/types.rs`](../../crates/openvikey-core/src/types.rs):

- `InputEvent` mang sequence, logical time, modifiers và `InputContext`;
- `CompositionSnapshot` giữ revision, raw keys, rendered và NFC-normalized text;
- `EditRange` dùng Unicode grapheme và revision;
- `ReplaceRangeAction` tự chứa original, replacement, delimiter và edit ID;
- inverse undo được tạo từ chính semantic action;
- `EngineAction` chỉ gồm update composition, commit, replace range và show suggestions.

Nhờ vậy adapter OS không phải suy đoán ý nghĩa của một correction từ số lần Backspace.

### 4.2 Engine

[`engine`](../../crates/openvikey-core/src/engine) chỉ sở hữu:

- raw keystrokes;
- Telex/VNI method;
- tone-placement profile;
- rendered composition cache;
- monotonic revision.

Engine không biết lexicon, candidate, model, persistence hoặc Windows API. Backend `vi-rs` được pin theo commit và bọc sau seam `engine/backend.rs`.

Backspace hiện xóa đúng một visible Unicode grapheme. Vì `vi-rs` không có native Backspace API, backend tìm tập raw keys nhỏ nhất có replay phù hợp với visible prefix. Quyết định này được ghi ở [ADR 0012](../decisions/0012-visible-grapheme-backspace.md).

### 4.3 Candidate pipeline

Pipeline correction:

```text
CompositionSnapshot + LeftContext
                │
                ▼
      pure candidate generators
                │
                ▼
      normalize / dedupe / rank
                │
                ▼
       exact correction memory
                │
                ▼
       intervention planner
      None / Suggest / Replace
                │
                ▼
 semantic action + feedback transaction
```

Năm nguồn candidate hiện tại:

| Source | Vai trò | Quyền tối đa |
|---|---|---|
| `TelexFix` | Sửa cấu trúc Telex/VNI sai vị trí | Có thể Replace khi guard an toàn đạt |
| `Fuzzy` | Đảo chữ, dư/thiếu phím và typo gần | Suggest ở cold start; Auto sau exact evidence |
| `Abbreviation` | Viết tắt một từ hoặc nhiều từ | Một từ có thể Auto sau evidence; cụm Suggest-only |
| `Diacritics` | Khôi phục dấu cho token không dấu | Suggest-only |
| `Personal` | Cặp sửa do người dùng tự dạy | Probation rồi Suggest-only trong v2 |

Generator không được đọc model. [`rank.rs`](../../crates/openvikey-core/src/rank.rs) mới được dùng bounded personal signals; [`intervention.rs`](../../crates/openvikey-core/src/intervention.rs) là nơi duy nhất được chọn None/Suggest/Replace.

### 4.4 Learning model v2

[`AdaptiveModel`](../../crates/openvikey-core/src/model.rs) payload v2 gồm:

- exact `CorrectionMemory`;
- unigram/bigram `UserLanguageModel`;
- observe-only `GeneralizedErrorModel`;
- model limits và learning-config hash.

Stable learning identity gồm:

```text
input_method
+ source
+ original_nfc
+ candidate_nfc
+ left_token_nfc?
+ source_rule_id
```

Learning model sử dụng positive/negative evidence mass có decay theo injected logical time. Passive settlement là tín hiệu yếu có trần; suggestion không được chọn chỉ tạo impression, không tự động bị coi là reject.

Safety rules nằm ngoài model. Model không thể học để vượt:

- `allow_transform=false`;
- `allow_learning=false` đối với mutation;
- sensitive/denylist policy;
- minimum correction graphemes;
- source action cap;
- invalid/stale edit range;
- yêu cầu semantic undo.

`GeneralizedErrorModel` hiện chỉ thống kê operation class phục vụ quan sát/offline research; nó không thay đổi candidate order hoặc typed text.

## 5. Session reducer

[`crates/openvikey-session/src/session.rs`](../../crates/openvikey-session/src/session.rs) là stateful application layer dùng chung cho lab và Windows.

Session sở hữu:

- engine;
- lexicon và generator configuration;
- learning session/model;
- committed `DocumentBuffer`;
- left context;
- capture journal;
- candidate/intervention state gần nhất;
- Auto undo và revert guard;
- delete→retype correction miner;
- composition rewind miner;
- lazy reopen anchor;
- learning notice cho local UI.

Một input event đi qua session theo thứ tự chính:

1. Validate/capture event theo context.
2. Invalidate caret/reopen/revert state nếu cần.
3. Chụp engine snapshot trước event.
4. Cho engine xử lý input.
5. Nếu commit boundary, chạy candidate pipeline trên snapshot vừa commit.
6. Tạo `SessionObservation` gồm engine action, candidate và intervention action.
7. Cập nhật committed document, left context và learning state.
8. Settlement các Auto transaction đủ điều kiện.
9. Phát structured learning notice nếu có mutation thật.

Tên `LabSession` là legacy vocabulary: reducer này hiện phục vụ cả lab và Windows product, không còn chỉ là lab harness.

## 6. Capture, replay và persistence

### 6.1 Capture journal

Capture v2 nằm ở [`crates/openvikey-session/src/capture.rs`](../../crates/openvikey-session/src/capture.rs), chứa các record như:

- input command;
- accept/reject/undo;
- evaluated candidate identities;
- intervention applied/reverted/settled;
- confirmed correction;
- settled language commit;
- physical forget marker.

Capture giữ monotonic `next_seq`, `next_edit_id`, model SHA-256 và learning-config hash. Replay fail closed nếu không có đúng config đã dùng để quyết định lịch sử.

### 6.2 Hai storage policy

Core/lab hỗ trợ encrypted envelope:

- XChaCha20-Poly1305 cho payload;
- DEK wrapping;
- Argon2id passphrase provider;
- backup/recovery cho file store.

Windows preview cố ý dùng plaintext:

- `model.ovkdev.json`;
- `capture.ovkdev.json`;
- sibling `.bak` và `.tmp` để recovery;
- model hash liên kết pair;
- một debounced worker snapshot và lưu coherent pair.

Plaintext Windows storage là temporary development policy theo [ADR 0008](../decisions/0008-open-development-persistence.md), không phải production security architecture.

## 7. Lab và evidence architecture

[`openvikey-lab`](../../crates/openvikey-lab) là adapter quan sát/verification, không phải system IME. CLI hiện có:

- `type`: stream input qua engine/session thành JSONL;
- `script run`: deterministic learning replay;
- `model dump`: đọc encrypted model sau authentication;
- `provenance-verify`: kiểm provenance/license/hash;
- `corpus verify/build-lexicon/evaluate`;
- `perf`: xuất performance report;
- `session`: encrypted interactive capture REPL.

Việc lab tái sử dụng `openvikey-session` giúp giảm nguy cơ lab và Windows có hai learning reducer khác nhau.

## 8. TSF optional boundary

[`openvikey-win-tsf`](../../crates/openvikey-win-tsf) từng cung cấp read-only InputScope và surrounding-left-token snapshot qua named pipe. Sau [ADR 0010](../decisions/0010-windows-standalone-default.md):

- TSF không còn nằm trên critical product path;
- default executable không start TSF bridge;
- standard package không chứa DLL hoặc registration helper;
- TSF không sở hữu model, session, settings hoặc learning riêng;
- component chỉ được kích hoạt bằng explicit developer/compatibility action.

`openvikey-win-context` vẫn có giá trị như pure context contract và identity-validation layer, ngay cả khi standalone detector không dùng TSF transport.

## 9. Điểm mạnh kiến trúc

### 9.1 Domain/OS separation rõ

`openvikey-core` không chứa Windows API và dùng workspace policy `unsafe_code = "forbid"`. `unsafe` chỉ được mở tại các Windows adapter cần FFI.

### 9.2 Một authority duy nhất cho intervention

Generator sinh dữ liệu, rank đánh giá, model trả read-only signal và chỉ planner quyết định hành vi. Điều này tránh việc mỗi adapter tự nâng Suggest thành Auto.

### 9.3 Semantic edit và rollback được thiết kế từ đầu

Revision, grapheme range, edit identity và inverse action giúp correction/undo có contract rõ trên nhiều adapter.

### 9.4 Deterministic/versioned learning

Model, capture, config, corpus và provenance đều có version/hash. Migration không được âm thầm reinterpret old payload thành quyền can thiệp cao hơn.

### 9.5 Privacy boundary có thể kiểm chứng

Không thấy network client, telemetry hoặc automatic upload path trong workspace. Sensitive context và `allow_learning=false` được kiểm bằng zero-mutation tests.

### 9.6 Hook path có backpressure policy rõ

Hot path dùng non-blocking reads và `try_lock`; injection có self-marker; failure có checkpoint restore. Những lựa chọn này phù hợp với low-level hook callback.

## 10. Rủi ro và architecture debt

### 10.1 Session reducer đang quá lớn

`openvikey-session/src/session.rs` khoảng 1.773 dòng và đồng thời quản lý:

- engine/document state;
- correction orchestration;
- learning transactions;
- implicit miners;
- capture/replay concerns;
- reopen/undo policy;
- UI learning notices.

Đây là điểm dễ phát sinh lỗi tương tác nhất. Nếu tiếp tục mở rộng, nên tách theo state-machine responsibility thay vì chia file cơ học.

### 10.2 Windows shell đang monolithic

Các hotspot hiện tại:

- `control.rs`: khoảng 3.684 dòng;
- `host.rs`: khoảng 1.409 dòng;
- `overlay.rs`: khoảng 737 dòng;
- `tray.rs`: khoảng 558 dòng.

Global `OnceLock`, native Win32 UI imperative và nhiều public mutable field trong `TypingHost` chấp nhận được cho preview một process, nhưng làm tăng chi phí test và thay đổi product shell.

### 10.3 Khoảng lệch giữa custom-field opt-in và implementation

Standalone spec định nghĩa `NormalOptIn`: custom field unsupported có thể được user cho phép transform theo per-app policy.

Hiện tại `policy::decide` trả `Pass` ngay khi context là `Pending`, `Sensitive` hoặc `Unavailable`, trước khi xét `AppTransformPolicy::Allow`. Vì vậy `Allow` chưa biến một UIA-unavailable custom field thành explicit safe opt-in; trong key policy nó gần như tương đương `Default`, ngoại trừ `Block` vẫn có hiệu lực.

Đây là khoảng lệch contract cần được làm rõ bằng spec decision và regression test trước khi sửa.

### 10.4 Hai storage policy chưa hội tụ

Encrypted core/lab store và plaintext Windows preview là quyết định có chủ ý, nhưng public release cần một non-interactive protected-storage policy riêng. Không nên coi implementation encryption trong core là bằng chứng Windows product đã bảo vệ dữ liệu.

### 10.5 TSF vẫn tạo maintenance surface

TSF không nằm trong package mặc định nhưng vẫn ở workspace và CI. Điều này giữ research evidence, đồng thời tăng FFI/test/Windows-registration maintenance cost. Mọi thay đổi shared context contract cần tránh vô tình đưa TSF trở lại default runtime.

### 10.6 Ubiquitous language chưa đồng bộ hoàn toàn

Tên `LabSession` và một số comment còn phản ánh giai đoạn GĐ2a/GĐ2b cũ. Behavior hiện đúng hơn tên gọi. Việc đổi tên chỉ nên thực hiện như refactor riêng có compatibility plan, không gộp vào feature work.

## 11. Production-readiness boundary

Kiến trúc hiện đủ tốt cho development preview nhưng chưa production-ready vì các gate sau còn mở:

- corpus chất lượng G3 chưa đạt quy mô/license yêu cầu;
- Windows model/capture còn plaintext;
- installer và Authenticode signing chưa hoàn tất;
- manual app/password compatibility matrix chưa được đóng toàn bộ;
- clean-machine release evidence còn là manual gate;
- macOS chưa có adapter implementation.

Không nên dùng automated unit-test status để suy ra các gate runtime/release trên đã hoàn tất.

## 12. Bằng chứng xác minh ngày 2026-08-23

Đã chạy:

```powershell
cargo test -p openvikey-core -p openvikey-session
cargo check -p openvikey-win --all-targets
git diff --check
```

Kết quả:

- 220 core/session tests pass;
- tất cả target của `openvikey-win` compile/check thành công;
- `git diff --check` không báo whitespace error, chỉ có cảnh báo LF/CRLF trên hai file UI đang sửa;
- không chạy full workspace test, Clippy, cargo-deny hoặc manual Windows compatibility matrix trong lượt khảo sát này.

Trạng thái checkout được bảo toàn:

```text
brainstorm/next...origin/brainstorm/next [ahead 1]
 M crates/openvikey-win/src/control.rs
 M crates/openvikey-win/tests/ui_settings.rs
```

Hai thay đổi UI trên đã tồn tại trước khảo sát và không bị chỉnh sửa bởi lượt ghi nhận kiến trúc này.

## 13. Hướng ưu tiên đề xuất

Nếu tiếp tục phát triển, thứ tự kiến trúc hợp lý là:

1. Chốt semantics của `AppTransformPolicy::Allow` đối với `Unavailable` custom field và thêm regression tests.
2. Giữ nguyên single-planner invariant khi mở rộng learning/product policy.
3. Giảm trách nhiệm của session reducer theo các state machine thật sự độc lập.
4. Tách Windows UI command/state khỏi native rendering khi có feature mới, tránh tiếp tục dồn vào `control.rs`.
5. Chọn protected non-interactive Windows persistence trước public release.
6. Đóng manual compatibility, corpus, installer và signing gates độc lập với unit tests.

## 14. Tài liệu chi phối

- [Core design](../superpowers/specs/2026-08-17-openvikey-design.md)
- [Windows standalone design](../superpowers/specs/2026-08-19-openvikey-windows-standalone-design.md)
- [Learning model v2 design](../superpowers/specs/2026-08-20-openvikey-learning-model-v2-design.md)
- [ADR 0010 — Windows standalone default](../decisions/0010-windows-standalone-default.md)
- [ADR 0011 — Learning model v2 seams](../decisions/0011-learning-model-v2-seams.md)
- [ADR 0012 — Visible-grapheme Backspace](../decisions/0012-visible-grapheme-backspace.md)
