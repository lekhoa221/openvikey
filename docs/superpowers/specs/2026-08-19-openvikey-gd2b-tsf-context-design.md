# OpenViKey GĐ2b — TSF read-only context (thiết kế)

- **Ngày:** 2026-08-19
- **Trạng thái:** v0.2 — contract khóa; Rust COM lifecycle xanh; OS profile activation còn chờ elevated smoke
- **Governing master:** [`2026-08-18-openvikey-gd2-windows-host-design.md`](./2026-08-18-openvikey-gd2-windows-host-design.md)
- **Khảo sát:** [`2026-08-19-openvikey-gd2b-tsf-context-survey.md`](./2026-08-19-openvikey-gd2b-tsf-context-survey.md)
- **Baseline:** `c8233e7`

### Phase 0 evidence (2026-08-19)

- `windows-rs` class factory tạo được `ITfTextInputProcessorEx`; `ActivateEx`/`Deactivate` chạy với `CLSID_TF_ThreadMgr` thật; object count trở về zero và `DllCanUnloadNow == S_OK`.
- x64 `cdylib` và registration helper build được.
- Per-user COM subtree rollback sạch.
- Cả `ITfInputProcessorProfiles::Register` và API Vista+ `ITfInputProcessorProfileMgr::RegisterProfile` trả `E_FAIL` trong process hiện tại vì phiên Codex không elevated. Helper đã kết thúc bằng unregister; không còn CLSID key dưới HKCU.
- Vì lifecycle Rust đã xanh, **chọn Rust** cho TSF DLL. Registration/Notepad activation vẫn là manual gate bắt buộc và chưa được coi là đóng.

## 0. Mục tiêu và success criteria

GĐ2b bổ sung context TSF chỉ đọc cho Windows host GĐ2a. Hook vẫn là đường nhập chính.

Pha này xong khi:

1. explicit password/PIN InputScope làm hook `Pass` trước khi ăn phím;
2. sensitive context không đọc surrounding text và không đổi model/capture;
3. normal context có thể cung cấp previous token cho `LabSession`;
4. snapshot stale hoặc sai foreground không vượt qua focus boundary;
5. app không hỗ trợ TSF vẫn giữ behavior GĐ2a;
6. hook callback không làm COM, IPC, allocation không giới hạn hoặc blocking lock;
7. Data Inspector development-only đọc được plaintext `.ovkdev.json` nhưng không ghi;
8. registration/unregistration và process shutdown không leak/hang trong matrix đã công bố.

`IS_PASSWORD` là tín hiệu ứng dụng cung cấp, không phải security boundary. GĐ2b không tuyên bố bảo vệ mọi password field nếu ứng dụng không expose TSF/InputScope.

## 1. Phạm vi

### 1.1 Trong phạm vi

- COM TSF text service read-only, x64 trước; x86 theo gate §11.
- Focus/context lifecycle và read edit session.
- InputScope classification.
- Bounded left-token extraction.
- Versioned local bridge và host cache.
- Host policy/session integration.
- Read-only Data Inspector.
- Automated tests và manual compatibility matrix.

### 1.2 Ngoài phạm vi

- TSF key event sink, composition hoặc `ITfRange::SetText` — GĐ2c.
- Settings/control UI hoàn chỉnh — GĐ2d.
- Password/passphrase hoặc production encryption.
- Installer, signing, icon, cloud sync, telemetry, macro.
- Đọc full paragraph, selection text hoặc lưu raw surrounding-text snapshot.

## 2. Module và dependency direction

```text
openvikey-win-tsf (cdylib, in app process)
  TSF/COM adapter ──► openvikey-win-context ──► bridge client
                                                    │ versioned frame
                                                    ▼
openvikey-win (host process)
  bridge server ──► openvikey-win-context/cache ──► policy/host ──► openvikey-session
                                                    └─► openvikey-core

openvikey-data-inspector (development executable)
  read-only inspection API ──► existing model/capture schema
```

Rules:

- TSF crate/shim không phụ thuộc core/session.
- `openvikey-win-context` là crate thuần, không `unsafe`, COM, hook hay persistence; cả DLL và host dùng chung contract này.
- Core không biết COM, PID, HWND, pipe hay InputScope numeric code.
- Session chỉ nhận normalized previous token và boolean input policy.
- Data Inspector không sở hữu saver, không gọi write/repair và không chạy trong TSF DLL.
- GPL reference repositories chỉ dùng để học ý tưởng; không chép/link code.

## 3. Public seams đã khóa cho TDD

### 3.1 Context classification

```rust
pub enum ContextState {
    Unsupported,
    Pending,
    Normal,
    Sensitive,
    Unavailable,
}

pub fn classify_input_scopes(scopes: &[i32]) -> ContextState;
```

