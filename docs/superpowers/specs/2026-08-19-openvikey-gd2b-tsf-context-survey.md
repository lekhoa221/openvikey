# GĐ2b TSF read-only context — khảo sát trước thiết kế

- **Ngày khảo sát:** 2026-08-19
- **Trạng thái:** khảo sát hiện trạng và contract; chưa phải design spec hay implementation plan
- **Baseline:** `main` tại `a529faa` (GĐ2a đã merge local)
- **Phạm vi:** InputScope, token bên trái caret, bridge TSF → host, chính sách hook, dữ liệu phát triển và Data Inspector

## 1. Kết luận ngắn

GĐ2b nên tiếp tục theo kiến trúc hybrid đã chọn:

- `openvikey-win` và hook GĐ2a vẫn là đường gõ chính;
- một COM in-process TSF text service chỉ **đọc** InputScope và một đoạn text nhỏ bên trái caret;
- TSF callback không gọi keyboard hook, core/session hay persistence trực tiếp;
- snapshot được đẩy ra khỏi tiến trình ứng dụng qua bridge bất đồng bộ; hook chỉ đọc cache local bằng thao tác không chặn;
- field nhạy cảm phải `Pass` phím vật lý **trước khi** host ăn phím, không chỉ đặt `InputContext.allow_transform=false` sau đó;
- trong field nhạy cảm, không đọc surrounding text, không transform, không learning, không capture;
- model/capture development vẫn là plaintext `.ovkdev.json`; không thêm password/passphrase ở GĐ2b;
- Data Inspector cuối GĐ2b chỉ đọc dữ liệu normal-field, không trở thành settings UI hoàn chỉnh.

GĐ2b chưa nên đi thẳng vào implementation plan. Cần design spec và một Phase 0 spike để khóa: Rust COM hay C++ shim, lifecycle register/unregister, transport, và hành vi thực tế của password field trên các app đại diện.

## 2. Điều GĐ2b làm và không làm

### Làm

1. Phân loại context thành `Unsupported`, `Pending`, `Normal`, `Sensitive`, hoặc `Unavailable`.
2. Nhận diện tối thiểu `IS_PASSWORD` và `IS_PIN` là `Sensitive`.
3. Đọc tối đa một cửa sổ UTF-16 giới hạn bên trái caret, rồi chỉ giữ token trái đã chuẩn hóa.
4. Ghép snapshot đúng foreground process/thread/context; bỏ snapshot stale hoặc sai identity.
5. Đưa kết quả vào policy trước quyết định `Pass`/`Eat` và vào session context khi thực sự xử lý phím.
6. Rebase `LeftContext` của session từ token TSF mà không ghi raw surrounding-text snapshot xuống đĩa.
7. Cung cấp Data Inspector development-only, read-only cho model/capture plaintext.

### Không làm

- TSF không nhận key và không commit composition; đó là GĐ2c.
- Không thay hook bằng TSF.
- Không thêm passphrase, mã hóa hoặc password UI cho development persistence.
- Không hứa phát hiện mọi password field nếu ứng dụng không công bố InputScope hoặc tắt text services.
- Không làm settings shell kiểu UniKey/VNIKey; phần đó là GĐ2d.
- Không thêm installer, signing, icon sản phẩm hoặc autostart ở pha này.

## 3. Hiện trạng code GĐ2a

### 3.1 Hook policy đang quyết định quá sớm cho InputScope hiện tại

`HostRuntime` chỉ giữ host, focus, sending/mode/suggestion flags và persistence callback (`crates/openvikey-win/src/host.rs:25`). `lock_free_host_state` chỉ đọc executable foreground từ `FocusCache` (`host.rs:106`). `HostState` chưa có context validity hay sensitive flag (`policy.rs:19`).

`dispatch_locked_key` gọi `decide` trước khi lấy `TypingHost` mutex (`host.rs:143`). Đây là seam đúng về latency nhưng có hệ quả: sensitive state phải có trong cache mà `decide` đọc được. Nếu chỉ đổi `InputContext` trong `apply_typed`, hook đã có thể ăn phím rồi.

**Khóa thiết kế:** `HostState` cần một policy projection nhỏ, copy/atomic-friendly. Khi projection là `Sensitive` hoặc `Pending` cho đúng foreground identity, `decide` phải `Pass`. Không gọi COM, pipe, serialization hoặc blocking lock từ `WH_KEYBOARD_LL`.

