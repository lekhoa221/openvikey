# OpenViKey Windows standalone — product và architecture spec

- **Ngày:** 2026-08-19
- **Trạng thái:** v1 — product direction accepted; implementation pending
- **Owner requirement:** một ứng dụng standalone kiểu UniKey
- **Survey:** [`2026-08-19-openvikey-windows-standalone-survey.md`](./2026-08-19-openvikey-windows-standalone-survey.md)
- **ADR:** [`../../decisions/0010-windows-standalone-default.md`](../../decisions/0010-windows-standalone-default.md)

---

## 0. Tóm tắt quyết định

Sản phẩm Windows chính là **một executable `OpenViKey.exe` chạy nền**. Executable này sở hữu tray, keyboard/mouse hooks, context safety, `SendInput`, session, learning, settings và persistence.

Default product path:

- không đăng ký Windows TSF input profile;
- không xuất hiện trong `Win + Space`;
- không cài COM DLL vào process khác;
- không cần quyền Administrator để chạy;
- không phụ thuộc một context publisher bên ngoài;
- chỉ transform và học khi `OpenViKey.exe` đang chạy.

TSF code đã làm ở GĐ2b được giữ làm research/optional compatibility component. GĐ2c TSF-primary dừng và không nằm trên release critical path.

---

## 1. Product contract

### 1.1 Trải nghiệm bắt buộc

1. Người dùng double-click `OpenViKey.exe`.
2. App chạy nền, không hiện console, và có icon V/E dưới system tray.
3. Left-click tray hoặc hotkey đổi Vietnamese/English.
4. Right-click tray mở Settings, bật/tắt Suggestions, Start with Windows và Exit.
5. Khi Vietnamese bật, hook xử lý Telex/VNI rồi inject Unicode vào app đang focus.
6. Khi English bật hoặc OpenViKey đã Exit, phím đi nguyên trạng và không có learning/capture.
7. Settings đóng không dừng host. Chỉ Exit mới tháo hooks và kết thúc process.
8. Chạy lần hai không tạo hook thứ hai; instance đang chạy mở Settings.
9. Restart app giữ method, mode, suggestion preference và learned model.
10. Không có bước chọn OpenViKey từ Windows language switcher.

### 1.2 “Standalone” nghĩa là gì

Một portable preview có thể gồm executable và asset cạnh nó, nhưng runtime chỉ có **một process product** và không cần OS registration. Mục tiêu đóng gói cuối là một executable với lexicon/resource nhúng.

Không coi các thành phần sau là standalone product:

- `openvikey-lab` CLI;
- `openvikey-context-probe`;
- `openvikey-tsf-register`;
- một TSF profile được chọn trong `Win + Space`;
- một DLL chỉ hoạt động khi Windows load vào ứng dụng khác.

---

## 2. Mục tiêu và non-goals

### 2.1 Mục tiêu

- Daily-driver Windows kiểu UniKey cho Win32, WPF, Electron/Chromium và các app thường dùng.
- Một runtime duy nhất cho typing và learning; không duplicate session state.
- Password/sensitive field pass-through trước khi hook ăn phím.
- Learning observable: người dùng biết app đã ghi nhận rule nào và có thể quên rule.
- Local-only, không backend, telemetry hay automatic upload.
- Startup/exit/restart đáng tin cậy; không double hook, không mất coherent model/capture pair.
- TSF hoàn toàn optional và không ảnh hưởng app khi chưa bật.

### 2.2 Non-goals của standalone v1

- GĐ2c TSF composition/primary input.
- Hỗ trợ process chạy quyền cao hơn host; app không tự elevation.
- Macro/clipboard automation.
- Cloud account, sync hoặc telemetry.
- Production corpus/lexicon G3.
- Code signing và installer trước portable preview.
- Đọc toàn bộ document của Word/browser.
- Bảo đảm hoạt động trong mọi game/anti-cheat.

---

## 3. Kiến trúc

```text
                         ┌─────────────────────────────┐
physical keyboard/mouse ─► OpenViKey.exe               │
                         │                             │
                         │ UI/message thread           │
                         │  ├─ tray V/E                │
                         │  ├─ settings window         │
                         │  └─ suggestion/learn notice │
                         │                             │
                         │ input runtime               │
                         │  ├─ WH_KEYBOARD_LL          │
                         │  ├─ WH_MOUSE_LL             │
                         │  ├─ focus/context cache     │
                         │  └─ SendInput               │
                         │                             │
                         │ openvikey-session           │
                         │  ├─ composition/correction  │
                         │  ├─ learning/capture        │
                         │  └─ model                   │
                         │                             │
                         │ persistence worker          │
                         └──────────────┬──────────────┘
                                        │
                                        ▼
                              %LOCALAPPDATA%\OpenViKey
```

