# OpenViKey Windows standalone — khảo sát lại hướng sản phẩm

- **Ngày:** 2026-08-19
- **Trạng thái:** Survey để thay đổi roadmap; chưa phải implementation plan
- **Yêu cầu khóa của owner:** OpenViKey là **một app standalone kiểu UniKey**, không phải Windows TSF language profile

---

## 0. Kết luận

Hướng sản phẩm đúng là một `OpenViKey.exe` chạy nền với tray icon, keyboard hook, `SendInput`, learning và settings trong cùng host. Người dùng không cần chọn OpenViKey trong `Win + Space`, không cần đăng ký COM/TSF và không cần DLL được Windows nạp vào từng app.

Repo đã có phần lớn lõi của app standalone trong `openvikey-win`, nhưng roadmap hiện tại đặt TSF GĐ2b/2c trên critical path và để product UI đến GĐ2d. Thứ tự này không phù hợp với yêu cầu mới xác nhận.

Khuyến nghị:

1. dừng GĐ2c TSF-primary;
2. đưa product-host/UI lên pha tiếp theo;
3. làm TSF thành plugin compatibility **optional/experimental**, không đăng ký mặc định;
4. trước tiên khôi phục và khóa gate “host hoạt động trên máy sạch không có OpenViKey TSF profile”.

---

## 1. Product contract đã làm rõ

Người dùng phải có trải nghiệm:

1. tải hoặc build một `OpenViKey.exe`;
2. double-click để chạy, không mở console;
3. thấy icon V/E dưới system tray;
4. gõ tiếng Việt trong app đang focus bằng hook + `SendInput`;
5. learning/model chỉ hoạt động khi app đang chạy;
6. mở settings từ tray để chọn Telex/VNI, gợi ý, hotkey, autostart và xem rule đã học;
7. đóng settings không dừng host; chọn Exit mới dừng;
8. OpenViKey không xuất hiện trong `Win + Space`;
9. không đăng ký TSF profile, COM DLL hoặc sửa Windows language list trong đường cài đặt mặc định;
10. dữ liệu ở local, không backend/telemetry/upload.

TSF có thể tồn tại sau này như compatibility add-on do người dùng chủ động bật, nhưng không được là điều kiện để app gõ, học hoặc khởi động.

---

## 2. Repo hiện có gì có thể tái sử dụng

| Thành phần | File/crate | Mức sẵn sàng cho standalone |
|---|---|---|
| Keyboard hook | `openvikey-win/src/hook.rs` | Có `WH_KEYBOARD_LL`, callback đồng bộ và fail-open |
| Mouse/focus reset | `mouse.rs`, `focus.rs` | Có hook và foreground generation; cần thêm field-focus cho password |
| Text injection | `inject.rs` | Có `SendInput`, Unicode và Electron profile |
| Core typing | `openvikey-core` | Tách OS, tái sử dụng nguyên trạng |
| Session/learning | `openvikey-session` | Có capture, implicit correction, personal pair, feedback và replay |
| Coordinator | `openvikey-win/src/main.rs`, `host.rs` | Có host runtime, nhưng hiện bị context TSF chi phối |
| Tray | `tray.rs` | Có V/E, Gợi ý, Exit; icon vẫn generic |
| Suggestion overlay | `overlay.rs` | Có popup topmost, chưa neo chính xác theo caret |
| Persistence | `persist.rs` | Có coherent pair saver; hiện là plaintext development JSON |
| Data Inspector | `bin/openvikey-data-inspector.rs` | Read-only CLI, có thể làm backend cho learned-rules UI |
| TSF bridge | `openvikey-win-context`, `openvikey-win-tsf` | Không thuộc default standalone path |

Kết luận: không cần viết lại engine hoặc learning. Trọng tâm là product shell, standalone context safety, cấu hình và đóng gói.

---

## 3. Vì sao bản hiện tại không tạo cảm giác “một app đang học”

### 3.1 Host đã bị dừng

`openvikey-win.exe` mới sở hữu hook, session, learning và saver. TSF DLL chỉ đọc context. Khi chỉ có TSF profile active mà host không chạy, người dùng không thấy transform, suggestion hoặc learning.

### 3.2 Có regression phụ thuộc TSF trong working tree

Projection mới được bind theo PID/TID/HWND/focus generation. Khi không có publisher TSF, slot chưa có foreground-bound projection:

- `sample_host_state` suy ra `ContextState::Unavailable`;
- `policy::decide` trả `Pass`;
- `dispatch_locked_key` cũng trả `Pass` nếu không có projection khớp.

Do đó working tree hiện tại **không đáp ứng gate standalone**: chạy `openvikey-win.exe` mà không có TSF profile/publisher có thể không transform phím.

