# OpenViKey GĐ2a — Hook + Electron inject (thiết kế)

- **Ngày:** 2026-08-18
- **Trạng thái:** implemented; v3 + post-checkpoint open-persistence amendment (ADR 0008)
- **Master GĐ2:** [`2026-08-18-openvikey-gd2-windows-host-design.md`](./2026-08-18-openvikey-gd2-windows-host-design.md)
- **Part 2 reducer:** [`2026-08-17-openvikey-part2-personal-capture-design.md`](./2026-08-17-openvikey-part2-personal-capture-design.md)
- **Plan:** [`../plans/2026-08-18-openvikey-gd2a-implementation-plan.md`](../plans/2026-08-18-openvikey-gd2a-implementation-plan.md)
- **License:** MIT

---

## 0. Mục tiêu

Tắt UniKey, chạy `openvikey-win`, gõ Telex trong Notepad **và** ô prompt Cursor. Enter **gửi prompt / xuống dòng**, không thành dấu cách.

### Changelog v2 (review 2026-08-18)

Đối chiếu `Engine::process`, `CorrectionSlice.action`, `LabSession::{process_event,commit_token,accept_top}`:

| ID | Kết luận | Sửa |
|---|---|---|
| A1 | **Đúng.** `ReplaceRange`/`ShowSuggestions` nằm ở `obs.action`, không phải `engine_actions`. Auto commit dùng `action.replacement` trong document, `Commit.text` vẫn là bản chưa sửa. | §4 đọc `action` |
| A2 | **Đúng.** Eat Enter → U+0020 phá Cursor. Mapping lab TTY không dịch 1-1. | `CommitAndPass` |
| A3 | **Đúng.** Saver `lock` + serialize 20k record; hook blocking lock → `LowLevelHooksTimeout`. | §Threading |
| A4 | **Đúng.** `Reset` → `UpdateComposition{""}` → backspace nhầm app mới. | `ObservationOrigin::CaretBreak` |
| B1 | **Đúng.** Spec nói click Reset nhưng không có mouse/nav. | §3.4 |
| B2 | **Đúng hiện tượng.** Sleep 8 ms trên hook mâu thuẫn “callback must not block”. | `is_sending`; **cấm sleep trên hook**; `electron_gap_ms` default **0** |
| B3 | **Đúng.** `accept_top`/`undo_last` không emit `EngineAction`. Đổi return `SessionObservation` không cứu. | `AcceptVisual` / `UndoVisual` |
| B4 | **Đúng.** `OpenProcess` mỗi key là chậm và fail elevated. | `EVENT_SYSTEM_FOREGROUND` + cache HWND |
| C1–C5 | **Đúng** (C5: `engine/` là thư mục). | Plan v2 |

Không chép mã VKey (GPL). Có credit ý tưởng: hook-thread không mutex; leaked-key lúc SendInput; winevent focus.

### Changelog v3 (review P0/P1, đối chiếu WH_KEYBOARD_LL + `capture_log`)

| ID | Kết luận | Sửa |
|---|---|---|
| P0-1 | **Đúng.** `EatAndIgnore` + hook return 1 nuốt luôn `SendInput` (`OVK_EXTRA`). | `OVK_EXTRA` → `Pass`, không session |
| P0-2 | **Đúng.** Queue + `CommitAndPass` để Enter lọt app trước auto-replace. | **Bỏ queue phím.** `try_lock` + inject **đồng bộ trong callback**; Enter Pass **sau** inject |
| P0-3 | **Đúng.** Task 2 thiếu VK_BACK, modifier shortcuts, key-up, Caps, hotkey, punctuation inject. | §3.1 đầy đủ; `AppendDelimiter` mọi delimiter trừ `\\n` |
| P1-4 | **Đúng.** `capture_log()` gọi `model_payload()` (serialize dưới lock). Hai `try_lock` lệch revision. | `SessionSaveSnapshot` một lock |
| P1-5 | **Đúng.** `repl.rs` match `AcceptTop`/`UndoLast` cùng kiểu `()`. `commands_from_undo` thiếu. | `let _ = …`; undo + `last_injected_token` + HWND |
| P1-6 | **Đúng.** `bool sending` trên injector không share với hook. | `Arc<AtomicBool>` + `SendingGuard` |
| P1-7 | **Đúng.** Task 7–9 stub-able. P95 unit flaky. | Tách task; P95 `#[ignore]`; `no_key_log` gồm `host.rs` |