### 3.2 Focus cache chưa đủ identity để ghép TSF snapshot

`FocusCache` hiện giữ `hwnd`, `exe`, inject profile và `generation` (`focus.rs:17`). WinEvent foreground làm tăng generation (`focus.rs:43`); hook dùng `try_read` (`focus.rs:65`). Cache chưa giữ PID/TID, trong khi TSF DLL chạy trong process/thread của ứng dụng và `ITfContextView::GetWnd` có thể trả HWND null cho windowless control.

**Khóa thiết kế:** mở rộng foreground identity tối thiểu bằng PID và thread ID. Không dùng HWND đơn lẻ làm khóa. Snapshot TSF phải chứa protocol version, source PID/TID, optional HWND, context generation/sequence, context state và optional left token.

### 3.3 Session chưa có API nhận external left context

`TypingHost::apply_typed` hiện luôn tạo `InputContext { allow_transform: true, allow_learning }` (`host.rs:543`). `allow_learning_for_foreground` chỉ dựa vào mode và executable policy (`host.rs:618`).

`LabSession::clear_document_context` xóa document buffer và `LeftContext` khi đổi foreground (`crates/openvikey-session/src/session.rs:479`). `sync_left_context` luôn lấy token từ document buffer nội bộ (`session.rs:950`). Chưa có API rebase token bên trái từ TSF.

**Khóa thiết kế:** thêm API session hẹp kiểu `rebase_left_context(Option<String>)`, không nhét raw surrounding text vào `InputContext`. External token chỉ có hiệu lực cho đúng context generation và bị xóa khi focus/caret/context đổi.

### 3.4 Persistence hiện đã phù hợp cho development

Session save snapshot chứa adaptive model, capture records, cursors và timestamp (`session.rs:465`). GĐ2b không đổi định dạng open development persistence chỉ để phục vụ TSF.

Trong normal field, rule đã học có thể lưu `left_token_nfc`; đây là dữ liệu học có chủ đích và cần nhìn thấy trong Data Inspector. Trong sensitive field, đường xử lý phải dừng trước session, nên không được sinh capture/model delta nào.

### 3.5 Build surface còn thiếu TSF/COM features

Workspace đang dùng `windows = 0.62.2` nhưng chưa bật `Win32_UI_TextServices`, `Win32_System_Com`, registry và các feature phụ cần cho registration (`Cargo.toml:34`). `openvikey-win` được phép chứa unsafe FFI; core/session vẫn phải `unsafe_code = forbid`.

CI hiện chỉ chạy một Windows target qua fmt, clippy, test và cargo-deny. GĐ2b phải bổ sung build matrix nếu ship cả x64 và x86 TSF DLL.

## 4. Contract TSF chính thức

### 4.1 Lifecycle và registration

Text service là COM in-process server. Ngoài COM registration, nó phải đăng ký với TSF, language profile và category. TSF tạo service cho từng thread và gọi `Activate`/`ActivateEx`; khi `Deactivate` trả về, service phải unadvise sinks và thả reference đến thread manager.

Hệ quả:

- DLL cần `DllGetClassObject`, `DllCanUnloadNow`, class factory và symmetric register/unregister;
- sink cookies, context references và worker/publisher phải đóng sạch trước khi DLL unload;
- registration phải idempotent và có lệnh chẩn đoán; không giấu side effect trong lần chạy host bình thường;
- x64 DLL không được nạp vào app x86, nên coverage app 32-bit cần artifact/registration x86 riêng.

### 4.2 Đọc InputScope và surrounding text

Text service lấy focused document manager từ `ITfThreadMgr::GetFocus`, rồi lấy top edit context. Đọc document text/properties phải diễn ra trong `ITfContext::RequestEditSession` với read access. Trong edit session:

1. lấy selection/caret range;
2. đọc app property `GUID_PROP_INPUTSCOPE` và query `ITfInputScope`;
3. phân loại scope;
4. nếu sensitive: publish `Sensitive` ngay và **không đọc text**;
5. nếu normal: clone/collapse range, shift start lùi trong một bound nhỏ, gọi `ITfRange::GetText`, rồi tách token trái;
6. lấy optional HWND từ active context view; chấp nhận HWND null.

Read request nên dùng async-capable read session; synchronous request không phải mặc định vì TSF có thể trả `TF_E_SYNCHRONOUS` hoặc `TF_E_LOCKED` tùy callback hiện tại.

### 4.3 InputScope không phải security boundary

