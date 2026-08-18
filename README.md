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
cargo run -p openvikey-win -- --method telex --allow-terminal
```

Optional: `--model` / `--capture` (default `%LOCALAPPDATA%\OpenViKey\model.ovk` and `capture.ovk`), `--electron-gap-ms` for Electron SendInput spacing. Passphrase is read hidden on the console before the message loop.

Debug builds embed the project-authored development lexicon; it is **not release-quality corpus evidence**. Release builds fail closed and require an explicit `--lexicon` until G3 closes. `--allow-terminal` explicitly enables Pi/PowerShell/Windows Terminal while disabling learning, capture, and persistence there. SSH running inside a terminal cannot yet be detected in GĐ2a: toggle OpenViKey to English before entering terminal secrets. Direct SSH clients, password managers, `LogonUI.exe`, and `CredentialUIBroker.exe` remain blocked.

Suggestion smoke: type `ko` and confirm the overlay shows `không`, then press **Ctrl+.** to replace it and append a space. Type `khogn` to confirm `không` appears as a fuzzy correction candidate.

| Key | Behavior |
|---|---|
| Letters / Backspace | Telex compose; inject into the focused app |
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

## Bảo mật & riêng tư

Local-first, **zero backend**. Mô hình lưu trong container mã hoá (envelope: DEK + XChaCha20-Poly1305, DEK bọc bởi OS keychain và/hoặc passphrase Argon2id). **Không tự động gửi plaintext hay khoá đi đâu** — chỉ *ciphertext do bạn chủ động export* mới có thể rời máy. Đồng bộ đa máy (mang blob mã hoá đi) là tính năng *dự kiến*; v1 chạy single-device.

## License

[MIT](LICENSE) — mã nguồn mở hoàn toàn (OSI). Tác giả không thu phí và không thương mại hoá; MIT **không** hạn chế người khác (kể cả dùng thương mại).