### Quyết định đã chốt

| Chủ đề | Quyết định |
|---|---|
| Surface | `WH_KEYBOARD_LL` + `WH_MOUSE_LL` (chỉ `WM_LBUTTONDOWN` → caret-break) + `SendInput` |
| Auto trên app | `obs.action == ReplaceRange` **thay** `Commit.text` khi inject |
| Enter | `CommitAndPass { '\\n' }`: session+inject **xong rồi** `CallNextHookEx`. Không queue |
| Space | `EatAndInject(Boundary { ' ' })` rồi inject U+0020 |
| Tab / Esc | **Luôn Pass** |
| Accept / Reject | `Ctrl+.` / `Ctrl+,` |
| Undo Auto | `Ctrl+Shift+Z` |
| Toggle | Left Ctrl + Left Shift **keyup**, không phím khác trong chord |
| Hook thread | **Không** `Mutex::lock` (blocking), **không** sleep, **không** serialize. **`try_lock` + SendInput đồng bộ trong callback.** Không mpsc hàng đợi phím |
| `OVK_EXTRA` | `Pass` (`CallNextHookEx`), không eat, không session |
| `is_sending` | `Arc<AtomicBool>` dùng chung hook + injector. Chỉ **phím vật lý** (extra ≠ OVK) lúc sending → eat |
| Electron gap | Default **0**. Cấm sleep trên hook |
| Password | Denylist exe (InputScope = 2b) |
| Tray | Icon V/E + Exit |
| Crate | `openvikey-session`; win không phụ thuộc REPL/crossterm |
| Core | Không sửa `engine/**`, `types.rs`, `model.rs` |
| `windows` crate | Pin **0.62.2** (MIT OR Apache-2.0) |

---

## 1. Phạm vi

### 1.1 Trong phạm vi
- Extract `openvikey-session`.
- `openvikey-win`: policy, sync (kể cả `action` + visual accept/undo), injector, classify, hook+mouse, focus cache, overlay, tray, CLI, persist clone-then-serialize.
- Nav keys + chuột trái → caret-break, **Pass** (app nhận phím/click).
- `is_sending`: chỉ nuốt **phím vật lý** trùng lúc SendInput; sự kiện `OVK_EXTRA` luôn Pass.

### 1.2 Ngoài phạm vi
- TSF, InputScope, bait character, autostart, Authenticode, G3, Tab-as-accept.
- Sleep trên hook để “sửa” Electron. Nếu Cursor lệch chữ: dừng, ADR riêng, không port VKey.

---

## 2. Crate & file

```text
crates/openvikey-session/   # copy document, capture, session, persistence; unsafe forbid
crates/openvikey-lab/       # re-export; không xóa file
crates/openvikey-win/
  src/lib.rs
  src/console.rs            # Ctrl+C/console-close → cooperative shutdown + final flush
  src/policy.rs
  src/sync.rs
  src/inject.rs             # unsafe SendInput only
  src/classify.rs
  src/focus.rs              # winevent thread + HWND cache
  src/host.rs
  src/hook.rs               # unsafe LL keyboard; KHÔNG lock/sleep
  src/mouse.rs              # unsafe LL mouse; chỉ LBUTTONDOWN → callback caret-break
  src/overlay.rs
  src/tray.rs
  src/passphrase.rs         # retired placeholder; Windows host không còn export module này
  src/main.rs
  tests/{policy,sync,inject,classify,host,hook,hook_thread_discipline,focus,overlay,tray,passphrase,persist,no_key_log}.rs
```