Đây phải là blocker đầu tiên của hướng mới.

### 3.3 Learning hiện khó quan sát

Learning code có thật, nhưng UX chưa giải thích các trạng thái:

- Candidate mới thường chỉ là `Suggest`.
- `Ctrl+.` tạo explicit accept.
- Sửa tự nhiên có thể tạo implicit/personal evidence khi detector xác nhận cùng composition/document.
- Personal pair cần lặp lại trước khi promote thành suggestion.
- Auto có gate confidence/evidence; không phải một typo là sửa tự động ngay.

Hiện chỉ có overlay text và Data Inspector CLI. Không có màn hình “đã học gì”, số evidence, trạng thái Suggest/Auto hoặc thao tác Forget rõ ràng.

### 3.4 Binary vẫn là development host

- Release build yêu cầu `--lexicon`; chưa phải double-click/no-argument binary.
- Chưa có Windows GUI subsystem; chạy trực tiếp có thể hiện console.
- Chưa single-instance.
- Chưa autostart.
- Chưa settings window.
- Chưa persist method/mode/hotkeys đầy đủ.
- Chưa product icon/version metadata/installer/signing.
- Model/capture Windows vẫn là plaintext `.ovkdev.json` theo ADR 0008.

---

## 4. Kiến trúc standalone đề xuất

```text
OpenViKey.exe — một process
│
├── UI/message thread
│   ├── tray V/E
│   ├── settings window
│   └── suggestion/learning overlay
│
├── input runtime
│   ├── WH_KEYBOARD_LL
│   ├── WH_MOUSE_LL
│   ├── focus/field event observer
│   └── SendInput injector
│
├── context guard
│   ├── process denylist
│   ├── Win32 ES_PASSWORD
│   ├── UI Automation CurrentIsPassword
│   └── optional non-sensitive TextPattern context
│
├── openvikey-session
│   ├── composition
│   ├── correction/learning
│   └── capture/replay
│
└── persistence worker
    ├── settings
    ├── personal model
    └── capture log
```

Không có DLL bắt buộc trong app khác. Không có language profile. Không có lựa chọn OpenViKey trong `Win + Space`.

---

## 5. Context và password khi không dùng TSF

TSF từng được thêm vì InputScope và surrounding text. Với standalone, cần tách hai nhu cầu:

### 5.1 Sensitive detection — bắt buộc

Dùng host-side `SensitiveFieldDetector` không đọc nội dung:

1. denylist theo executable;
2. Win32 edit style `ES_PASSWORD` khi có child HWND;
3. UI Automation `AutomationElement.CurrentIsPassword` cho WPF, WinUI và browser;
4. cache kết quả theo foreground + field-focus generation;
5. khi focus vừa đổi mà chưa phân loại xong: trạng thái `Unknown`, hook pass nguyên phím;
6. tuyệt đối không gọi TextPattern/surrounding-text sau khi field là sensitive.

Cần nghe `EVENT_OBJECT_FOCUS`, không chỉ `EVENT_SYSTEM_FOREGROUND`, vì password và normal field có thể nằm trong cùng một top-level HWND. Mouse click, Tab và focus event phải invalidate verdict trước phím kế tiếp.

UI Automation/Win32 query chạy ngoài low-level hook callback. Hook chỉ đọc snapshot lock-free đã bind identity/generation.

### 5.2 Surrounding text — optional enhancement

Standalone MVP không cần surrounding text ngoài app để gõ và học cơ bản. Session đã biết:

- composition hiện tại;
- token OpenViKey vừa commit;
- focus/caret break.

Có thể thêm UI Automation `TextPattern` cho normal field sau, theo best-effort. Không có context ngoài thì dùng `left_token=None`, không chặn typing.

Quy tắc quan trọng:

- `Sensitive`/`Unknown` → pass, không transform/learn/capture;
- `Normal` → transform;
- context text unavailable nhưng field đã xác nhận không sensitive → vẫn transform với context nội bộ;
- không biến “không có TSF” thành lý do vô hiệu hóa toàn bộ app.

---

## 6. Vai trò mới của TSF

`openvikey-win-tsf` không cần bị xóa. Nó có thể được giữ dưới một trong hai trạng thái:

1. **development diagnostic** để nghiên cứu InputScope/TSF;
2. **optional compatibility plugin** cho một số app đặc biệt, với consent và installer riêng.

Default standalone build phải:

- không chạy registration helper;
- không yêu cầu elevation/UAC;
- không thêm Windows language profile;
- không start named-pipe context bridge nếu optional plugin bị tắt;
- pass integration test trên user profile chưa từng đăng ký OpenViKey TSF.

