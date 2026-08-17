# OpenViKey

> **Bộ gõ tiếng Việt tự học cá nhân** — a self-learning Vietnamese input method.
> Open source (MIT), privacy-local, cross-platform. **Đang phát triển giai đoạn đầu (WIP).**

## Vấn đề

Gõ tiếng Việt nhanh thì hay sai: đảo chữ (`khọgn`), nuốt/đặt dấu sai chỗ (`ch2ao`), viết tắt (`ko`, `ntn`), quên bỏ dấu (`khong the nao`) — phải xoá đi gõ lại liên tục. Các bộ gõ hiện có bắt bạn *liệt kê thủ công* từng luật sửa và **không học** thói quen riêng của bạn.

## Ý tưởng khác biệt

Không hai người gõ giống nhau — như nét chữ tay. OpenViKey xây một **mô hình gõ chữ cá nhân hoá, tự học dần** theo cách gõ của *bạn*:

- **Tự học ngầm:** thấy bạn gõ `teh` → xoá → gõ `the`, nó ghi nhận `teh→the` là phép sửa của bạn — không cần khai báo.
- **Độ tự tin học được:** mỗi phép sửa "tốt nghiệp" từ *gợi ý* lên *tự thay im lặng* khi bạn liên tục chấp nhận, và bị hạ cấp khi bạn từ chối. "Chắc chắn" là thứ đo được, không phải luật chết.
- **Riêng tư tuyệt đối:** mô hình cá nhân được coi như dữ liệu định danh — lưu **mã hoá, 100% trên máy bạn**, không server, không cloud, không telemetry.

## Sáu năng lực (bức tranh đầy đủ)

1. Engine gõ Telex/VNI.
2. Sửa lỗi gõ/đảo chữ (fuzzy): `khọgn → không`.
3. Sửa telex/dấu sai vị trí: `ch2ao → chào`.
4. Viết tắt tự bung: `ko → không`, `ntn → như thế nào`.
5. Tự thêm dấu cho chữ không dấu: `khong the nao → không thể nào`.
6. Mô hình tự tin thích nghi theo từng người dùng.

## Lộ trình

- **v1 (đang làm) — "chứng minh bộ não":** engine + 4 loại sửa + vòng tự học chạy trong harness thử nghiệm (CLI/TUI), **chưa** hook hệ thống. Mục tiêu: chứng minh phần khó nhất trước.
- **v2 — Windows:** tích hợp toàn hệ thống qua TSF (Text Services Framework).
- **v3 — macOS:** qua CGEventTap, tái dùng chung core.

## Kiến trúc (tóm tắt)

- `openvikey-core` (Rust thuần, không phụ thuộc OS): `engine`, `lexicon`, `correct`, `model` (tự học), `decision`, `store` (mã hoá), `feedback`.
- `openvikey-lab`: harness v1 để *nhìn bộ não hoạt động* + test-runner đo độ chính xác.
- *(sau)* `openvikey-win` (TSF), `openvikey-mac` (CGEventTap) — lớp mỏng bọc core.

Thiết kế chi tiết: [`docs/superpowers/specs/2026-08-17-openvikey-design.md`](docs/superpowers/specs/2026-08-17-openvikey-design.md).

## Bảo mật & riêng tư

Local-first, **zero backend**. Mô hình lưu trong container mã hoá (envelope: DEK + XChaCha20-Poly1305, DEK bọc bởi OS keychain và/hoặc passphrase Argon2id). **Không tự động gửi plaintext hay khoá đi đâu** — chỉ *ciphertext do bạn chủ động export* mới có thể rời máy. Đồng bộ đa máy (mang blob mã hoá đi) là tính năng *dự kiến*; v1 chạy single-device.

## License

[MIT](LICENSE) — mã nguồn mở hoàn toàn (OSI). Tác giả không thu phí và không thương mại hoá; MIT **không** hạn chế người khác (kể cả dùng thương mại).