`openvikey-win` **không** `[lints] workspace = true` (workspace `unsafe_code = "forbid"` áp `#![forbid(unsafe_code)]`, rustc không cho `allow` đè). Chép bảng `[workspace.lints.clippy]` vào crate; `[lints.rust] unsafe_code = "allow"`.

---

## 3. Policy (`policy.rs`) — thuần

```rust
pub const OVK_EXTRA: usize = 0x4F564B31;

pub enum KeyDecision {
    Pass,
    EatAndIgnore,
    EatAndInject(InputKind),
    CommitAndPass { delimiter: char },
    Hotkey(HostHotkey),
    CaretBreakAndPass,
}
```

`HostState` lock-free trên hook: `mode: AtomicU8`, `is_sending: AtomicBool`, foreground exe đọc từ `FocusCache` (đã fill sẵn).

### 3.1 Thứ tự quyết định (khoá)

Chỉ xét **keydown**, trừ bước Toggle (keyup). Keyup không phải toggle → `Pass`.

1. `extra_info == OVK_EXTRA` → **`Pass`** (không session, không eat). `SendInput` của mình phải tới app.
2. `is_sending == true` **và** extra ≠ OVK → `EatAndIgnore` (nuốt leak/auto-repeat vật lý)
3. `Tab` / `Esc` → `Pass`
4. Denylist hoặc English → `Pass` trừ Toggle
5. `control \|\| alt \|\| meta` và **không** phải hotkey OpenViKey → `Pass` (Ctrl+C/V/X/A, Alt+Tab, Win+…)
6. Hotkeys (chỉ keydown): `Ctrl+.` Accept; `Ctrl+,` Reject; `Ctrl+Shift+Z` Undo. Toggle: cả Left-Ctrl và Left-Shift đã down, keyup một trong hai, không phím khác trong chord
7. Nav: Left/Right/Up/Down/Home/End/Prior/Next/Delete → `CaretBreakAndPass`
8. `VK_RETURN` keydown → `CommitAndPass { '\\n' }`
9. `VK_SPACE` keydown → `EatAndInject(Boundary { ' ' })`
10. `VK_BACK` keydown → `EatAndInject(Backspace)`
11. Chữ/số: `EatAndInject(Key { logical })` với `logical` = shift XOR caps_lock
12. Punctuation ASCII thuộc `is_boundary_char` (trừ whitespace đã xử lý): `EatAndInject(Boundary { delimiter })` — gồm `. , ; : ? !` và các ký tự `backend.rs` còn lại khi gõ được không kèm Ctrl

`Ctrl+Z` không phải Undo OpenViKey.

### 3.1.1 Hook return (WH_KEYBOARD_LL)

| `KeyDecision` | Return | Session |
|---|---|---|
| `Pass` (kể cả OVK_EXTRA) | `CallNextHookEx` (0) | không |
| `EatAndIgnore` | 1 | không |
| `EatAndInject` / `Hotkey` | 1 **sau** `try_lock` + inject | đồng bộ |
| `CommitAndPass` / `CaretBreakAndPass` | `CallNextHookEx` **sau** `try_lock` + inject/reset | đồng bộ |

`try_lock` fail: `EatAndInject` → `Pass` (0); `CommitAndPass` → **1** (nuốt Enter, không gửi prompt nửa từ); `Hotkey` → 1; `CaretBreakAndPass` → 0 (app vẫn nhận nav/click).

**Không** mpsc hàng đợi phím. Winevent/mouse caret-break: `try_lock` trên thread của chúng, không trì hoãn Enter.

`HostState.caps_lock: bool` (callback đọc `GetKeyState(VK_CAPITAL)`).

### 3.2 Denylist exe