### 3.1 Crate ownership

| Crate | Trách nhiệm |
|---|---|
| `openvikey-core` | Engine, candidate generation/rank, adaptive model; không OS/unsafe |
| `openvikey-session` | Composition/document reducer, correction, feedback, capture/replay |
| `openvikey-win` | Product executable, hooks, injector, context guard, tray/settings/overlay, persistence |
| `openvikey-win-context` | Pure identity/context contracts dùng lại được; không buộc TSF |
| `openvikey-win-tsf` | Optional research/compatibility artifact; không nằm trong package mặc định |

`openvikey-win` không được tạo một engine/session riêng cho settings. UI đọc và gửi command đến runtime đang sở hữu session.

---

## 4. Process lifecycle

### 4.1 Startup ordering

1. Acquire per-user single-instance mutex.
2. Nếu mutex đã tồn tại: gửi `OpenSettings` đến message-only window của instance trước và exit.
3. Load/validate settings.
4. Load embedded/packaged lexicon.
5. Load coherent model/capture pair.
6. Khởi tạo session, saver và lock-free focus/context caches.
7. Start context worker và seed foreground/field verdict.
8. Tạo message-only window, tray, settings shell và overlay.
9. Cài focus/mouse hooks.
10. Chỉ cài keyboard hook sau khi context guard đã sẵn sàng.
11. Chạy Win32 message loop.

Nếu lỗi trước bước 10, app hiện một dialog không chứa typed data rồi exit; không để hook nửa sống.

### 4.2 Shutdown ordering

1. Đánh dấu shutdown và ngừng nhận UI commands mới.
2. Tháo keyboard hook trước.
3. Tháo mouse/focus hooks và dừng context worker.
4. Ẩn overlay và xóa tray icon.
5. Flush một coherent model/capture snapshot.
6. Lưu settings dirty.
7. Release single-instance mutex và exit.

Console close handler chỉ phục vụ development build. Product build dùng tray Exit, session shutdown và Windows end-session notifications.

### 4.3 Không console

Product target dùng Windows GUI subsystem. Lỗi startup đi qua dialog/tray notification và local diagnostic code; không log raw keys/tokens ra console.

---

## 5. Input pipeline

### 5.1 Đường gõ mặc định

```text
WH_KEYBOARD_LL
  → đọc Mode + foreground + sensitive verdict lock-free
  → policy Eat/Pass/Hotkey
  → session try_lock
  → openvikey-session inject
  → semantic Replace/Append commands
  → SendInput với OVK_EXTRA
  → own injected events Pass
```

Các nguyên tắc hiện hành được giữ:

- callback không sleep, I/O, serialize hoặc blocking `Mutex::lock`;
- `try_lock` fail thì fail-open đối với phím thường;
- own `SendInput` events luôn Pass;
- Tab/Esc và shortcut không thuộc OpenViKey luôn vào app;
- focus/caret break xóa composition stale mà không backspace cửa sổ mới;
- hai bộ gõ hook cùng chạy là unsupported.

### 5.2 Không có TSF publisher

Không có context provider bên ngoài là trạng thái bình thường, không phải lỗi.

- normal field đã được standalone detector xác nhận → transform;
- surrounding text không có → dùng session context nội bộ và `left_token=None` khi cần;
- không được map “TSF absent” thành `ContextState::Unavailable` rồi Pass mọi phím;
- named-pipe TSF bridge không start trong default build/config.

Đây là regression gate bắt buộc vì working tree GĐ2b hiện chưa đáp ứng contract này.

### 5.3 Injection profiles

Giữ `Win32` và `Electron` profiles. Profile chỉ thay cách batch `SendInput`/intervention, không thay security policy. Per-app compatibility config tương lai chỉ chọn hook behavior, không chọn TSF trong v1.

---

## 6. Sensitive-field guard không TSF

### 6.1 Mục tiêu

Trước khi ăn một physical letter/digit/backspace trong field nhạy cảm, hook phải thấy verdict `Sensitive` hoặc chưa xác định và trả `Pass`. Sensitive field không transform, không surrounding-text read, không learning và không capture.

### 6.2 Nguồn phân loại

Theo thứ tự:

1. executable denylist: password managers, logon/credential UI, SSH clients;
2. Win32 child control style `ES_PASSWORD`;
3. UI Automation focused element `CurrentIsPassword`;
4. app/user policy block;
5. explicit normal verdict từ UIA/Win32 capability;
6. unsupported custom surface → fail-safe mặc định, người dùng có thể opt in per app sau.

Không dùng text/value pattern để quyết định password. `CurrentIsPassword` và style query phải chạy trước mọi optional context read.

### 6.3 Verdict state machine

```text
Window/field focus changed
        │
        ▼
     Pending ──────────────► Sensitive
        │                       │
        ├──────────────────► Normal
        │
        └──────────────────► Unsupported
```

Policy:

| Verdict | Transform | Learning/capture | Context text read |
|---|---:|---:|---:|
| `Pending` | No | No | No |
| `Sensitive` | No | No | No |
| `Normal` | Yes | Yes nếu policy app cho phép | Optional |
| `Unsupported` | No mặc định | No | No |
| `NormalOptIn` | Yes | theo explicit app policy | Không nếu provider không có |

### 6.4 Identity và race control

Verdict phải gắn với:

```text
PID + top-level HWND + focused-element identity + focus generation
```

- `EVENT_SYSTEM_FOREGROUND` tăng window generation.
- `EVENT_OBJECT_FOCUS` tăng field generation ngay cả khi top-level HWND không đổi.
- Mouse button down, Tab/Shift+Tab và navigation invalidate verdict thành `Pending` trước input có thể thuộc field mới.
- Worker publish bằng latest-value bounded slot; hook chỉ nhận snapshot identity khớp.
- Snapshot cũ không được vượt app switch, child-field switch hoặc reconnect.

UI Automation COM chạy trên worker phù hợp apartment model, không trong LL hook callback.

### 6.5 Optional normal context

Sau verdict `Normal`, worker có thể đọc một token trái bằng UIA TextPattern nếu app hỗ trợ. Đây là enhancement, không là dependency:

- giới hạn một Unicode token và kích thước bounded;
- normalize NFC;
- lỗi đọc chỉ bỏ external left context, không vô hiệu hóa typing;
- không cache text qua focus generation;
- không bao giờ gọi ở `Pending`, `Sensitive` hoặc `Unsupported`.

---

## 7. Learning contract và UX

### 7.1 Khi nào learning chạy

Learning chỉ chạy khi tất cả đúng:

- `OpenViKey.exe` đang chạy;
- mode Vietnamese;
- verdict `Normal` hoặc explicit safe opt-in;
- app không thuộc terminal/denylist/no-learning policy;
- injection thành công;
- session event cho phép learning.

English mode, sensitive/unknown field, terminal và denied app không mutate model/capture.

### 7.2 Cơ chế hiện có

- Explicit accept: `Ctrl+.`.
- Explicit reject: `Ctrl+,`.
- Natural composition rewind: gõ sai, Backspace trong composition, gõ lại, commit.
- Candidate-matched rewrite tạo implicit evidence.
- Unmatched rewrite có thể tạo personal pair; phải lặp lại trước khi promote.
- Telex/VNI-form fix đủ an toàn có policy auto tại boundary.
- Learned Auto cần evidence/confidence gate; một typo không lập tức thành auto rule.

### 7.3 Observable learning events

Session/host phải phát structured UI event, không phải raw logging:

```text
SuggestionShown
RuleEvidenceAdded { source, original, replacement, evidence, state }
PersonalPairObserved { original, replacement, count, promoted }
RulePromoted { from, to }
CorrectionApplied { mode, original, replacement, undo_available }
RuleForgotten
```

Typed text chỉ hiện trong local UI theo hành động người dùng; không console, telemetry hay network.

Overlay cần phân biệt:

- “Gợi ý: …”;
- “Đã ghi nhận sửa: X → Y”;
- “Đã học gợi ý: X → Y”;
- “Đã sửa: X → Y · Backspace/Ctrl+Shift+Z để hoàn tác”.

### 7.4 Learned-rules UI

Settings cung cấp bảng local:

- original;
- replacement;
- source;
- input method;
- evidence/confidence;
- state `Observed`/`Suggest`/`Auto`;
- last-used/last-feedback khi schema có;
- Forget selected và Forget last.

Delete phải đi qua versioned model command API và coherent saver; UI không sửa JSON trực tiếp.

### 7.5 Learning acceptance flow

Manual gate tối thiểu:

1. Start một standalone instance trên user profile không có OpenViKey TSF.
2. Gõ một correction tự nhiên đủ hai lần trong Notepad normal field.
3. Learned-rules UI cho thấy observed/promoted pair.
4. Exit graceful và start lại.
5. Rule vẫn tồn tại và candidate xuất hiện đúng.
6. Accept rule làm evidence tăng.
7. Lặp flow trong password: visible input giữ nguyên, không có event/model/capture mutation.

---

## 8. Settings contract

### 8.1 File

`%LOCALAPPDATA%\OpenViKey\settings.json`

Settings không chứa typed history. Payload versioned và atomic-replace.

### 8.2 Schema v1

```text
SettingsV1 {
  version,
  input_method: Telex | Vni,
  tone_placement: Modern | Traditional,
  mode_on_start: Viet | English | RestoreLast,
  show_suggestions,
  start_with_windows,
  hotkeys,
  app_policies[]
}

AppPolicyV1 {
  executable,
  transform: Default | Allow | Block,
  learning: Default | Allow | Block,
  inject_profile: Auto | Win32 | Electron
}
```

Không có TSF routing field trong schema standalone v1.

### 8.3 Validation

- unknown version fail-safe với dialog; không overwrite tự động;
- hotkey conflict bị từ chối trước save;
- executable key canonicalize theo basename case-insensitive cho v1;
- explicit Block thắng Allow;
- user không thể bật learning cho denylisted credential process;
- autostart mutation chỉ xảy ra khi người dùng đổi setting tương ứng.

### 8.4 Live apply

Method/tone change tạo caret break, reset composition rồi swap config. Mode/suggestion có thể apply ngay. Settings command được serialize qua UI/runtime coordinator, không trực tiếp mutate `TypingHost` từ nhiều thread.

---

## 9. Windows UI

### 9.1 Tray

- Product icon thể hiện V/E rõ ràng, tooltip `OpenViKey — Vietnamese/English`.
- Left-click: toggle V/E.
- Double-click hoặc menu Settings: mở control window.
- Menu: Vietnamese, Suggestions, Settings…, Start with Windows, Exit.

### 9.2 Settings window

Native Windows window trong cùng process là lựa chọn mặc định để tránh web runtime/second backend. Minimum pages:

1. **General:** Telex/VNI, tone placement, startup mode, suggestions.
2. **Hotkeys:** toggle, accept, reject, undo, forget-last.
3. **Learning:** rule table, filter, Forget.
4. **Applications:** Allow/Block/inject profile.
5. **About/Diagnostics:** version, data folder, local-only statement, no typed-data logs.

Closing window hides it; process và tray tiếp tục chạy.

---

## 10. Persistence và privacy

### 10.1 Preview

ADR 0008 plaintext `.ovkdev.json` có thể tiếp tục cho development preview, nhưng UI/About phải ghi rõ đây là inspectable local development data.

### 10.2 Production gate

Trước public release, chọn và triển khai storage không tương tác:

- OS-backed key protection hoặc explicit user-selected encrypted mode;
- không hard-code key;
- migration/recovery của coherent pair;
- export chỉ do user chủ động;
- không automatic upload.

### 10.3 Data boundaries

- Settings tách model/capture.
- Diagnostic logs không chứa raw keys, tokens, suggestions hoặc model rows.
- Data folder action mở Explorer; không upload.
- Password/sensitive test phải chứng minh byte-for-byte model/capture không đổi.

---

## 11. Packaging

### 11.1 Portable preview gate

- `OpenViKey.exe` product target, Windows GUI subsystem.
- Embedded development lexicon được label Preview, hoặc packaged artifact cạnh exe.
- Product icon, version metadata và manifest `asInvoker`.
- Không yêu cầu admin.
- Không chứa/register `openvikey_win_tsf.dll` hoặc TSF helper.
- ZIP giải nén và chạy được; settings/data ở `%LOCALAPPDATA%`.

### 11.2 Installer/signing

Làm sau portable gate:

- per-user install mặc định;
- Start Menu shortcut;
- autostart opt-in;
- uninstall không xóa model nếu user chưa chọn;
- Authenticode/signing và AV false-positive validation.

Không bundle TSF registration trong standard installer.

---

## 12. TSF optional boundary

`openvikey-win-tsf` được giữ trong workspace nhưng:

- không build/package bởi standard product command;
- không start bridge trong default executable;
- registration chỉ qua explicit developer/compatibility action;
- optional component phải reversible và có status/cleanup;
- standalone acceptance chạy trên profile chưa từng đăng ký OpenViKey TSF;
- không mở lại TSF-primary nếu chưa có một compatibility case thực tế và spec riêng.

TSF không được sở hữu model, session, settings hoặc learning riêng.

---

## 13. Test strategy

### 13.1 Automated

- No-provider normal field transforms; absence of TSF is not `Unavailable`.
- Sensitive verdict has precedence before transform/session/text read.
- Field generation rejects stale normal verdict.
- Tab/click/focus invalidates verdict before next field input.
- Unsupported defaults Pass; explicit safe app opt-in behavior tested.
- Hook remains no blocking lock/I/O/sleep/log.
- Single-instance command routing.
- Settings schema round-trip, invalid version and hotkey conflict.
- Runtime method change resets composition safely.
- Learning UI event corresponds to real model mutation only.
- Sensitive/English/terminal model and capture remain byte-identical.
- Graceful exit flushes one coherent pair.
- Default product dependency/package check excludes TSF registration artifacts.

### 13.2 Manual compatibility matrix

| Surface | Normal field | Password/PIN | Learning |
|---|---|---|---|
| Notepad/Win32 | type + correct | pass raw | normal only |
| WinForms | type + correct | pass raw | normal only |
| WPF | type + correct | pass raw | normal only |
| WinUI sample | type + correct | pass raw | normal only |
| Edge | type + correct | pass raw | normal only |
| Chrome | type + correct | pass raw | normal only |
| Cursor composer | type + Enter | n/a | according app policy |
| Windows Terminal | off by default | pass raw | never |
| Password manager/credential UI | blocked | blocked | never |

### 13.3 Clean-machine gate

Before declaring standalone ready:

```text
OpenViKey COM class absent
OpenViKey TSF profiles absent
OpenViKey absent from Get-WinUserLanguageList
active keyboard = ordinary Windows layout
```

Then run `OpenViKey.exe` and prove typing, password safety, learning persistence and Exit without registration changes.

---

## 14. Performance and safety budgets

- Host startup target < 300 ms excluding first-run OS delays.
- `TypingHost::handle_key` P95 < 15 ms under current fixture/stress policy.
- LL hook callback has no blocking lock, disk/network I/O, sleep or serialization.
- Context classification asynchronous and bounded latest-value.
- Focus-to-verdict should normally settle before next printable key; until then policy Passes.
- `SendInput` partial failure rolls session/model/capture checkpoint back.
- No hidden elevation or injection into higher-integrity processes.

---

## 15. Implementation slices

### S1 — standalone runtime independence

- Make TSF bridge optional/off by default.
- Replace TSF-required policy with standalone context verdict.
- Add no-provider regression tests.
- Gate: normal Notepad typing with no registration.

### S2 — sensitive guard

- Add field-focus generation and bounded UIA/Win32 classifier.
- Add password matrix and zero-mutation tests.
- Gate: browser/WPF password raw input, normal field transforms.

### S3 — settings and observable learning

- Versioned settings coordinator.
- Learning event API and learned-rule query/forget commands.
- Persist method/mode/suggestions.
- Gate: natural correction → UI evidence → restart persistence.

### S4 — UniKey-style product shell

- Settings window, tray menu, single-instance and autostart.
- GUI subsystem and actionable startup errors.
- Gate: one process, close-settings-keeps-running, Exit flushes.

### S5 — portable preview

- Embedded/packaged lexicon, icon/version/manifest, ZIP build.
- Clean-profile acceptance without TSF artifacts.
- Production storage, corpus, installer and signing remain later release gates.

Mỗi slice cần implementation plan TDD riêng; không code S2–S5 trong một big-bang patch.

---

## 16. Definition of done

Standalone Windows preview chỉ được coi là hoàn tất khi:

1. một `OpenViKey.exe` chạy không console và không admin;
2. không có OpenViKey TSF/COM/language registration trước hoặc sau khi chạy;
3. tray V/E, settings và single-instance hoạt động;
4. normal fields gõ Telex/VNI qua hook + `SendInput`;
5. password/PIN/denylist pass raw và không mutate data;
6. learning thực sự ghi evidence, được nhìn thấy, quên được và tồn tại qua restart;
7. method/mode/suggestions/autostart round-trip qua settings;
8. Exit tháo hook và flush coherent data;
9. automated gates xanh và manual matrix đạt mức preview đã công bố;
10. package không chứa TSF helper/DLL mặc định.
