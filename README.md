# OpenViKey

> **Bộ gõ tiếng Việt tự học cá nhân** — a self-learning Vietnamese input method.
> Open source (MIT), privacy-local, cross-platform. **Đang phát triển giai đoạn đầu (WIP).**

## Vấn đề

Gõ tiếng Việt nhanh thì hay sai: đảo chữ (`khọgn`), nuốt/đặt dấu sai chỗ (`ch2ao`), viết tắt (`ko`, `ntn`), quên bỏ dấu (`khong the nao`) — phải xoá đi gõ lại liên tục. Các bộ gõ hiện có bắt bạn *liệt kê thủ công* từng luật sửa và **không học** thói quen riêng của bạn.

## Ý tưởng khác biệt

Không hai người gõ giống nhau — như nét chữ tay. OpenViKey xây một **mô hình gõ chữ cá nhân hoá, tự học dần** theo cách gõ của *bạn*:

- **Tự học ngầm:** thấy bạn gõ `teh` → xoá → gõ `the`, nó ghi nhận `teh→the` là phép sửa của bạn — không cần khai báo.
- **Độ tự tin học được:** mỗi phép sửa "tốt nghiệp" từ *gợi ý* lên *tự thay im lặng* khi bạn liên tục chấp nhận, và bị hạ cấp khi bạn từ chối. "Chắc chắn" là thứ đo được, không phải luật chết.
- **Riêng tư tuyệt đối:** mô hình cá nhân được coi như dữ liệu định danh — lưu mã hoá local, không backend/telemetry/automatic upload; chỉ ciphertext do bạn chủ động export mới có thể rời máy.

## Sáu năng lực (bức tranh đầy đủ)

1. Engine gõ Telex/VNI.
2. Sửa lỗi gõ/đảo chữ (fuzzy): `khọgn → không`.
3. Sửa telex/dấu sai vị trí: `ch2ao → chào`.
4. Viết tắt tự bung: `ko → không`, `ntn → như thế nào`.
5. Tự thêm dấu cho chữ không dấu: `khong the nao → không thể nào`.
6. Mô hình tự tin thích nghi theo từng người dùng.

## Lộ trình

- **v1 (đang làm) — "chứng minh bộ não":** engine + 4 loại sửa + vòng tự học chạy trong CLI harness thử nghiệm, **chưa** hook hệ thống. Mục tiêu: chứng minh phần khó nhất trước.
- **v2 — Windows:** tích hợp toàn hệ thống qua TSF (Text Services Framework).
- **v3 — macOS:** spike InputMethodKit (`IMKInputController`) so với CGEventTap rồi chọn adapter, tái dùng chung core.

## Kiến trúc (tóm tắt)

- `openvikey-core` (Rust thuần, không phụ thuộc OS): `types`, `engine`, `lexicon`, `generate`, `rank`, `model`, `decision`, `feedback`, `store`.
- `openvikey-lab`: harness v1 để *nhìn bộ não hoạt động* + test-runner đo độ chính xác.
- *(sau)* `openvikey-win` (TSF), `openvikey-mac` (InputMethodKit/CGEventTap — chờ spike) — lớp mỏng bọc core.

Thiết kế chi tiết: [`docs/superpowers/specs/2026-08-17-openvikey-design.md`](docs/superpowers/specs/2026-08-17-openvikey-design.md).

Kế hoạch triển khai v1: [`docs/superpowers/plans/2026-08-17-openvikey-v1-implementation-plan.md`](docs/superpowers/plans/2026-08-17-openvikey-v1-implementation-plan.md).

## Lab CLI (harness, not a system IME)

`openvikey-lab` is a **local developer/lab harness**. It is **not** a system input method: there is **no tray icon**, **no UniKey/OpenKey window**, and **no TSF/OS keyboard hook**. Typing here does not send keys into other applications.

### `type` — stdin JSONL (unchanged)

```bash
cargo run -p openvikey-lab -- type --method telex --lexicon data/fixtures/lexicon/authored.json
```

Each stdin character becomes a live engine observation on stdout (JSONL).

### `session` — personal capture REPL

Interactive one-line raw-mode session. The passphrase is prompted (hidden) **before** raw mode. The personal model and capture log are stored encrypted; they are not a substitute for OS-level IME integration.

```bash
cargo run -p openvikey-lab -- session --method telex --lexicon data/fixtures/lexicon/authored.json --model openvikey-model.ovk --capture openvikey-capture.ovk
```

Requires a real terminal (piped stdin exits with an error mentioning `terminal`). Space and Enter both commit with a space. Tab accepts the top suggestion, Esc rejects it, Ctrl+Z undoes the last Auto edit **in this process**, Ctrl+C / Ctrl+D flush both encrypted files and quit.

## `openvikey-win` runbook (GĐ2a hook host)

Windows system hook host (Notepad + Cursor/Electron). **Turn UniKey / other IMEs off** before running — only one keyboard filter should own the keys.

```bash
cargo run -p openvikey-win -- --allow-terminal
```

Default input method is **VNI**. Pass `--method telex` only when you want Telex. Learning keys are per-method: Telex evidence does not transfer to VNI.

