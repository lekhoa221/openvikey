# OpenViKey Settings UI v1 — Design Specification

- **Ngày:** 2026-08-19
- **Trạng thái:** Implemented through UI-4 in working tree; automated review gates pass; manual visual acceptance pending
- **Mục tiêu:** Cửa sổ cài đặt native Windows trong cùng process `OpenViKey.exe`, quản lý cấu hình và quan sát rule học máy kiểu UniKey
- **ADR liên quan:** [`../../decisions/0010-windows-standalone-default.md`](../../decisions/0010-windows-standalone-default.md)
- **Spec kiến trúc nền tảng:** [`2026-08-19-openvikey-windows-standalone-design.md`](./2026-08-19-openvikey-windows-standalone-design.md)

---

## 1. Mục tiêu và Nguyên tắc cốt lõi

1. **Native Windows, cùng process**: Chạy trực tiếp trong `OpenViKey.exe` qua Win32 APIs (`windows` crate 0.62.2), không WebView/Electron, không tạo background process/backend thứ hai.
2. **Dễ dùng kiểu UniKey/EVKey**: Giao diện trực quan, nhẹ (< 10 MB RAM footprint cho toàn bộ app), phản hồi tức thì.
3. **Quan sát được học máy (Observable Learning)**: Người dùng xem được toàn bộ danh sách cặp từ đã học, trạng thái thăng cấp (`Observed` → `Suggest` → `Auto`), bằng chứng độ tin cậy và có thể chủ động quên từng rule hoặc quên rule gần nhất.
4. **Thay đổi cấu hình không gián đoạn**: Đổi phương pháp gõ (Telex/VNI), kiểu đặt dấu, gợi ý hay phím tắt được áp dụng ngay lập tức vào `TypingHost` mà không cần restart app.
5. **Độc lập, không đặc quyền**: Không yêu cầu quyền Administrator, không đăng ký TSF/COM, không ghi vào Registry hệ thống ngoài mục `Run` per-user khi bật autostart.
6. **Đóng cửa sổ không dừng bộ gõ**: Nút đóng (X) hoặc Alt+F4 chỉ ẩn cửa sổ (`ShowWindow(SW_HIDE)`). Chỉ lệnh *Thoát* từ Tray menu mới kết thúc process và tháo hook an toàn.

---

## 2. Bố cục và Thiết kế tổng thể

### 2.1 Kích thước & DPI Scaling
- **Kích thước mặc định**: 760 × 560 pixel (tại 96 DPI / 100% scale), căn giữa màn hình khi mở lần đầu.
- **Hỗ trợ DPI**: Per-Monitor V2 (`DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`), tự động tính toán lại font, padding và control bounds khi nhận `WM_DPICHANGED`.
- **Phông chữ**: Sử dụng phông chữ hệ thống tiêu chuẩn (`Segoe UI` hoặc system UI font qua `SystemParametersInfoW(SPI_GETNONCLIENTMETRICS)`).

### 2.2 Sơ đồ giao diện (Wireframe)

```text
┌────────────────────────────────────────────────────────────────────────┐
│ OpenViKey — Cài đặt                                       [—][□][×]    │
├─────────────────┬──────────────────────────────────────────────────────┤
│ [ Sidebar Nav ] │ [ Content View Area ]                                │
│                 │                                                      │
│ ⚙ Chung         │                                                      │
│ 🧠 Đã học       │                                                      │
│ 📱 Ứng dụng     │                                                      │
│ ⌨ Phím tắt      │                                                      │
│ 🛡 Riêng tư     │                                                      │
│ ℹ Giới thiệu    │                                                      │
│                 │                                                      │
├─────────────────┴──────────────────────────────────────────────────────┤
│ ● OpenViKey đang chạy · Local-only · v0.1.0        [Đóng]  [Áp dụng]  │
└────────────────────────────────────────────────────────────────────────┘
```

### 2.3 Thành phần Win32 Controls
- **Main Shell**: Window class `OpenViKeySettingsWindow`, xử lý dialog keyboard navigation (`IsDialogMessageW` trong host message loop để hỗ trợ Tab/Shift+Tab, Arrow keys, Space, Enter, Escape).
- **Sidebar**: Native ListBox (`WC_LISTBOXW`) hoặc Custom Navigation Bar với icon/text cho 6 trang:
  1. *Chung* (General)
  2. *Đã học* (Learned Rules)
  3. *Ứng dụng* (Applications Policy)
  4. *Phím tắt* (Hotkeys)
  5. *Riêng tư* (Privacy)
  6. *Giới thiệu* (About & Diagnostics)
