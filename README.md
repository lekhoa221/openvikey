# OpenViKey

> **Bộ gõ tiếng Việt tự học cá nhân** — a self-learning Vietnamese input method.
> Open source (MIT), privacy-local, cross-platform. **Đang phát triển (WIP).**

## Vấn đề

Gõ tiếng Việt nhanh thì hay sai: đảo chữ (`khogn`), nuốt/đặt dấu sai chỗ (`ch2ao`), viết tắt (`ko`, `ntn`), quên bỏ dấu (`khong the nao`) — phải xoá đi gõ lại liên tục. Các bộ gõ hiện có bắt bạn *liệt kê thủ công* từng luật sửa và **không học** thói quen riêng của bạn.

## Ý tưởng khác biệt

Không hai người gõ giống nhau — như nét chữ tay. OpenViKey xây một **mô hình gõ chữ cá nhân hoá, tự học dần** theo cách gõ của *bạn*:

- **Tự học ngầm:** thấy bạn gõ sai rồi xoá, gõ lại đúng, nó ghi nhận cặp sửa đó — không cần khai báo.
- **Độ tự tin học được:** mỗi phép sửa "tốt nghiệp" từ *gợi ý* lên *tự thay* khi bạn liên tục chấp nhận, và bị hạ cấp khi bạn từ chối.
- **Riêng tư tuyệt đối:** không backend, telemetry hay automatic upload. Dữ liệu học chỉ nằm trên máy.

## Sáu năng lực

1. Engine gõ Telex/VNI.
2. Sửa lỗi gõ/đảo chữ (fuzzy): `khogn → không`.
3. Sửa telex/dấu sai vị trí: `ch2ao → chào`.
4. Viết tắt được gợi ý và có thể tự bung sau khi đủ bằng chứng cá nhân: `ko → không`. Cụm như `ntn → như thế nào` chỉ gợi ý (`Ctrl+.`), không tự bung.
5. Tự thêm dấu cho chữ không dấu: `khong the nao → không thể nào`.
6. Mô hình tự tin thích nghi theo từng người dùng.

