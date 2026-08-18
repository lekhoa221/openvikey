# OpenViKey GĐ2 — Windows host (thiết kế master)

- **Ngày:** 2026-08-18
- **Trạng thái:** v1.3 — GĐ2a implemented; GĐ2b survey complete; GĐ2d Windows product UI recorded
- **Governing spec:** [`2026-08-17-openvikey-design.md`](./2026-08-17-openvikey-design.md)
- **Part 2 (lab capture):** [`2026-08-17-openvikey-part2-personal-capture-design.md`](./2026-08-17-openvikey-part2-personal-capture-design.md)
- **Pha đầu (spec chi tiết):** [`2026-08-18-openvikey-gd2a-hook-electron-design.md`](./2026-08-18-openvikey-gd2a-hook-electron-design.md)
- **Master plan (3 plan nhỏ):** [`../plans/2026-08-18-openvikey-gd2-master-plan.md`](../plans/2026-08-18-openvikey-gd2-master-plan.md)
- **License:** MIT (kế thừa)

---

## 0. Bối cảnh & động lực

Part 1 + Part 2 đã có bộ não và vòng học trong `openvikey-lab session`. Người dùng muốn **gõ Telex ngay lúc làm việc** (ô prompt AI Agent trong Cursor, Word, Chrome), không vào harness TTY.

Spec gốc ghi **“GĐ2 = TSF”**. Khảo sát repo tham chiếu tại `D:\Workspace\CloneFromGit\VNKeyboard` (2026-08-18) cho thấy bộ gõ tiếng Việt Windows **đang dùng được hàng ngày** không đi TSF-only:

| Nguồn | License | Surface Windows | Bài học |
|---|---|---|---|
| VKey | GPL-3.0 | Hook `WH_KEYBOARD_LL` là **nhập chính**; TSF đọc ngữ cảnh / mật khẩu; TSF nhập chính **theo app** | Mô hình sản phẩm trưởng thành nhất trong clone |
| OpenKey win32 | GPL-3.0 | Chỉ hook + `SendInput` | Đủ “mọi app” kiểu UniKey; changelog đầy vá Chrome |
| UniKey host Windows | đóng | Hook (host không có trong clone) | Engine Telex mở; GUI Windows không phải nguồn MIT |
| EVKey clone | không source | — | Không dùng được cho thiết kế |
| goxkey / PHTV | BSD-3 / AGPL | Không Windows (`CGEventTap`) | Cùng họ intercept → engine → sửa app |
| vietc | MIT | Linux evdev | Direct input; password + per-app — ý tưởng, không code Windows |