Microsoft ghi rõ `IS_PASSWORD` chỉ mô tả loại input, không tự bảo vệ password; tài liệu còn khuyên password field tắt text services. Vì vậy có ba trường hợp phải phân biệt:

- app công bố `IS_PASSWORD`/`IS_PIN`: OpenViKey pass-through, zero read/capture;
- TSF context tồn tại nhưng scope đang pending hoặc đọc lỗi: tạm pass-through cho context đó;
- app không hỗ trợ/publish TSF context: giữ fallback denylist GĐ2a, đồng thời công bố đây là giới hạn chứ không tuyên bố bảo vệ tuyệt đối.

Chrome/Edge HTML password, Win32 edit, WPF và WinUI cần manual matrix. Kết quả của matrix quyết định wording sản phẩm và có cần thêm signal Windows hợp lệ khác hay không.

## 5. Kiến trúc đề xuất

```text
App process/thread
  TSF DLL
    focus/context sinks
      read edit session
        InputScope first
        bounded left token only when normal
          latest-value queue
            per-user named pipe writer
                         │
                         ▼
openvikey-win process
  pipe server + validator (off hook thread)
    ContextCache keyed by PID/TID/context generation
                         │ try_read / atomic projection
                         ▼
  WH_KEYBOARD_LL policy ── Pass sensitive/pending
                         └─ existing session + SendInput for normal/unsupported
```

### 5.1 Transport: ưu tiên named pipe, chưa khóa trước spike

| Phương án | Ưu điểm | Rủi ro | Kết luận khảo sát |
|---|---|---|---|
| Per-user named pipe, host là server | Nhiều app process kết nối tự nhiên; host sở hữu cache; frame version hóa/test được; ACL rõ | I/O có thể block; worker lifecycle trong DLL; reconnect | **Ưu tiên** nếu callback chỉ ghi latest-value queue và worker làm I/O |
| Shared memory + seqlock | Đọc/ghi nhanh, không cần reconnect | Multi-writer phức tạp; ACL, torn data, cross-bitness, cleanup/stale owner | Chỉ chọn nếu pipe spike không đạt latency/lifecycle |
| COM/RPC trực tiếp về host | Contract typed | Activation/security/lifecycle phức tạp hơn nhu cầu snapshot | Không chọn cho GĐ2b |

Hook tuyệt đối không đọc pipe. Khi queue đầy, giữ snapshot mới nhất và bỏ snapshot cũ; context data là state, không phải event log.

### 5.2 Protocol tối thiểu

Protocol cần version hóa ngay từ đầu nhưng chỉ mang dữ liệu cần thiết:

- `version`;
- `source_pid`, `source_tid`;
- `hwnd: Option<u64>`;
- `context_seq` và `observed_seq` tăng đơn điệu trong một DLL instance;
- `state: Pending | Normal | Sensitive | Unavailable`;
- `scope_codes` đã lọc hoặc reason code nhỏ;
- `left_token_nfc: Option<String>` chỉ khi `Normal`, giới hạn byte/UTF-16 length.

Không gửi full paragraph, selection text, keystrokes, model hoặc capture records qua bridge.

### 5.3 Freshness và fail behavior

- Foreground PID/TID/context generation match: dùng snapshot.
- Source được biết là active nhưng snapshot mới đang `Pending`: pass-through để không ăn ký tự trước khi biết scope.
- `Sensitive`: pass-through; clear composition/candidates/left context một lần khi state chuyển vào sensitive.
- Snapshot sai PID/TID/generation hoặc instance đã disconnect: bỏ, không tái dùng token cũ.
- App không có TSF bridge: dùng hành vi GĐ2a (exe denylist + hook typing).
- Lỗi pipe/COM không được crash app host hoặc app đích; log chỉ metadata/reason, không log surrounding text ở mức mặc định.

Không nên dùng TTL thời gian làm điều kiện correctness duy nhất: người dùng có thể dừng gõ lâu trong cùng field. Identity + explicit context transitions là nguồn chính; timeout chỉ phục vụ health diagnostics.

### 5.4 Rust COM hay C++ shim

`windows-rs` có binding TSF và macro implement COM, nên Rust DLL là khả thi về mặt API. Tuy nhiên repo không có COM server/TSF lifecycle hiện hữu và Microsoft không cung cấp sample TSF bằng Rust. COM class factory, unload semantics, x86/x64 registration và sink ownership là phần rủi ro cao nhất.