`WindowsTerminal.exe`, `powershell.exe`, `pwsh.exe`, `cmd.exe`, `conhost.exe`, `OpenSSH.exe`, `ssh.exe`, `putty.exe`, `1Password.exe`, `KeePass.exe`, `KeePassXC.exe`, `Bitwarden.exe`, `loginui.exe`. So `eq_ignore_ascii_case` trên file name.

### 3.3 Focus (không OpenProcess trên hook)

Thread riêng: `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, ...)`. Callback: HWND → exe (OpenProcess **ở đây**, không phải key path) → `FocusCache { hwnd, exe, profile }`. Hook chỉ `load` cache. Elevated fail → exe `""` → treat as Win32, không denylist.

Đổi HWND so với `last_injected_hwnd` **trước** phím mới, trong cùng `try_lock`: `commands_from_caret_break` rồi mới xử lý phím. Accept-after-commit / undo chỉ khi `hwnd == last_injected_hwnd`.

### 3.4 Chuột

`WH_MOUSE_LL`: `WM_LBUTTONDOWN` (và RBUTTON/MBUTTON) → host `CaretBreakAndPass` tương đương (không eat chuột). Không xử lý move.

---

## 4. Composition sync — hợp đồng với core

**Sự thật core (không được viết lại):**

- `Engine::process` chỉ emit `UpdateComposition` và `Commit` (`engine/mod.rs`).
- `ReplaceRange` / `ShowSuggestions` chỉ gán `CorrectionSlice.action` → `SessionObservation.action` (`correction.rs`, `session.rs` observation).
- Auto lúc commit: `commit_token` đẩy `action.replacement` vào document; `engine_actions` vẫn `Commit { text: snapshot.rendered }` gốc (`session.rs` `commit_token`).
- Auto range basis lúc commit là `ActiveComposition` (không phải `CommittedBeforeCaret`).
- `Reset` luôn emit `UpdateComposition { text: "" }`.
- `accept_top` / `undo_last` sửa `document` trực tiếp, **không** đẩy `EngineAction`.

```rust
pub enum ObservationOrigin { Typed, CaretBreak }

pub enum InjectCommand {
    Replace { backspace_graphemes: usize, text_nfc: String },
    AppendDelimiter { delimiter: char },
}

pub fn commands_from_typed(
    obs: &SessionObservation,
    sent_nfc: &str,
) -> (Vec<InjectCommand>, String);

pub fn commands_from_caret_break(sent_nfc: &str) -> (Vec<InjectCommand>, String);
// luôn ([], "") — quên sent, KHÔNG backspace

pub fn commands_from_accept(
    visual: &AcceptVisual,
    sent_nfc: &str,
) -> (Vec<InjectCommand>, String);

pub fn commands_from_undo(
    visual: &UndoVisual,
    sent_nfc: &str,
) -> (Vec<InjectCommand>, String);
```

### 4.1 `commands_from_typed`

1. Nếu `obs.action == Some(ReplaceRange(action))` **và** có `Commit` trong `engine_actions`:
   - `Replace { backspace_graphemes: grapheme_len(sent), text_nfc: action.replacement }`
   - nếu `Commit.delimiter == Some('\n')` → **không** AppendDelimiter
   - nếu `Commit.delimiter == Some(d)` và `d != '\n'` → `AppendDelimiter { d }` (space **và** `. , ; : ? !` …)
   - `sent = ""`
   - `last_injected_token = replacement` (host cập nhật khi apply commands)
   - **bỏ qua** `Commit.text` gốc
2. Else duyệt `engine_actions`:
   - `UpdateComposition { text }` → `Replace { len(sent), text }`, `sent = text`
   - `Commit { text, delimiter: Some('\n') }` → replace tới `text` nếu `sent != text`, **không** AppendDelimiter, `sent = ""`, `last_injected_token = text` (hoặc replacement nếu có auto — nhánh 1)
   - `Commit { text, delimiter: Some(d) }` `d != '\n'` → replace nếu cần, `AppendDelimiter { d }`, `sent = ""`, `last_injected_token = text`