**Không chép mã GPL/AGPL** (OpenKey, VKey, PHTV, espanso). Chỉ học kiến trúc. Credit bắt buộc khi triển khai: [VKey](https://github.com/phatMT97/VKey), [OpenKey](https://github.com/tuyenvm/OpenKey), TSF InputScope tham khảo [VietType](https://github.com/dinhngtu/VietType) (VKey cũng ghi vậy).

### Quyết định đã chốt (2026-08-18)

| Chủ đề | Quyết định |
|---|---|
| Mô hình GĐ2 | **Hybrid VKey-shaped:** hook = đường gõ chính; TSF = ngữ cảnh + tuỳ chọn nhập chính theo app |
| Không làm | TSF-only như README cũ; hook-only OpenKey như trạng thái cuối; Cursor extension-only |
| Não | Tái dùng `openvikey-core` + reducer session (Part 2). **Không** sửa `engine/`, `types.rs`, `model.rs` |
| License | MIT; `unsafe` chỉ trong crate Windows; không link GPL |
| “Xong GĐ2” | Bốn pha độc lập, mỗi pha ra phần mềm chạy được + test; GĐ2a đủ dùng hàng ngày với Agent; GĐ2d hoàn thiện control surface Windows |
| G3 / lexicon lớn | **Không** thuộc GĐ2. IME mọi app không làm từ điển fixture thành tiếng Việt đầy đủ |

Sửa chữ trên README / spec phần 1: GĐ2 không còn là “Windows qua TSF” mà là **Windows host (hook + TSF theo pha)**.

---

## 1. Bốn pha — hợp đồng tách

Mỗi pha có spec (2a đã có; 2b/2c/2d viết khi bắt đầu pha đó), một implementation plan, và tiêu chí “xong” riêng. **Không** gộp TSF DLL vào 2a hoặc product UI vào 2b/2c.

```text
GĐ2a  hook + Electron inject + học session
  └─► GĐ2b  TSF đọc ngữ cảnh / InputScope (hook vẫn nhập chính)
        └─► GĐ2c  TSF nhập chính theo app (UWP / anti-cheat)
              └─► GĐ2d  Windows settings/control UI
```

| Pha | Việc người dùng làm được | Surface OS | Học / document |
|---|---|---|---|
| **2a** | Gõ Telex trong Notepad **và** ô chat Cursor; UniKey tắt | `WH_KEYBOARD_LL` + `SendInput`; profile Electron | Session buffer = từ đang gõ + token vừa commit **trong cùng focus**; đổi cửa sổ = `Reset` |
| **2b** | Ô password/PIN không bị Telex; left-context đọc được khi TSF cho phép | Thêm DLL TSF **chỉ đọc** (InputScope + surrounding text). Hook vẫn gõ | `left_context` giàu hơn; `allow_transform=false` theo InputScope |
| **2c** | App trong list TSF (UWP, một số game chống cheat) gõ được | TSF **nhập chính** cho những exe đó (composition Windows, có gạch chân) | Cùng reducer; adapter `ReplaceRange` → UTF-16 range |
| **2d** | Cấu hình và kiểm tra OpenViKey qua cửa sổ kiểu UniKey | Settings/control UI trên host 2a–2c đã ổn định | Xem/xóa luật đã học; quản lý method, hotkey, app policy và autostart |

**Không thuộc GĐ2 (mọi pha):** macOS GĐ3, G3 corpus, PII export, sync/CRDT, production key management, macro automation, automatic upload, keymap Tab/Esc như lab. Post-checkpoint ADR 0008 uses open JSON only for the Windows development host; Part 2 lab encryption remains.

---

## 2. Kiến trúc đích (cả GĐ2)

```text
  phím Windows
       │
       ▼
  ┌─────────────┐     policy        ┌──────────────────┐
  │ openvikey-  │  eat / pass /     │ openvikey-session│
  │ win host    │  hotkey           │  LabSession      │
  │ hook+inject │ ────────────────► │  document/capture│
  │ overlay/tray│ ◄──────────────── │  DebouncedSaver  │
  └─────────────┘  EngineAction     └────────┬─────────┘
       │                                     │
       │ SendInput / (2c) TSF range          │
       ▼                                     ▼
     app đang focus                    openvikey-core
```

- **`openvikey-core`:** không OS, không `unsafe`. Giữ `unsafe_code = forbid`.
- **`openvikey-session`:** tách reducer Part 2 khỏi TTY (document, capture, session, persistence). Lab và win cùng phụ thuộc crate này.
- **`openvikey-lab`:** CLI + REPL; re-export session để test cũ ít đổi.
- **`openvikey-win`:** hook, injector, overlay, tray. `unsafe` chỉ trong module Win32. **Không** phụ thuộc `crossterm`/`clap` của lab nếu có thể dùng `clap` riêng. Development persistence is open JSON under ADR 0008.
- **`openvikey-win-tsf` (2b/2c):** DLL C++ hoặc `windows` COM — **không** viết trong 2a.
- **Windows product UI (2d):** framework/process boundary chọn sau 2c; chỉ gọi settings/model-inspection API đã version hóa, không sở hữu hook, TSF hoặc session state riêng.

Workspace hiện `unsafe_code = "forbid"` toàn cục → crate win **override** `unsafe_code = "allow"` và giới hạn file.

---

## 3. Ràng buộc toàn GĐ2 (mọi pha phải theo)

1. **Zero backend / không telemetry / không upload** — như spec phần 1 §2.4.
2. **Không chép** OpenKey/VKey/espanso/PHTV. Test Electron tự viết; không port “bait character” từ VKey trừ khi spike Cursor 2a thất bại — khi đó ghi ADR riêng, vẫn tự implement, vẫn credit VKey về *hiện tượng* Chromium đa tiến trình.
3. **Tab và Esc luôn đi vào app** (Cursor autocomplete / Agent). Accept/reject dùng hotkey khác (khoá ở spec 2a).
4. **Hai bộ gõ cùng lúc = undefined.** README: tắt UniKey/EVKey/VKey trước khi chạy.
5. **Perf + lock:** P95 `TypingHost::handle_key` (không FFI) < 15 ms. Hook callback **không** `Mutex::lock`, **không** sleep, **không** serialize/Argon2. Saver clone model/log dưới lock ngắn, seal ngoài lock. `try_lock` fail → Pass.
6. **Sensitive:** core chỉ nhận `InputContext` flags; host quyết định flags. 2a = denylist exe; 2b = InputScope.
7. **Enter trên OS:** không eat thành U+0020. Lab REPL vẫn Space/Enter = space; win host `CommitAndPass` newline.
8. **G3 không đóng** vì có IME.

---

## 4. Tiêu chí xong từng pha (đo được)

### 4.1 GĐ2a
- Unit: policy eat/pass, composition→inject commands, injector recording, denylist, hotkey.
- `cargo test --workspace` xanh trên `windows-latest` (CI hiện tại).
- Manual (không gate CI): Notepad `xin chao` Telex → `xin chào`; Enter xuống dòng; Cursor composer cùng Telex **và Enter gửi prompt**; Ctrl+. nhận gợi ý; Tab trong Cursor **không** bị nuốt.

### 4.2 GĐ2b
- Khảo sát trước thiết kế: [`2026-08-19-openvikey-gd2b-tsf-context-survey.md`](./2026-08-19-openvikey-gd2b-tsf-context-survey.md).
- Test InputScope password/PIN → hook `Pass` trước khi eat key; không đọc surrounding text, không transform/learn/capture.
- Test surrounding text (mock TSF) → `left_context.prev_token_nfc` khác rỗng khi TSF trả token trái.
- Development persistence tiếp tục là plaintext `.ovkdev.json`; GĐ2b không thêm password/passphrase.
- Manual: Notepad password field (nếu có) / Chrome password không Telex.

### 4.3 GĐ2c
- App trong allowlist TSF nhận composition qua TSF, không double-inject hook.
- UWP Notepad hoặc app Store mẫu gõ được Telex.
- Manual: gạch chân composition là chấp nhận được trên list đó.

### 4.4 GĐ2d
- Settings round-trip qua một schema version hóa; restart host giữ nguyên cấu hình.
- V/E, method, suggestions, hotkeys, app routing và autostart có control surface rõ ràng.
- Xem và xóa chọn lọc rule đã học; đóng cửa sổ không dừng tray host.
- Installer, product icons và signing là release gates sau khi UI ổn định.

---

## 5. Việc cố ý không làm trong GĐ2

- Đọc transcript Cursor / nạp hội thoại Agent làm corpus.
- Sở hữu document buffer của Word/Cursor (B1 lab không dịch 1-1 ra OS).
- Macro editor/automation như các bộ gõ mở rộng; GĐ2d chỉ làm settings/control và learned-data inspection.
- Code signing / Authenticode (ghi nhận AV false positive; làm sau nếu phát hành).

---

## 6. ADR

Quyết định kiến trúc GĐ2: [`docs/decisions/0007-gd2-windows-hybrid-host.md`](../../decisions/0007-gd2-windows-hybrid-host.md).

Windows development persistence override: [`docs/decisions/0008-open-development-persistence.md`](../../decisions/0008-open-development-persistence.md).