Quyết định sau Phase 0:

- giữ Rust nếu spike chứng minh activate/deactivate, focus/context sink, read edit session, registration và unload sạch;
- dùng C++ shim nhỏ nếu Rust COM lifecycle không đạt gate nhanh; shim chỉ đọc TSF + publish protocol, không chứa engine/policy/session;
- cả hai phương án vẫn giữ MIT boundary và không chép code GPL từ VKey/OpenKey.

## 6. Phase 0 spikes bắt buộc

### Spike A — lifecycle/registration

Build x64 DLL tối thiểu, register/unregister idempotent, kích hoạt trong Notepad, ghi nhận `ActivateEx`/focus/context/`Deactivate`, rồi chứng minh DLL unload hoặc process exit sạch. Lặp lại 20 vòng register → activate → unregister để bắt cookie/reference leak.

**Gate:** chọn Rust hoặc C++ dựa trên bằng chứng, không dựa trên sở thích ngôn ngữ.

### Spike B — read-only contract

Trong read edit session, đọc InputScope trước, rồi đọc tối đa 128 UTF-16 code units bên trái caret ở normal field. Không chỉnh selection/text và không tạo composition.

**Gate:** Notepad normal field trả token hợp lý; context không hỗ trợ text trả `Unavailable` an toàn; không crash khi HWND null.

### Spike C — password matrix

Kiểm tra tối thiểu:

- Win32 password edit;
- WPF password box;
- WinUI password box;
- Chrome và Edge HTML `<input type=password>`;
- Windows credential UI chỉ để xác nhận denylist/fail-safe, không cố inject DLL vào secure desktop.

Mỗi case ghi: TSF service có active không, có InputScope không, scope gì, host `Pass` hay không, model/capture hash có đổi không.

**Gate:** explicit sensitive scope luôn zero-read/zero-capture. Những app không expose signal phải được liệt kê là limitation hoặc có signal bổ sung được chứng minh bằng API chính thức.

### Spike D — bridge latency/lifecycle

TSF callback chỉ enqueue; worker gửi snapshot đến host. Kill/restart host, đổi focus nhanh A → password → A, đóng app đích khi send đang pending, và tạo nhiều client process.

**Gate:** không deadlock/unload hang; hook không thực hiện IPC; stale snapshot không được áp dụng sang foreground mới.

### Spike E — architecture coverage

Chứng minh x64 artifact trước. Sau đó quyết định GĐ2b ship gate cho x86: hoặc build/register song song, hoặc công bố rõ x86 chưa được hỗ trợ và giữ fallback GĐ2a.

## 7. Seam thay đổi dự kiến trong design spec

Đây là file map dự kiến, chưa phải chỉ thị implement:

- crate mới `openvikey-win-tsf` (`cdylib`) hoặc `native/openvikey-win-tsf` nếu chọn C++;
- `crates/openvikey-win/src/context.rs`: protocol types, validator, cache, foreground projection;
- `crates/openvikey-win/src/context_bridge.rs`: pipe server/reconnect/health, không chạy trên hook callback;
- `crates/openvikey-win/src/focus.rs`: bổ sung PID/TID identity;
- `crates/openvikey-win/src/policy.rs`: sensitive/pending pass-through;
- `crates/openvikey-win/src/host.rs`: sync context generation, clear/rebase session, derive both transform/learning flags;
- `crates/openvikey-session/src/session.rs`: public rebase API hẹp cho external left token;
- Data Inspector executable/module development-only đọc qua versioned inspection API, không parse file tùy tiện ở UI layer;
- registration helper/script và manual smoke evidence riêng; host startup không tự ý sửa registry.

Core engine không cần biết TSF, HWND, PID, COM hay IPC.

## 8. Chiến lược test

### Pure/unit tests

- InputScope codes → context state, gồm password/PIN/multiple scopes/empty/error;
- UTF-16 bounded text → previous token NFC, gồm surrogate pair, combining marks, punctuation, CRLF;
- protocol encode/decode, size bounds, unknown version, invalid UTF-16;
- foreground identity + context sequence → accept/reject stale snapshot;
- policy: sensitive/pending pass, normal giữ behavior GĐ2a, own `SendInput` luôn pass;
- session rebase/clear: token ngoài được dùng đúng một context và không sống qua caret/focus change;
- sensitive transition không đổi model/capture snapshot.

### Adapter contract tests