`IS_PASSWORD` (31), `IS_NUMERIC_PIN` (64), `IS_ALPHANUMERIC_PIN` (65) và `IS_ALPHANUMERIC_PIN_SET` (66) là `Sensitive`. Sensitive thắng mọi scope khác. Empty/missing property trong một context đọc được là `Normal`; lỗi/lock/disconnect là `Unavailable`, không được giả thành normal.

### 3.2 Protocol/cache

```rust
pub struct ForegroundIdentity {
    pub pid: u32,
    pub tid: u32,
    pub hwnd: Option<u64>,
    pub generation: u64,
}

pub struct ContextSnapshot {
    pub protocol_version: u16,
    pub source_pid: u32,
    pub source_tid: u32,
    pub instance_id: u64,
    pub context_seq: u64,
    pub observed_seq: u64,
    pub hwnd: Option<u64>,
    pub state: ContextState,
    pub left_token_nfc: Option<String>,
}

pub enum ContextProjection {
    Unsupported,
    Pending,
    Normal { left_token_nfc: Option<String> },
    Sensitive,
    Unavailable,
}
```

Cache validator từ chối version lạ, zero PID/TID, token quá bound, sequence lùi trong cùng instance, và snapshot không match foreground. HWND là corroborating signal, không phải identity duy nhất vì `ITfContextView::GetWnd` có thể trả null.

### 3.3 Policy

`HostState` nhận một projection nhỏ:

- `Sensitive` hoặc `Pending` → physical keys `Pass` sau own-`SendInput` rule và trước sending/key classification;
- `Normal` → behavior GĐ2a;
- `Unsupported` → behavior GĐ2a;
- `Unavailable` cho một active TSF context → `Pass` đến khi có explicit transition/normal snapshot;
- failure khi đọc cache trên hook → `Pass` một key, không block.

Own `SendInput` vẫn luôn `Pass` đầu tiên để không phá injection cleanup.

### 3.4 Session rebase

```rust
pub fn rebase_left_context(&mut self, token_nfc: Option<String>);
```

API normalize NFC, chỉ giữ một token bounded, invalidate learning anchors phụ thuộc caret, và không tự ghi capture. Focus/caret/context change gọi clear/rebase rõ ràng. Document buffer nội bộ tiếp tục là nguồn context khi không có external token.

### 3.5 TSF adapter

COM boundary được bọc bởi một seam trả value owned:

```rust
pub struct ReadContextResult {
    pub state: ContextState,
    pub left_token_nfc: Option<String>,
    pub hwnd: Option<u64>,
}
```

Pure tests dùng result literals; không mock các object nội bộ. Windows integration tests kiểm tra adapter thật.

## 4. TSF lifecycle

TSF service là in-process COM server và implement tối thiểu:

- `IClassFactory`;
- `ITfTextInputProcessor` + `ITfTextInputProcessorEx`;
- `ITfThreadMgrEventSink` để theo focused document/context;
- context/text edit sink nhỏ nhất cần để refresh snapshot.

`ActivateEx` giữ thread manager/client ID, advise sinks và publish `Pending`. `OnSetFocus`/push/pop context đổi `context_seq`, yêu cầu một read edit session. `Deactivate` unadvise theo thứ tự ngược, cancel publisher, thả references và publish disconnect best-effort. `DllCanUnloadNow` chỉ trả unloadable khi object và worker count bằng zero.

Registration là lệnh explicit, idempotent và symmetric:

- COM in-proc server registry;
- `ITfInputProcessorProfiles::Register` + language profile;
- category cần thiết tối thiểu;
- unregister profile/category/COM entries;
- host startup không tự sửa registry.

Phase 0 đã chọn Rust `windows 0.62.2` sau direct lifecycle test. C++ shim chỉ được mở lại nếu elevated OS activation cho thấy lỗi ownership/unload không tái hiện ở direct test.

## 5. Read edit session

Mỗi refresh:

1. lấy top context đang focus;
2. request `TF_ES_READ | TF_ES_ASYNCDONTCARE`;
3. trong `DoEditSession`, lấy selection range;
4. đọc `GUID_PROP_INPUTSCOPE` qua app property;
5. classify scope;
6. nếu sensitive, trả ngay và không gọi range `GetText`;
7. nếu normal, clone/collapse caret, shift start tối đa 128 UTF-16 code units, đọc text;
8. tách token cuối theo Unicode word/boundary policy dùng chung nhỏ, normalize NFC, giới hạn 128 UTF-8 bytes;
9. lấy optional active-view HWND;
10. enqueue latest snapshot.

Không retry đồng bộ trong callback khi `TF_E_LOCKED`, disconnected hoặc synchronous request thất bại. Publish `Unavailable`, chờ event kế tiếp.

## 6. Bridge

### 6.1 Transport

Thiết kế mặc định là per-user named pipe:

- host là server, TSF instances là clients;
- pipe name có protocol major và user-local scope;
- server ACL chỉ cho current user và SYSTEM;
- length-prefixed frame với hard size limit 4 KiB;
- payload không chứa raw surrounding text ngoài final left token;
- malformed/unknown frames bị drop và rate-limit log.