- **Controls nội dung**: Standard Win32 Button (`BS_RADIOBUTTON`, `BS_AUTORADIOBUTTON`, `BS_AUTOCHECKBOX`, `BS_PUSHBUTTON`), ComboBox (`CBS_DROPDOWNLIST`), ListView (`WC_LISTVIEWW` với `LVS_REPORT | LVS_SINGLESEL | LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER`), Edit (`ES_AUTOHSCROLL`).

---

## 3. Tích hợp Tray Icon & Chuột

### 3.1 Click chuột trái
- Toggle nhanh chế độ gõ: **Tiếng Việt (V) ↔ Tiếng Anh (E)**.
- Cập nhật tooltip tray và icon trên khay hệ thống.

### 3.2 Double-click chuột trái
- Mở cửa sổ Settings hoặc đưa cửa sổ hiện tại lên foreground nếu đang mở.

### 3.3 Menu chuột phải
```text
  ┌─────────────────────────────────┐
  │  ● Tiếng Việt                   │
  │    Tiếng Anh                    │
  │ ─────────────────────────────── │
  │    Kiểu gõ: VNI  (hoặc Telex)   │
  │  ✓ Hiện gợi ý                   │
  │    Cho phép trong Terminal      │
  │ ─────────────────────────────── │
  │    Cài đặt...                   │
  │    Rule đã học...               │
  │ ─────────────────────────────── │
  │    Khởi động cùng Windows       │
  │    Thoát                        │
  └─────────────────────────────────┘
```
- Các mục có trạng thái checkmark (`MF_CHECKED` / `MF_UNCHECKED`) phản ánh đúng snapshot runtime của host.

---

## 4. Chi tiết các trang chức năng

### 4.1 Trang Chung (General)

#### Giao diện:
```text
Chế độ gõ
  (●) Tiếng Việt (V)
  ( ) Tiếng Anh (E)

Kiểu gõ
  (●) VNI
  ( ) Telex

Cách đặt dấu
  [ Hiện đại (òa, úy) ▼ ]  (hoặc Cổ điển: oà, uý)

Khi mở OpenViKey
  [ Khôi phục trạng thái trước ▼ ]  (hoặc: Luôn bật Tiếng Việt / Luôn bật Tiếng Anh)

Tùy chọn hiển thị
  [✓] Hiện khung gợi ý từ (Overlay)
  [ ] Khởi động cùng Windows

Hỗ trợ Terminal
  [✓] Cho phép gõ tiếng Việt trong Terminal (PowerShell, CMD, Windows Terminal)
      * Lưu ý: OpenViKey không học và không lưu bất kỳ nội dung nào gõ trong Terminal.
```

#### Quy tắc xử lý:
1. Đổi **Kiểu gõ** (Telex ↔ VNI) hoặc **Cách đặt dấu** (Modern ↔ Traditional):
   - Kích hoạt Caret Break trong session;
   - Reset buffer composition hiện tại (không backspace xóa chữ cũ trong ứng dụng);
   - Áp dụng cấu hình `EngineConfig` mới vào live `TypingHost`;
   - Ghi nhận vào `settings.json`.
2. Toggle **Terminal**:
   - Cho phép phím đi qua pipeline chuyển đổi tiếng Việt khi focus nằm ở terminal;
   - Invariant: Module learning và capture log luôn ở trạng thái **Bị khóa (Blocked)** đối với mọi terminal process.
3. Mọi cập nhật đều được ghi xuống `%LOCALAPPDATA%\OpenViKey\settings.json` bằng cơ chế ghi file atomic (.tmp → rename).

---

### 4.2 Trang Đã học (Learned Rules)

Trang hiển thị tính năng học thích ứng tự nhiên (Adaptive Learning) của OpenViKey.