(1) luôn inject khi gõ. (3) tự thay lúc Space, im lặng. Ở cold start, (2) và viết tắt một chữ (4) chỉ gợi ý; chúng chỉ được tự thay sau khi đủ bằng chứng cá nhân theo policy learning v2. Cụm viết tắt và (5) thiếu dấu luôn chỉ gợi ý. Chi tiết ở mục [Gợi ý vs tự thay](#gợi-ý-vs-tự-thay).

## Lộ trình

- **v1 — bộ não (đã chạy trong lab):** engine + 4 loại sửa + vòng tự học trong CLI. Gate chất lượng G3 (corpus ≥ 50.000 token đúng) **vẫn mở** vì chưa có corpus license sạch.
- **v2 — Windows standalone (đang làm, đã gõ được):** một `OpenViKey.exe` kiểu UniKey — tray, hook, `SendInput`, học và Settings trong cùng process; không TSF, không `Win + Space`.
- **v3 — macOS:** spike InputMethodKit (`IMKInputController`) so với CGEventTap rồi chọn adapter, tái dùng chung core.

## Kiến trúc (tóm tắt)

- `openvikey-core` (Rust thuần, không phụ thuộc OS): `types`, `engine`, `lexicon`, `generate`, `rank`, `model`, `decision`, `feedback`, `store`.
- `openvikey-lab`: harness để *nhìn bộ não hoạt động* + đo corpus/perf. Không phải IME hệ thống.
- `openvikey-win`: app standalone Windows — hook, injector, tray, overlay, Settings, learning host. TSF chỉ là research/compatibility optional.
- *(sau)* `openvikey-mac` — lớp mỏng bọc core.

Thiết kế Windows hiện hành: [`docs/superpowers/specs/2026-08-19-openvikey-windows-standalone-design.md`](docs/superpowers/specs/2026-08-19-openvikey-windows-standalone-design.md).

Thiết kế core: [`docs/superpowers/specs/2026-08-17-openvikey-design.md`](docs/superpowers/specs/2026-08-17-openvikey-design.md).

## `openvikey-win` — chạy bộ gõ Windows

Tắt UniKey / EVKey / bộ gõ hook khác trước khi chạy. OpenViKey **không** đăng ký TSF và **không** xuất hiện trong `Win + Space`.

```bash
cargo run -p openvikey-win -- --allow-terminal
```

Hoặc bản portable không console:

```powershell
powershell -File scripts/build-standalone-preview.ps1
# output: dist\OpenViKey-preview\OpenViKey.exe
```

Hướng dẫn nghiệm thu thủ công: [`STANDALONE-TEST-GUIDE.txt`](STANDALONE-TEST-GUIDE.txt).

Mặc định **VNI**. Đổi Telex/VNI từ tray hoặc cửa sổ Cài đặt; lựa chọn lưu ở `%LOCALAPPDATA%\OpenViKey\settings.json`. `--method telex|vni` chỉ là override lúc dev. Evidence học **theo kiểu gõ**: Telex không chuyển sang VNI.

Mở app thủ công sẽ hiện ngay cửa sổ Cài đặt/control; Start with Windows dùng chế độ nền. Click trái tray: đổi Vietnamese/English. Click phải hoặc double-click: mở lại Cài đặt. Đóng cửa sổ Cài đặt **không** thoát bộ gõ; chỉ *Thoát* mới tháo hook.

Optional: `--model` / `--capture` (mặc định `%LOCALAPPDATA%\OpenViKey\model.ovkdev.json` và `capture.ovkdev.json`). Tên file custom phải kết thúc `.ovkdev.json`. Host Windows development **không hỏi passphrase**; hai file này là JSON plaintext để soi learning state. Overlay nhớ trong `ui.ovkdev.json`. File `.ovk` mã hoá cũ không bị đụng.

> **Cảnh báo dev:** `model.ovkdev.json`, `capture.ovkdev.json` và sidecar recovery **không mã hoá**. Đừng chia sẻ. Terminal, denylist và English mode không học/capture. Chrome/Discord/Slack/Cursor **có thể học** khi đang gõ tiếng Việt.

`--allow-terminal` bật Pi/PowerShell/Windows Terminal nhưng tắt học/capture/persist ở đó. SSH trong terminal chưa detect được: chuyển OpenViKey sang English trước khi gõ secret. SSH client, password manager, `LogonUI.exe`, `CredentialUIBroker.exe` vẫn bị chặn.

Preview embed lexicon authored của project — **không** phải corpus G3. Exit từ tray hoặc **Ctrl+C** ở console dev: tháo hook rồi flush model/capture/settings.

### Gợi ý vs tự thay

Overlay góc màn hình hiện **một** gợi ý kèm `Ctrl+.`. Việc chữ trên Notepad/Cursor có đổi hay không phụ thuộc nguồn sửa:

| Bạn gõ (VNI) | Overlay | Chữ trong app sau Space |
|---|---|---|
| `ch2ao` + Space | không nháy capsule | **Tự thay** thành `chào` (TelexFix, im lặng) |
| `d91o` + Space | không nháy capsule | **Tự thay** thành `đó` |
| `ko` + Space | Cold start: gợi ý `không`; đã học đủ: *Đã sửa* | Cold start vẫn là `ko `; sau đủ bằng chứng có thể **tự thay** thành `không` |
| `khogn` + Space | Cold start: gợi ý `không`; đã học đủ: *Đã sửa* | Cold start vẫn là `khogn `; sau đủ bằng chứng có thể **tự thay** thành `không` |
| `ntn` + Space | gợi ý cụm / fuzzy | Vẫn là `ntn ` — không đoán `nên` hay bung cụm |
| `khong` + Space | Gợi ý dấu thanh | Vẫn là `khong ` — Diacritics **không** Auto |

Tự thay lúc Space **không** tính như `Ctrl+.` (không +1.0 điểm). Backspace **ngay sau Space** hoàn tác (`không` → lại `ko` đang soạn). `Ctrl+.` vẫn là nhận tường minh. `Ctrl+Shift+Z` hoàn tác Auto gần nhất.

### Phím

| Phím | Hành vi |
|---|---|
| Chữ / số / Backspace | Compose Telex hoặc VNI; inject vào app đang focus |
| Space / dấu câu | Commit. TelexFix an toàn tự replace; viết tắt/fuzzy chỉ tự replace sau khi đủ bằng chứng cá nhân |
| **Enter** | Commit + inject **rồi** trả Enter cho app |
| **Ctrl+.** | Nhận gợi ý trên cùng (khi chưa tự thay, hoặc muốn Accept mạnh) |
| **Ctrl+,** | Từ chối gợi ý |
| **Ctrl+Shift+Z** | Hoàn tác Auto gần nhất |
| **Ctrl+Shift+.** | Quên rule vừa học |
| Ctrl+C / Ctrl+V / Tab / Esc | Đi thẳng vào app |

### Checklist thủ công (không nằm trong CI)

- [ ] UniKey (và IME hook khác) tắt
- [ ] Notepad, VNI: `ch2ao` + Space thành `chào`; cold `ko` + Space chỉ gợi ý, `Ctrl+.` nhận thành `không`
- [ ] Notepad: Enter xuống dòng sau chữ Việt đã hiện
- [ ] Cursor: Enter gửi prompt **sau** chữ Việt đã inject
- [ ] Password/PIN: phím đi nguyên, không học

## Lab CLI (harness, không phải IME)

`openvikey-lab` **không** gửi phím vào ứng dụng khác.

### `type` — stdin JSONL

```bash
cargo run -p openvikey-lab -- type --method telex --lexicon data/fixtures/lexicon/authored.json
```

### `session` — REPL capture cá nhân (mã hoá)

```bash
cargo run -p openvikey-lab -- session --method telex --lexicon data/fixtures/lexicon/authored.json --model openvikey-model.ovk --capture openvikey-capture.ovk
```

Passphrase được hỏi (ẩn) trước raw mode. Space/Enter commit. Tab nhận gợi ý, Esc từ chối, Ctrl+Z undo Auto trong process này.

## Công cụ TSF (research, không phải product mặc định)

Inspector đọc cặp model/capture plaintext, không ghi file:

```bash
cargo run -p openvikey-win --bin openvikey-data-inspector -- --source fuzzy --original paht
```

DLL TSF x64 và `openvikey-tsf-register` chỉ dùng khi chủ động nghiên cứu compatibility. Host mặc định **không** đăng ký TSF. Xem [ADR 0010](docs/decisions/0010-windows-standalone-default.md).

## Bảo mật & riêng tư

Local-first, **zero backend**. Core và `openvikey-lab` hỗ trợ store mã hoá (DEK + XChaCha20-Poly1305, passphrase Argon2id). `openvikey-win` hiện dùng **JSON plaintext có chủ đích** (ADR 0008) để bỏ prompt lúc phát triển. Không có dữ liệu nào tự động rời máy. Storage production là gate riêng trước public release.

## License

[MIT](LICENSE) — mã nguồn mở hoàn toàn (OSI). Tác giả không thu phí và không thương mại hoá; MIT **không** hạn chế người khác (kể cả dùng thương mại).