Optional: `--model` / `--capture` (default `%LOCALAPPDATA%\OpenViKey\model.ovkdev.json` and `capture.ovkdev.json`), `--electron-gap-ms` for Electron SendInput spacing. Custom store names must end in `.ovkdev.json` so Git ignores the plaintext and all recovery sidecars. The Windows development host starts without a passphrase and stores these two files as inspectable plaintext JSON. Overlay visibility is remembered in `ui.ovkdev.json` (a boolean, not typing data). Existing encrypted `.ovk` files are left untouched.

> **Development privacy warning:** `model.ovkdev.json`, `capture.ovkdev.json`, and their recovery sidecars are not encrypted. Do not share them or use this open-storage mode with sensitive text. Terminal, denylist, and English-mode learning/capture stay disabled. Electron apps (Chrome/Discord/Slack) may learn and capture when transform is on.

Exit from the tray or press **Ctrl+C** in the launching console for a graceful shutdown: input hooks stop first, then the coherent model/capture pair is flushed.

Debug builds embed the project-authored development lexicon; it is **not release-quality corpus evidence**. Release builds fail closed and require an explicit `--lexicon` until G3 closes. `--allow-terminal` explicitly enables Pi/PowerShell/Windows Terminal while disabling learning, capture, and persistence there. SSH running inside a terminal cannot yet be detected in GĐ2a: toggle OpenViKey to English before entering terminal secrets. Direct SSH clients, password managers, `LogonUI.exe`, and `CredentialUIBroker.exe` remain blocked.

Suggestion smoke (VNI): type `ch2ao` then Space and confirm the host replaces it with `chào`. Type `ko` and press **Ctrl+.** to accept `không` (this writes VNI learning mass). Type `khogn` to confirm `không` appears as a fuzzy candidate. Right-click the tray icon and uncheck **Gợi ý** to hide the overlay (Accept/Reject still work; the preference is remembered). Left-click the tray icon still toggles Vietnamese/English.

| Key | Behavior |
|---|---|
| Letters / digits / Backspace | VNI compose; inject into the focused app |
| Space / punctuation | Commit |
| **Enter** | Commit + inject **then** pass to the app (newline / Cursor submit) |
| **Ctrl+.** | Accept top suggestion |
| **Ctrl+,** | Reject suggestion |
| **Ctrl+Shift+Z** | Undo last Auto edit |
| Ctrl+C / Ctrl+V / … | Pass through (not OpenViKey hotkeys) |

### Manual checklist (not CI)

- [ ] UniKey (and other IMEs) off
- [ ] Notepad: Enter inserts a newline after Vietnamese is visible
- [ ] Cursor: Enter submits the prompt **after** visible Vietnamese (post-inject)

## GĐ2b development tools

Inspect the plaintext development model/capture pair without opening a saver or changing either file:

```bash
cargo run -p openvikey-win --bin openvikey-data-inspector -- --source fuzzy --original paht
```

The inspector uses the host's model/capture schema and provenance validator. `--model` and `--capture` accept explicit `.ovkdev.json` paths; optional filters are `--original`, `--candidate`, `--source`, and `--left-token`. Run the command again for a manual refresh. Missing, invalid, or hash-mismatched pairs fail closed. The tool has no write, repair, delete, import, export, or settings commands.

The x64 TSF DLL and its registration helper are development artifacts; x86 processes are unsupported in the current GĐ2b development contract. Registration is explicit and requires an elevated terminal; host startup never changes TSF registration. `status` reports the active keyboard profile and OpenViKey's two development profiles. On the review machine, programmatic `activate` was visible only inside the helper process and did not replace desktop selection; choose OpenViKey from the Windows language switcher for a real-app smoke. Always return the test machine to its prior input profile and run `deactivate` afterward.

```powershell
cargo build -p openvikey-win-tsf
target\debug\openvikey-tsf-register.exe register (Resolve-Path target\debug\openvikey_win_tsf.dll)
target\debug\openvikey-tsf-register.exe status
target\debug\openvikey-tsf-register.exe activate
target\debug\openvikey-tsf-register.exe deactivate
```

GĐ2b remains a hybrid: the GĐ2a hook owns key input; TSF only supplies read-only InputScope/left-context snapshots. The UniKey/VNIKey-style settings window is still GĐ2d, after GĐ2c stabilizes per-app TSF input behavior.

## Bảo mật & riêng tư

Local-first, **zero backend**. Core và `openvikey-lab` vẫn hỗ trợ container mã hoá (envelope: DEK + XChaCha20-Poly1305, DEK bọc bởi passphrase Argon2id). Riêng `openvikey-win` hiện dùng **plaintext JSON có chủ đích trong giai đoạn phát triển** để bỏ prompt và dễ kiểm tra learning state; xem cảnh báo phía trên. Không có dữ liệu nào tự động rời máy.

## License

[MIT](LICENSE) — mã nguồn mở hoàn toàn (OSI). Tác giả không thu phí và không thương mại hoá; MIT **không** hạn chế người khác (kể cả dùng thương mại).