#### Giao diện:
```text
Tìm kiếm rule: [____________________________________]

┌────────────┬──────────────┬────────────┬────────────┬──────────────┐
│ Đã gõ      │ Thay bằng    │ Trạng thái │ Bằng chứng │ Nguồn        │
├────────────┼──────────────┼────────────┼────────────┼──────────────┤
│ ko         │ không        │ Suggest    │ +2.0 / -0.0│ Abbreviation │
│ paht       │ phát         │ Observed   │ +0.0 / -0.0│ Rewind       │
│ dc         │ được         │ Auto       │ +5.0 / -0.0│ CandidateFix │
└────────────┴──────────────┴────────────┴────────────┴──────────────┘

Thông tin chi tiết rule:
  • Từ gốc:        ko
  • Từ thay thế:   không
  • Kiểu gõ:       VNI
  • Nguồn gốc:     Viết tắt / Ghép tự nhiên (Abbreviation)
  • Bằng chứng:    Tích cực: +2.0  |  Tiêu cực: -0.0
  • Trạng thái:    Suggest (Được ưu tiên hiện trong danh sách gợi ý)

[ Quên rule đã chọn ]    [ Quên rule vừa học ]    [ Làm mới ]
```

#### Trạng thái Rule (`RuleState`):
- `Observed`: Rule mới được phát hiện qua hành vi sửa từ hoặc gõ tự nhiên, chưa đủ bằng chứng để đề xuất.
- `Suggest`: Đã đạt ngưỡng tin cậy để hiển thị lên thanh gợi ý khi gõ tiền tố.
- `Auto`: Đạt độ tin cậy cao, bộ gõ sẽ tự động sửa ngay tại ranh giới từ (Space/Enter).

#### Quy tắc an toàn:
1. **Chỉ đọc qua snapshot an toàn**: UI gọi `host::control_snapshot()` để lấy dữ liệu; không mở khóa session hay lock blocking trên typing thread.
2. **Xóa rule có kiểm soát**: Lệnh *Quên rule đã chọn* hoặc *Quên rule vừa học* gửi command đến session reducer để mutate model coherent, sau đó kích hoạt background saver ghi ngay bản lưu bền vững.
3. **Tuyệt đối không lộ lịch sử gõ**: Bảng chỉ chứa các cặp từ biến đổi `(original, candidate)` đã được chuẩn hóa NFC, không bao giờ chứa cả câu văn, ngữ cảnh xung quanh hay dữ liệu riêng tư.

---

### 4.3 Trang Ứng dụng (Applications Policy)

Cho phép người dùng tùy biến hành vi của OpenViKey trên từng ứng dụng cụ thể.

#### Giao diện:
```text
Danh sách ứng dụng tùy biến:
┌────────────────────────┬─────────────┬─────────────┬──────────────┐
│ Tên tiến trình (EXE)   │ Chuyển đổi  │ Học máy     │ Profile      │
├────────────────────────┼─────────────┼─────────────┼──────────────┤
│ WindowsTerminal.exe    │ Allow       │ Block (Cố định)│ Win32     │
│ Cursor.exe             │ Default     │ Default     │ Electron     │
│ KeePassXC.exe          │ Block       │ Block       │ Auto         │
└────────────────────────┴─────────────┴─────────────┴──────────────┘

[ + Thêm ứng dụng ]    [ Lấy ứng dụng đang kích hoạt ]    [ - Xóa cấu hình ]
```

#### Ràng buộc nghiệp vụ:
1. **Tiến trình bảo mật / Quản lý mật khẩu**: Các EXE thuộc danh sách `DENYLIST` hệ thống (1Password, KeePass, Bitwarden, OpenSSH...) luôn bị **Block hoàn toàn** (cả chuyển đổi lẫn học máy), người dùng không thể ghi đè Allow.
2. **Terminal Invariant**: Các terminal (`cmd.exe`, `powershell.exe`, `WindowsTerminal.exe`...) có thể được đặt `Allow Transform`, nhưng `Learning Policy` luôn bị ép về `Block`.
3. **Ưu tiên Block**: Trong mọi trường hợp xung đột, quy tắc `Block` luôn có quyền ưu tiên cao nhất so với `Allow`.

---

### 4.4 Trang Phím tắt (Hotkeys)

Cung cấp khả năng tùy biến tổ hợp phím điều khiển.

#### Giao diện:
```text
Phím tắt chức năng:
  • Chuyển đổi Tiếng Việt / Tiếng Anh:   [ LeftCtrl + LeftShift ▼ ]
  • Nhận gợi ý đầu tiên:                 [ Ctrl + .             ]
  • Từ chối gợi ý:                       [ Ctrl + ,             ]
  • Hoàn tác từ tự sửa (Undo Auto):      [ Ctrl + Shift + Z     ]
  • Quên rule vừa học (Forget Last):     [ Ctrl + Shift + .     ]

[ Khôi phục mặc định ]
```