TSF callback chỉ `try_send` vào bounded latest-value queue. Một publisher worker làm connect/write/reconnect. Queue đầy thay snapshot cũ bằng snapshot mới. Không giữ event history.

Shared memory chỉ là fallback nếu Phase 0 chứng minh named-pipe worker làm DLL không unload sạch.

### 6.2 Shutdown

- Worker có cancellation token và finite connect/write timeout.
- `Deactivate` không chờ I/O vô hạn.
- Host restart làm clients reconnect.
- Client disconnect làm host invalidate instance snapshots.
- Host shutdown đóng server trước khi teardown hook/session.

## 7. Foreground/cache semantics

`FocusCache` bổ sung PID/TID lấy ngoài hook callback. Mỗi foreground event vẫn tăng generation.

`ContextCache::project(foreground)`:

- exact PID/TID + current instance/context sequence → projection snapshot;
- HWND present ở cả hai phía nhưng khác → reject;
- source active nhưng chưa có read result → `Pending`;
- disconnected/stale identity → không reuse token;
- không có bridge/source cho app → `Unsupported`.

Không dùng wall-clock TTL làm correctness key. TTL chỉ hiển thị bridge health trong diagnostics/Data Inspector.

Khi projection chuyển `Normal → Sensitive/Pending/Unavailable`, host clear composition/candidates và external left context đúng một lần, nhưng không inject backspace/text. Khi trở lại `Normal`, rebase token trước physical key tiếp theo nếu snapshot match.

## 8. Sensitive-field contract

Thứ tự policy:

1. own `SendInput` → `Pass`;
2. context `Sensitive/Pending/Unavailable-active` → physical input `Pass`;
3. GĐ2a sending/key-up/denylist/terminal/mode/hotkey rules.

Sensitive snapshot luôn có `left_token_nfc=None`. TSF adapter phải classify scope trước `GetText`. Host không gọi session reducer, saver notify hoặc overlay candidate update cho physical keys được pass bởi sensitive policy.

Executable denylist GĐ2a vẫn tồn tại làm fallback. Secure desktop/credential processes không thuộc manual injection scope.

## 9. Open development persistence và Data Inspector

Không thêm password/passphrase. `model.ovkdev.json`, `capture.ovkdev.json` và recovery/provenance behavior hiện có giữ nguyên.

Data Inspector v1:

- mở default path hoặc CLI path explicit;
- parse bằng public inspection/load API dùng cùng schema validator với host;
- hiển thị summary và filtered rows cho original, candidate, source, left token, count/evidence/time;
- manual refresh;
- warning rõ cho missing/invalid/provenance mismatch;
- không có write/delete/repair/import/export/settings.

CLI/TUI read-only chấp nhận được cho GĐ2b. Product window thuộc GĐ2d.

## 10. TDD slices

Mỗi slice làm đỏ → xanh riêng:

1. scope classification;
2. bounded previous-token extraction;
3. protocol validation/order;
4. cache projection/focus mismatch;
5. policy pass-through;
6. session rebase/clear;
7. host transition + zero persistence mutation;
8. pipe codec/server/client lifecycle;
9. TSF COM activation and read adapter;
10. Data Inspector read-only outputs.

Tests quan sát qua public seams §3, không assert private call order.

## 11. Phase 0 gates

### Gate A — Rust COM

- build x64 `cdylib`;
- register/unregister 20 vòng idempotent;
- Notepad gọi activate/focus/deactivate;
- sink cookies và references về zero; process exit/unload không hang;
- CI compile được mà không ảnh hưởng non-Windows core/session.

Fail gate → C++ shim. Không mở rộng Rust spike bằng abstraction mới để né lỗi lifecycle.

### Gate B — read contract

- normal Notepad trả previous token;
- sensitive scope không gọi text read (instrumented counter = 0);
- unavailable/windowless context không crash;
- no write/edit/composition API.

### Gate C — bridge

- callback enqueue bounded và không block;
- host absent/restart, app close, focus storm không deadlock;
- stale instance/context frame bị reject;
- median/p95 diagnostics được ghi nhưng không đặt latency promise trước phép đo.

### Gate D — app matrix

Win32, WPF, WinUI, Chrome, Edge password fields; Notepad/Cursor/contenteditable normal fields. Mỗi case ghi TSF active, scopes, policy, model/capture before-after.

### Gate E — architecture

x64 là minimum development gate. Trước khi gọi GĐ2b production-complete, phải build/register/smoke x86 DLL hoặc ghi rõ x86 unsupported trong release contract.

## 12. Verification và done boundary

Automated:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo deny check
```

Manual evidence không được thay bằng unit test giả lập: COM registration/activation, password matrix, host restart/focus storm và architecture coverage.

GĐ2b dừng ở read-only context + Data Inspector. Không bắt đầu key-event/composition GĐ2c hoặc settings mutation GĐ2d trong cùng plan.