GĐ2c “TSF primary per app” bị loại khỏi critical path và chỉ được xem lại nếu có app thực tế mà hook path không hỗ trợ được.

---

## 7. Learning UX cần làm rõ

Standalone app không nên hứa “một lỗi là tự sửa vĩnh viễn”. UI phải cho thấy vòng đời rule:

```text
Observed → Suggest → Auto
             ↑         │
             └─ Undo / Reject demote
```

Minimum observable loop:

1. app đang chạy được thể hiện rõ ở tray;
2. typo có candidate thì overlay hiển thị candidate/source;
3. `Ctrl+.` hoặc sửa tự nhiên tạo evidence;
4. overlay có thông báo ngắn “Đã học X → Y” khi evidence thực sự được ghi;
5. settings có bảng rule: original, replacement, source, evidence, state;
6. có Forget cho từng rule và Forget last;
7. Auto correction có thông báo và đường Undo rõ ràng.

Acceptance smoke đề xuất:

- gõ một cặp sửa tự nhiên hai lần trong cùng app/focus;
- rule xuất hiện trong learned-rules UI;
- restart `OpenViKey.exe` vẫn còn rule;
- lần sau candidate xuất hiện đúng;
- model/capture không thay đổi trong password, terminal, English mode hoặc denylist.

---

## 8. Gap sản phẩm ngoài typing

### P0 — để gọi là standalone daily driver

- Không phụ thuộc TSF registration/publisher.
- Password guard host-side.
- Double-click chạy không cần CLI/console.
- Embed một lexicon được phép ship hoặc một artifact development được gắn nhãn rõ.
- Single-instance; lần mở thứ hai đưa settings ra trước.
- Tray icon ổn định; Exit flush dữ liệu.
- Persist Telex/VNI, V/E và suggestion setting.

### P1 — để người dùng thấy learning

- Learning event presentation.
- Learned-rules settings page.
- Forget/selective delete.
- Mô tả Suggest/Auto và hotkey trong UI.
- First-run lựa chọn Telex/VNI thay vì phụ thuộc CLI default.

### P2 — để phân phối

- Start with Windows opt-in.
- Product icon, version metadata và Windows manifest.
- Portable ZIP trước; installer sau.
- Xử lý lỗi startup bằng dialog/tray notification thay vì console.
- Review persistence production và bảo vệ local model.
- Signing/AV strategy.
- Nêu rõ giới hạn elevated apps; không tự elevation nếu chưa có quyết định sản phẩm.

---

## 9. Roadmap thay thế

### Standalone-0 — khóa lại kiến trúc

- ADR mới supersede phần bắt buộc của ADR 0007.
- TSF optional/experimental.
- Cập nhật README: Windows product là standalone hook host.
- Gỡ GĐ2c TSF-primary khỏi đường phát hành chính.

### Standalone-1 — restore app typing không TSF

- Thêm test “no TSF publisher” vẫn gõ Notepad normal.
- Tách `ContextProvider` optional khỏi typing runtime.
- Thêm host-side sensitive field detector.
- Test normal/password/focus transition không stale.
- Gate: máy sạch, English US keyboard active, chạy một EXE và gõ được.

### Standalone-2 — visible learning daily-driver

- Persist settings method/mode.
- Hiện learning feedback.
- Learned-rules read/forget API version hóa.
- Gate: natural correction → learned rule → restart → suggestion/behavior giữ nguyên.

### Standalone-3 — UniKey-style control window

- Đưa scope GĐ2d lên đây: method, tone, hotkeys, suggestions, learned rules, app deny/allow và autostart.
- Settings dùng cùng host process/runtime; không tạo session/hook thứ hai.

### Standalone-4 — packaging

- GUI subsystem, embedded assets, icon/metadata, single-instance, portable package.
- Installer/signing chỉ sau khi portable daily-driver ổn định.

### Optional-TSF — không nằm trên critical path

- Chỉ mở lại theo compatibility evidence cụ thể.
- Registration phải explicit, reversible và không thay default UX.

---

## 10. Quyết định đề xuất

1. Chọn **standalone hook host** làm sản phẩm Windows chính.
2. Không qua GĐ2c TSF-primary.
3. Gọi pha tiếp theo là `Standalone-1`, không tiếp tục tên cũ gây hiểu rằng registration là sản phẩm.
4. Giữ code TSF nhưng không enable/install/register mặc định.
5. Sau survey này, viết design spec Standalone-1 rồi implementation plan TDD trước khi sửa code.
6. Trên máy development hiện tại, unregister TSF và restore language list sau khi owner yêu cầu cleanup; không dùng môi trường đã đăng ký TSF làm acceptance environment duy nhất.