#### Ràng buộc & Kiểm tra hợp lệ:
1. **Chống trùng lặp (Conflict Detection)**: Không cho phép gán một tổ hợp phím cho 2 chức năng khác nhau.
2. **Yêu cầu Modifier Key**: Mọi phím tắt phải chứa ít nhất một phím bổ trợ (`Ctrl`, `Alt`, `Shift`, `Win`) kết hợp với phím chính, không cho phép phím ký tự đơn lẻ làm hotkey toàn cục.
3. Áp dụng ngay vào module `classify` và `policy` của host sau khi bấm lưu/áp dụng.

---

### 4.5 Trang Riêng tư (Privacy)

Cam kết bảo mật và minh bạch dữ liệu theo nguyên tắc Zero-Backend.

#### Giao diện:
```text
Cam kết quyền riêng tư & Bảo mật dữ liệu

OpenViKey hoạt động 100% cục bộ (Local-only) trên máy tính của bạn:
  ✓ Không có máy chủ phụ trợ (Zero Backend)
  ✓ Không thu thập dữ liệu ẩn danh hoặc telemetry
  ✓ Không tự động tải dữ liệu lên internet dưới mọi hình thức
  ✓ Trường mật khẩu (Password / PIN) tự động nhận diện và truyền phím thô (Pass-through)
  ✓ Terminal được cách ly tuyệt đối khỏi hệ thống học máy
  ✓ Không can thiệp hoặc đăng ký TSF/COM vào Windows

Vị trí lưu trữ dữ liệu cục bộ:
  C:\Users\<Username>\AppData\Local\OpenViKey

[ 📁 Mở thư mục dữ liệu trong Explorer ]

* Lưu ý phiên bản Preview: Mô hình học cá nhân hiện lưu ở dạng văn bản JSON cục bộ (.ovkdev.json).
  Vui lòng không chia sẻ file này nếu chứa các từ viết tắt riêng tư của bạn.
```

---

### 4.6 Trang Giới thiệu & Chẩn đoán (About & Diagnostics)

#### Giao diện:
```text
OpenViKey Standalone Preview
Phiên bản: 0.1.0 (x64)
Bản quyền: MIT License · OpenViKey Authors

Trạng thái Runtime hiện tại:
  • Trạng thái bộ gõ:          Đang hoạt động (Tiếng Việt - VNI)
  • Cửa sổ đang kích hoạt:     Notepad.exe (PID: 12480)
  • Phân loại trường nhập:     Normal (Cho phép chuyển đổi & học máy)
  • Single Instance Guard:     Đã kích hoạt (1 process duy nhất)
  • TSF / COM Registration:    Không đăng ký (Clean OS footprint)
  • Tổng số rule đã học:       14 rules

[ Kiểm tra trạng thái hệ thống ]    [ Sao chép thông tin chẩn đoán ]
```

- **Quy tắc an toàn**: Thông tin chẩn đoán tuyệt đối không bao gồm buffer phím, chuỗi ký tự vừa gõ hay nội dung văn bản xung quanh.

---

## 5. Kiến trúc Kỹ thuật & Luồng Điều khiển

### 5.1 Sơ đồ luồng Coordinator và UI

```text
┌─────────────────────────────────────────────────────────────┐
│                    OpenViKey.exe                            │
│                                                             │
│  ┌──────────────────────────┐   UiCommand    ┌───────────┐  │
│  │   SettingsWindow (Win32) │───────────────►│           │  │
│  │   (Main UI Thread)       │◄───────────────│           │  │
│  └──────────────────────────┘ UiViewSnapshot │           │  │
│                                              │           │  │
│  ┌──────────────────────────┐                │  Runtime  │  │
│  │   TraySink / ContextMenu │───────────────►│Coordina-  │  │
│  └──────────────────────────┘                │   tor     │  │
│                                              │           │  │
│  ┌──────────────────────────┐                │           │  │
│  │   LL Hooks / SendInput   │◄──────────────►│           │  │
│  └──────────────────────────┘                └─────┬─────┘  │
│                                                    │        │
│                                                    ▼        │
│                                           %LOCALAPPDATA%\...│
└─────────────────────────────────────────────────────────────┘
```

### 5.2 Command & Snapshot Interface