3. `ShowSuggestions` trên `action` → không inject (overlay đọc `obs.candidates`).
4. Không bịa `ReplaceRange` `CommittedBeforeCaret` trong unit test typed path — state đó **không** do engine emit lúc auto.

### 4.2 Caret-break

`commands_from_caret_break` → `([], "")`. Caller **không** gọi `commands_from_typed` trên observation Reset/CursorMoved.

### 4.3 Accept / Undo visual

Session API mới (crate session, lab REPL bỏ qua visual nếu muốn):

```rust
pub struct AcceptVisual {
    pub candidate_nfc: String,
    pub was_composing: bool,
    /// Token đang hiện ở app cho từ này: composition (`sent`) hoặc last committed.
    pub replace_len_hint: usize, // host dùng sent hoặc last_injected_token
}

pub struct UndoVisual {
    pub show_nfc: String, // inverse.replacement
}
```

- Accept đang soạn (`was_composing`): `Replace { len(sent), candidate }` + `AppendDelimiter { ' ' }`, `sent = ""`, `last_injected_token = candidate`.
- Accept sau commit: chỉ khi `hwnd == last_injected_hwnd` và `last_injected_token` không rỗng. `Replace { grapheme_len(last_injected_token), candidate }`. Khác HWND → no-op visual (vẫn học trong session nếu `accept_top` đã chạy — **khoá:** host **không** gọi `accept_top` nếu HWND lệch).
- Undo: cùng ràng HWND; `Replace { grapheme_len(last_injected_token), show_nfc }`; cập nhật `last_injected_token = show_nfc`.
- Reject: không inject.

Lab REPL (`repl.rs` match): mỗi arm block `{ let _ = session.accept_top(at_ms); }` / undo tương ứng — **không** để hai `Option<T>` khác kiểu trong cùng match.

`commands_from_undo(visual, last_injected_token)`:

```rust
vec![InjectCommand::Replace {
    backspace_graphemes: grapheme_len(last_injected_token),
    text_nfc: visual.show_nfc.clone(),
}]
```

---

## 5. Injector

```rust
pub trait InputSender {
    fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError>;
}
pub enum SynthesizedEvent { Backspace, Utf16(u16) }
pub enum InjectProfile { Win32, Electron }
```

Mọi event `dwExtraInfo = OVK_EXTRA`.

- Win32: một `send` = all Backspace rồi all Utf16.
- Electron: hai `send` nếu cả hai phía non-empty; **không sleep**.
- `replace(0, "a")`: một batch unicode.
- Bọc `send`: `SendingGuard` trên `Arc<AtomicBool>` — `store(true)` trước `send`, `store(false)` trong `Drop` kể cả khi `send` lỗi / partial.

`electron_gap_ms` CLI default 0; > 0 parse rồi ignore (một `tracing`/`debug_assertions` warning tối đa, không in phím).

Partial `SendInput` (`n < events.len()`) → `Err(InjectError::Partial { sent: n, expected: events.len() })`. Không retry im lặng.

---

## 6. Session, persist, threading

- Keyboard callback: `try_lock` + `LabSession` + `SendInput` **đồng bộ**. Không `lock()`, không sleep, không serialize/persist.
- `notify()` sau khi **nhả** mutex.
- Một API snapshot:

```rust
pub struct SessionSaveSnapshot {
    pub model: AdaptiveModel,
    pub capture_records: Vec<CaptureRecord>,
    pub cursors: SessionCursors,
    pub last_at_ms: i64,
}

impl LabSession {
    pub fn save_snapshot(&self) -> SessionSaveSnapshot { /* clone only, no to_json */ }
}
```