COM trực tiếp khó mock ổn định. Tách adapter thành interface nhỏ kiểu `ContextReader` và test pure extraction bằng fake range/scope result; chỉ giữ vài Windows integration tests cho projection thật.

### Integration/manual gates

- password matrix ở Spike C;
- normal typing Notepad, Cursor/VS Code, Chrome/Edge contenteditable;
- focus storm và app close/reopen;
- host absent/restart;
- x64/x86 theo quyết định coverage;
- full CI hiện hữu: fmt, clippy `-D warnings`, workspace tests, cargo-deny.

## 9. Data Inspector cuối GĐ2b

Mục tiêu là cùng người phát triển nhìn vào dữ liệu và phát hiện học sai:

- mở read-only `model.ovkdev.json` và `capture.ovkdev.json` qua parser/schema hiện hữu;
- refresh thủ công, filter theo original/candidate/source/left token, xem counts/evidence/timestamp;
- hiển thị parse/schema/provenance warning mà không tự sửa file;
- không có passphrase prompt;
- không edit/delete rule, settings, autostart, import/export hoặc per-app routing — các thao tác đó thuộc GĐ2d;
- nếu sensitive-field test làm model/capture đổi, đó là lỗi release-blocking của GĐ2b.

## 10. Rủi ro và quyết định còn mở

| Rủi ro / câu hỏi | Cách đóng |
|---|---|
| Rust COM server lifecycle có đủ chắc? | Spike A; C++ shim là fallback giới hạn rõ |
| Password field không expose InputScope | Spike C; wording limitation hoặc signal chính thức bổ sung |
| Named pipe worker giữ DLL không unload | Spike D; explicit shutdown + bounded queue + connection cancellation |
| Snapshot lẫn giữa app/context | PID/TID/context sequence + focus generation, test A → sensitive → A |
| HWND null/windowless control | HWND optional; PID/TID/context identity là chính |
| 32-bit app | x86 build/register gate hoặc limitation công khai |
| Raw context vô tình đi vào disk/log | Scope-first zero-read; protocol chỉ token; log redaction; persistence tests |
| GĐ2b lấn sang 2c/2d | Không key sink/composition; Data Inspector read-only |

## 11. Readiness verdict

Khảo sát đủ để viết **GĐ2b design spec**. Chưa đủ để viết implementation plan cuối cùng vì bốn lựa chọn phải có bằng chứng từ Phase 0: implementation language, registration/unload lifecycle, bridge transport, và password compatibility matrix.

Design spec tiếp theo phải khóa success criteria sau:

1. Hook path vẫn không chặn và không làm COM/IPC.
2. Explicit password/PIN scope pass-through trước khi eat key, zero surrounding-text read và zero persistence mutation.
3. Normal TSF context có thể rebase previous token mà không lưu raw snapshot.
4. Context stale/mismatch không vượt qua focus boundary.
5. TSF failure không làm hỏng đường gõ GĐ2a ở app unsupported.
6. Data Inspector đọc được normal learning/capture plaintext và không có quyền ghi.

## 12. Nguồn chính thức

- [Microsoft Learn — Text Service Registration](https://learn.microsoft.com/en-us/windows/win32/tsf/text-service-registration)
- [Microsoft Learn — ITfTextInputProcessor](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nn-msctf-itftextinputprocessor)
- [Microsoft Learn — Thread Manager](https://learn.microsoft.com/en-us/windows/win32/tsf/thread-manager)
- [Microsoft Learn — Edit Contexts](https://learn.microsoft.com/en-us/windows/win32/tsf/edit-contexts)
- [Microsoft Learn — ITfContext::RequestEditSession](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfcontext-requesteditsession)
- [Microsoft Learn — ITfContext](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nn-msctf-itfcontext)
- [Microsoft Learn — ITfRange](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nn-msctf-itfrange)
- [Microsoft Learn — ITfInputScope::GetInputScopes](https://learn.microsoft.com/en-us/windows/win32/api/inputscope/nf-inputscope-itfinputscope-getinputscopes)
- [Microsoft Learn — InputScope enumeration](https://learn.microsoft.com/en-us/windows/win32/api/inputscope/ne-inputscope-inputscope)
- [Microsoft Learn — ITfContextView::GetWnd](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfcontextview-getwnd)
- [Microsoft Windows classic sample — TSF text service](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/Win7Samples/winui/input/tsf/textservice/uilessmode/TextService.cpp)
- [Microsoft windows-rs — COM authoring support](https://github.com/microsoft/windows-rs)