```rust
pub enum UiCommand {
    SetMode(Mode),
    SetInputMethod(InputMethod),
    SetTonePlacement(TonePlacement),
    SetSuggestions(bool),
    SetTerminalTransform(bool),
    SetStartWithWindows(bool),
    SetHotkey { action: HotkeyAction, shortcut: String },
    SetAppPolicy(AppPolicyV1),
    RemoveAppPolicy(String),
    ForgetRule { original_nfc: String, candidate_nfc: String },
    ForgetLastRule,
    OpenDataFolder,
    ApplySettings,
}

pub struct UiViewSnapshot {
    pub settings: SettingsV1,
    pub mode: Mode,
    pub engine_config: EngineConfig,
    pub show_suggestions: bool,
    pub foreground_exe: String,
    pub learning_allowed: bool,
    pub learned_rows: Vec<LearnedRuleRowSnapshot>,
    pub autostart_enabled: bool,
}
```

---

## 6. Kế hoạch triển khai theo từng lát cắt (Implementation Slices)

### Slice UI-1: Window Shell & Trang Chung
- Tạo `SettingsWindow` native Win32 (kích thước 760×560, Per-Monitor DPI aware).
- Cấu trúc thanh Sidebar Navigation và chuyển đổi các container view.
- Triển khai toàn bộ nội dung Trang Chung: Chế độ gõ, Kiểu gõ Telex/VNI, Cách đặt dấu, Bật/tắt Gợi ý, Terminal opt-in, Khởi động cùng Windows.
- Tích hợp đóng cửa sổ ẩn xuống khay (`Close-to-Tray`) và nhận tín hiệu mở từ Single Instance.

### Slice UI-2: Trang Đã học (Learned Rules Management)
- Tạo bảng `SysListView32` hiển thị các rule học máy.
- Thanh tìm kiếm/lọc real-time theo từ gõ hoặc từ thay thế.
- Card hiển thị chi tiết độ tin cậy và nguồn gốc rule khi click chọn dòng.
- Nút bấm và handler xử lý *Quên rule đã chọn* và *Quên rule vừa học* liên kết với model saver.

### Slice UI-3: Trang Ứng dụng & Phím tắt
- Bảng danh sách ứng dụng tùy biến per-app policy.
- Dialog / popup thêm ứng dụng mới hoặc tự động lấy tên EXE đang focus.
- Form cấu hình phím tắt với logic kiểm tra xung đột (conflict validation).

### Slice UI-4: Trang Riêng tư, About & Hoàn thiện Accessibility
- Trang Riêng tư với nút mở trực tiếp `%LOCALAPPDATA%\OpenViKey`.
- Trang Giới thiệu & bảng chẩn đoán trạng thái runtime.
- Keyboard navigation pass (Tab, Shift+Tab, Enter, Escape, Space).
- Cập nhật Tray context menu đầy đủ các mục checkmark.

### Trạng thái triển khai 2026-08-19

- **Visual direction accepted:** phối hợp **A — UniKey Compact** và **B — OpenKey Modern**: navigation ngang, header V/E luôn hiện, full-width content, typography/spacing rõ và không dùng sidebar emoji.
- **UI-1:** hoàn tất — shell native, General, tray/single-instance, DPI, close-to-tray.
- **UI-2:** hoàn tất — ListView learned rules, filter, detail không lộ surrounding context, exact Forget và durable reload test.
- **UI-3:** hoàn tất — per-app transform/learning/injection policy và hotkey validation/live apply.
- **UI-4:** hoàn tất — Privacy, mở data folder, About/runtime diagnostics và dialog navigation.
- `UiCommand` coordinator là boundary duy nhất cho mutation do Settings window khởi phát.
- Manual visual/DPI/real-app acceptance vẫn phải thực hiện trên package release.

---

## 7. Tiêu chí Nghiệm thu (Acceptance Gate)

1. Double-click tray hoặc chạy instance thứ 2 mở đúng cửa sổ Settings; đóng cửa sổ (X/Alt+F4) bộ gõ vẫn chạy bình thường.
2. Đổi Telex/VNI trên UI có tác dụng gõ ngay lập tức trên Notepad/Chrome mà không cần khởi động lại app.
3. Gõ một từ được học tự nhiên (ví dụ `ko` → `không`), mở trang Đã học thấy xuất hiện rule với trạng thái và điểm tin cậy tương ứng.
4. Bấm *Quên rule đã chọn*, rule biến mất khỏi UI và gõ lại `ko` không còn tự động gợi ý/sửa thành `không`.
5. Đặt policy `Block` cho một EXE bất kỳ, khi focus vào EXE đó phím bấm đi thẳng (Pass raw), không biến đổi và không học.
6. Toàn bộ test suite `cargo test --workspace` và `cargo clippy` đều vượt qua không có cảnh báo nào.