Một paired debounced saver trên **saver thread**: một lần `save_snapshot()`, unlock, rồi `model.to_json_payload()`, SHA-256, dựng `CaptureLog { header, records }`, `to_payload()`, atomic pair recovery write. **Không** gọi `capture_log()` trên đường save (hàm đó đang `model_payload()` dưới ý lock). Windows development host writes open JSON under ADR 0008; core/lab encrypted stores remain unchanged.

- Discipline grep: `hook.rs`/`mouse.rs` cấm `lock(`, `sleep`, `model_payload`, `to_payload`. `try_lock` chỉ trong `host.rs`.
- P95: `tests/host_perf.rs` `#[ignore]` — không gate CI. Correctness không dùng `Instant`.

---

## 7. Overlay, tray, CLI, open development persistence

Overlay/tray như v1 (display-only; left click tray = toggle).

CLI:

```text
openvikey-win --method telex|vni --lexicon <path>
  [--model PATH] [--capture PATH] [--electron-gap-ms 0]
```

Default Windows development model/capture: `%LOCALAPPDATA%\OpenViKey\model.ovkdev.json` / `capture.ovkdev.json`; custom names must keep the `.ovkdev.json` suffix so Git ignores all plaintext sidecars.

Per ADR 0008, `openvikey-win` does not prompt for a passphrase and writes inspectable plaintext JSON with atomic `.bak` recovery. Existing encrypted `.ovk` stores are left untouched. This is a development policy, not a production security claim; core and `openvikey-lab` retain passphrase envelopes.

Release: cấm `println!`/`eprintln!` trong `hook.rs`, `inject.rs`, **`host.rs`**.

---

## 8. Học — giới hạn 2a

Mine implicit chỉ khi backspace token vừa commit **cùng HWND**. Click/nav → CaretBreak. Lexicon fixture 4 từ. Smoke: `xin` Space `chao` Space; Enter gửi prompt Cursor.

---

## 9. Test bắt buộc

| ID | Việc |
|---|---|
| P1–P5 | Tab/Esc Pass; letter eat; denylist; **OVK_EXTRA → Pass**; Ctrl+. accept; Ctrl+Z không undo |
| P6 | `VK_RETURN` → `CommitAndPass { '\\n' }` |
| P7 | `VK_LEFT` → `CaretBreakAndPass` |
| P8 | `is_sending` + extra≠OVK → `EatAndIgnore`; OVK lúc sending vẫn `Pass` |
| P9 | `VK_BACK` → `EatAndInject(Backspace)` |
| P10 | `Ctrl+C` → `Pass` |
| P11 | keyup chữ → `Pass` |
| P12 | Caps+`A` không shift → `'A'` |
| P13 | `Ctrl+,` Reject; `Ctrl+Shift+Z` Undo |
| S1 | Typed UpdateComposition `a`→`à` |
| S2 | Typed Commit space |
| S3 | auto `action` replacement, không `Commit.text` |
| S4 | caret-break không backspace |
| S5 | Accept composing |
| S6 | Enter: không AppendDelimiter |
| S7 | Commit `.` → `AppendDelimiter { '.' }` |
| S8 | Accept sau commit cùng HWND; khác HWND no-op visual |
| S9 | Undo visual |
| I1 | Win32 batch nội dung + keydown/keyup mapping test |
| I2 | Electron hai batch, không sleep |
| I3 | `SendInput` partial → `Partial` error; `AtomicBool` false sau lỗi |
| C1 | Cursor.exe Electron; notepad Win32 |
| H-enter | `handle_key` Enter không xen trước Replace (một call stack) |
| D1 | hook/mouse không `lock(`/`sleep` |
| D2 | `no_key_log` gồm `host.rs` |
| T-reg | `session_capture` + REPL compile |

Manual (bắt buộc trước 2b): Notepad Enter xuống dòng; Cursor Enter gửi **sau** chữ đã sửa; UniKey tắt.

---

## 10. Non-goals

Không đọc chat Agent. Không G3. Không TSF. Không port bait/leaked-key code GPL — chỉ `is_sending` + cấm sleep hook.
