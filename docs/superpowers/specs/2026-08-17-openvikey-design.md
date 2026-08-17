# OpenViKey — Thiết kế (Design Spec)

- **Ngày:** 2026-08-17
- **Trạng thái:** Draft (chờ review)
- **Tên dự án:** OpenViKey (`openvikey`)
- **License dự kiến:** Open source hoàn toàn (permissive, ví dụ MIT/Apache-2.0), phi thương mại — không thu phí.

---

## 1. Tầm nhìn & điểm khác biệt

OpenViKey là **bộ gõ tiếng Việt tự học cá nhân**. Khác với UniKey/OpenKey (bộ gõ *tĩnh*: luật cố định + danh sách viết tắt phải tự khai báo), OpenViKey xây một **mô hình gõ chữ cá nhân hoá** thích nghi dần theo *cách gõ riêng của từng người* — giống như mỗi người một nét chữ tay.

Vấn đề gốc cần giải: gõ tiếng Việt nhanh thì hay sai (đảo chữ, nuốt dấu, đặt dấu sai chỗ, viết tắt), phải xoá đi gõ lại liên tục; các công cụ hiện có yêu cầu liệt kê luật thủ công và không học được thói quen cá nhân.

**Luận điểm cốt lõi:** không hai người gõ giống nhau. "Chắc chắn" (khi nào tự sửa) không phải một ngưỡng cố định, mà là **độ tự tin học được** riêng cho từng người, từng phép sửa.

---

## 2. Mục tiêu & phạm vi

### 2.1 Sáu năng lực sản phẩm (bức tranh đầy đủ)
1. **Engine gõ** Telex/VNI: phím → tiếng Việt.
2. **Sửa lỗi gõ/đảo chữ (fuzzy):** `khọgn → không`, `chàoo → chào`.
3. **Sửa telex/dấu sai vị trí:** `ch2ao → chào`.
4. **Viết tắt tự bung:** `ko → không`, `dc → được`, `ntn → như thế nào`.
5. **Tự thêm dấu cho chữ không dấu (diacritic restoration):** `khong the nao → không thể nào`.
6. **Mô hình tự tin thích nghi:** học từ tín hiệu ngầm + tường minh, "tốt nghiệp" phép sửa từ *gợi ý* lên *tự thay*, và hạ cấp khi bị từ chối.

### 2.2 Nền tảng & lộ trình
- **Đích cuối:** Windows + macOS, dùng chung một core.
- **Giai đoạn 1 (v1 — spec này tập trung vào đây):** *Headless brain*. Toàn bộ engine + 4 loại sửa + vòng học chạy trong một **harness thử nghiệm (CLI/TUI)** — **chưa** hook toàn hệ thống. Mục tiêu: **chứng minh phần khó nhất (sự "thông minh")** trước khi đụng tích hợp OS.
- **Giai đoạn 2:** Bọc tích hợp toàn hệ thống trên **Windows qua TSF (Text Services Framework)** quanh core đã chín.
- **Giai đoạn 3:** **macOS qua CGEventTap**, tái dùng chính core đó.

### 2.3 Non-goals (v1)
- Không hook bàn phím toàn hệ thống (để GĐ 2).
- Không UI cài đặt hoàn chỉnh, không đóng gói installer.
- Không hạ tầng backend, không tài khoản, không cloud, không telemetry — **vĩnh viễn**.
- Không macOS trong v1.

---

## 3. Tiêu chí thành công (đo được) — v1

1. **Đúng cơ bản:** engine Telex/VNI vượt một bộ golden test tất định (chuỗi phím → tiếng Việt kỳ vọng) cho mọi ca chuẩn.
2. **Bốn bộ sửa hoạt động:** mỗi bộ (telex-fix, fuzzy, abbrev, diacritics) vượt bộ golden test riêng.
3. **Không sửa bừa:** trên corpus câu tiếng Việt gõ đúng, **tỉ lệ sửa-sai (false-correction) < ngưỡng đặt ra** (chỉ số quan trọng nhất). Từ gõ đúng không bao giờ bị đổi.
4. **Học được:** trong mô phỏng vòng học, một phép sửa nhất quán **tốt nghiệp** lên "tự thay" sau ≤ N lần lặp; một phép sửa bị từ chối liên tục **hạ cấp** khỏi "tự thay".
5. **Hoàn tác được:** mọi tự-thay đều đảo ngược được; chốt-rồi-undo trả về nguyên trạng ký tự.
6. **Riêng tư:** model lưu ở dạng mã hoá; không có lời gọi mạng nào phát sinh khi chạy.

---

## 4. Kiến trúc

### 4.1 Crate & module

**`openvikey-core`** (Rust thuần, không phụ thuộc OS) — bộ não:

| Module | Nhiệm vụ | Phụ thuộc |
|---|---|---|
| `engine` | Telex/VNI: phím → tiếng Việt; giữ *từ đang soạn* (composing buffer). Phần tất định. | — |
| `lexicon` | Từ điển tiếng Việt nền + bảng tần suất từ/bigram (kiến thức cold-start), nạp từ file data đóng gói. | — |
| `correct` | 4 bộ sửa chạy trên một từ hoàn chỉnh + ngữ cảnh trái → sinh ứng viên có điểm. | `lexicon`, `model` |
| `model` | **Mô hình cá nhân** đè lên `lexicon`: đếm chấp nhận/từ chối, tần suất từ riêng, viết tắt & cặp gõ-sai→sửa đã học; điểm tự tin + ngưỡng + decay. | — |
| `decision` | Cổng tự tin: mỗi ứng viên → *tự-thay / gợi-ý / bỏ qua*. | `model` |
| `store` | Lưu mã hoá (XChaCha20-Poly1305 + Argon2id + OS keyring); nạp/ghi/**merge** blob model. | `model` |
| `feedback` | Nuốt tín hiệu ngầm (gõ-xoá-gõ lại) + tường minh (accept/reject) → cập nhật `model`. | `model` |

**`openvikey-lab`** (harness v1): CLI/TUI nạp phím vào core, hiển thị từ-đang-soạn + ứng viên + gợi ý, cho phép accept/reject để *quan sát bộ não*; kèm **test-runner** chạy corpus đo độ chính xác & hành vi học.

**Về sau (không thuộc v1):** `openvikey-win` (TSF), `openvikey-mac` (CGEventTap) — mỏng, bọc quanh `openvikey-core`.

### 4.2 Nguyên tắc thiết kế
- **Core tách rời hoàn toàn khỏi nền tảng** (bài học từ bamboo-core/libunikey): core không biết gì về OS, chỉ nhận sự kiện phím và trả về *(số backspace cần phát, text mới, danh sách gợi ý)*.
- **Interface rõ ràng** cho engine (tham khảo `IInputEngine` của VKey): `push_key`, `backspace`, `peek`, `commit`, `reset` — không dùng global state.
- Mỗi module một trách nhiệm, test độc lập được.

### 4.3 Luồng dữ liệu (mỗi lần gõ phím)
```
phím → engine (soạn từ) → [ranh giới từ? space/dấu câu]
   → correct (4 bộ sửa → ứng viên có điểm)
   → model (tái tính trọng số theo thống kê cá nhân)
   → decision (cổng tự tin)
        ├─ tự tin cao → TỰ THAY (phát backspace + text mới) + ghi log để undo
        └─ tự tin thấp → HIỆN GỢI Ý (1–3 ứng viên)
   ← feedback (accept/reject; hoặc phát hiện gõ-xoá-gõ lại) → cập nhật model → store
```

---

## 5. Pipeline sửa lỗi (chi tiết 4 bộ sửa)

Đầu vào mỗi bộ: một *từ hoàn chỉnh* (khi gặp ranh giới từ) + ngữ cảnh trái (vài từ trước). Đầu ra: danh sách ứng viên `(text, loại, điểm gốc)`.

- **`telex_fix`** — bắt lỗi *chuỗi gõ* dấu/mũ đặt sai vị trí (vd `ch2ao`, số/ký tự dấu lọt sai chỗ) và tái dựng đúng theo quy tắc đặt dấu tiếng Việt.
- **`fuzzy`** — so khớp mờ với `lexicon` theo **edit distance có trọng số**: phím liền kề trên bàn phím rẻ hơn, hoán vị (transposition) rẻ hơn, có xét âm tiết hợp lệ tiếng Việt. Xử lý `khọgn → không`.
- **`abbrev`** — bung viết tắt từ *bộ mồi phổ biến* + *viết tắt đã học của người dùng*. `ko → không`.
- **`diacritics`** — khôi phục dấu từ chữ không dấu; xếp hạng nhiều phương án theo **mô hình ngôn ngữ bigram** (nền + cá nhân). Đây là bộ **mơ hồ nhất** → thường ra *gợi ý* thay vì tự-thay.

Tất cả ứng viên đi qua `model` (tái tính trọng số theo cá nhân) rồi `decision`.

---

## 6. Hệ thống tự học (trái tim sản phẩm)

### 6.1 Điểm tự tin & hai ngưỡng
- Mỗi luật/cặp sửa giữ bộ đếm *chấp nhận/từ chối*; confidence = tỉ lệ chấp nhận được làm mượt (Bayesian smoothing).
- **Hai ngưỡng có vùng đệm (hysteresis)** chống dao động: `T_gợi-ý` (vượt → hiện gợi ý) và `T_tự-thay` (vượt → tự thay). Đây chính là định nghĩa định lượng của "chắc chắn".

### 6.2 Tín hiệu học
- **Ngầm (mạnh nhất, không tốn công người dùng):** phát hiện mẫu *gõ X → backspace → gõ Y* trong cửa sổ ngắn → ứng viên sửa `X→Y`.
- **Tường minh:** chọn gợi ý / không hoàn tác tự-thay = `+`; hoàn tác trong N phím / lờ gợi ý = `−`.
- **Corpus cá nhân:** từ đã chốt cập nhật bảng tần suất riêng → nuôi `fuzzy` và `diacritics` nghiêng về từ ngữ *người dùng thật sự dùng*.

### 6.3 Ổn định & an toàn học
- **Decay theo thời gian:** gần đây quan trọng hơn nhưng không overfit một phiên.
- **Sàn nền (regularization):** `lexicon` nền luôn có tiếng nói → chống "học chết" một lỗi gõ.
- **Cold start:** ngày đầu app không rỗng — ship kèm lexicon nền + bộ viết tắt mồi + mẫu lỗi gõ phổ biến; lớp cá nhân đắp lên và dần lấn át.
- **Vòng lặp phản hồi:** vì tự-thay có thể tự củng cố cái sai → **undo một phím** + **nhật ký "vừa sửa gì"**.

### 6.4 Minh bạch & kiểm soát
- Bảng "app đã học gì": xem/sửa/xoá từng mục; nút **"quên cái này"** và **"quên tất cả"**.
- Model khi giải mã là đọc được bằng mắt người (định dạng minh bạch).

---

## 7. Bảo mật & lưu trữ

Ràng buộc bất di bất dịch: **local-first, zero backend, không lưu data người dùng ở bất kỳ đâu ngoài máy họ.** Model cá nhân được coi như *dữ liệu định danh sinh trắc* → bảo mật từ ngày đầu.

1. **Mã hoá tại chỗ:** toàn bộ model nằm trong một container mã hoá **XChaCha20-Poly1305** (hoặc AES-256-GCM).
2. **Nguồn khoá:** khoá chính bọc trong **kho bảo mật HĐH** — Windows DPAPI/Credential Manager, macOS Keychain — để không phải gõ mật khẩu mỗi lần. Kèm **passphrase** người dùng đặt (dẫn xuất khoá **Argon2id**) làm khoá *di động* cho đồng bộ.
3. **Đồng bộ = chính blob mã hoá đó, người dùng tự mang đi** (Dropbox/iCloud/USB của họ). Nơi trung chuyển không bao giờ thấy nội dung → không cần server nào.
4. **Merge khi đồng bộ:** Win và Mac học độc lập → gộp có **cộng dồn đếm + mốc thời gian + decay** (hướng CRDT), không ghi đè thô.
5. **Không telemetry, không gọi mạng** mặc định — chạy offline hoàn toàn.
6. **Chốt chặn ngữ cảnh nhạy cảm:** không học & không kích hoạt trong ô mật khẩu, terminal, app trong denylist (mô phỏng ở v1 harness; thật ở GĐ 2).

---

## 8. Chiến lược test (mục đích cốt lõi của v1)

Làm **test-first (TDD)**:
- **Golden corpus:** ca `(chuỗi phím → tiếng Việt kỳ vọng)` tất định cho engine + từng bộ sửa.
- **Đo độ chính xác:** chạy trên corpus câu có chèn lỗi → precision/recall và **tỉ lệ sửa-sai** (chỉ số then chốt).
- **Mô phỏng vòng học:** "user script" gõ theo phong cách + lỗi nhất quán → khẳng định model *tốt nghiệp* đúng phép sửa sau N lần và *hạ cấp* cái bị từ chối.
- **Property test:** từ gõ đúng không bao giờ bị sửa; tự-thay luôn hoàn tác được; chốt-rồi-undo trả nguyên trạng.

---

## 9. Prior art & tài liệu tham khảo

> Đã khảo sát & clone (shallow) để nghiên cứu — "học cái người khác đã làm thay vì tự vẽ lại con đường". URL đã verify trực tiếp trên GitHub. Bản đồ *repo → feature* giúp biết đọc cái gì khi làm phần nào.

### 9.1 Repo tiếng Việt đã có sẵn (`D:\Workspace\CloneFromGit\VNKeyboard`)
- **OpenKey** (C++, GPL) — mẫu đa nền tảng Win+Mac gần nhất: engine C++ dùng chung + CGEventTap (mac) + LL hook (win).
- **bamboo-core** (Go, MIT) — engine thuần tách rời nền tảng cực sạch (mẫu tách core/platform).
- **VKey** (C++20, GPL) — thiết kế `IInputEngine` + `EngineFactory` đẹp nhất, có TSF.
- **libunikey / ibus-unikey / ukengine** (C++, LGPL) — engine UniKey gốc dạng thư viện.
- **PHTV** (Swift, AGPL) — IME macOS hiện đại (tham khảo lớp CGEventTap).

### 9.2 Repo tiếng Việt clone thêm (`D:\Workspace\CloneFromGit\VNKeyboard`)
| Repo | Ngôn ngữ | License | Liên quan (feature) |
|---|---|---|---|
| `ZeroX-DG/vi-rs` | Rust | MIT ✅ | **Prior art gần nhất** — engine gõ TV bằng Rust; ứng viên *dependency* cho `engine` (F1). |
| `huytd/goxkey` | Rust | BSD-3 ✅ | App IME Rust dựng *trên* vi-rs → mẫu nối engine ↔ hook OS (F1, khung app). |
| `vndangkhoa/vietc` | Rust | MIT ✅ | IME Rust hiện đại: macro + nhớ theo app + **phát hiện ô mật khẩu** (F1, F4, chốt chặn ngữ cảnh). |
| `ducngg/v7` | Python | Apache-2.0 ✅ | AI IME viết tắt phụ âm+dấu (`x0ch2→xin chào`) qua GPT riêng (F4, F6). |
| `lamquangminh/EVKey` | C++ | (kiểm tra `docs/LICENSE`) | IME Win/mac phổ biến sau UniKey — benchmark smart-typing/auto-correct (F2, F3, F4). |
| `undertheseanlp/underthesea` | Python | Apache-2.0 ✅ | NLP tiếng Việt: tách từ + corpus + tần suất — dữ liệu cho **thêm dấu** (F5). |
| `duongntbk/restore_vietnamese_diacritics` | Python | MIT ✅ | Khôi phục dấu bằng Transformer (~94%) — tham chiếu *tái dùng được* tốt nhất cho F5. |
| `suicao/Vn-Accent-Restorer` | Python | — (chỉ nghiên cứu) | Nhiều cách tiếp cận thêm dấu (RNN/Transformer/seq2seq) để so sánh (F5). |

*Dead-ends (khỏi tìm):* **GoTV/GoTiengViet** = freeware, **không** open source; bonus nếu cần thêm mẫu macOS: `xmannv/xkey` (Swift), `locple/VietKK` (JS).

### 9.3 Hệ thống auto-fix / adaptive-input ngôn ngữ khác (`D:\Workspace\CloneFromGit\SmartInput-OtherLangs`)
| Repo | Ngôn ngữ | License | Liên quan (feature) |
|---|---|---|---|
| `espanso/espanso` | Rust | GPL-3.0 (⚠️ copyleft, chỉ nghiên cứu) | **Kiến trúc song sinh**: Rust đa nền tảng, bắt phím toàn hệ thống + chèn text; blueprint khung app + F4. |
| `wolfgarbe/SymSpell` | C# | MIT ✅ | Thuật toán Symmetric-Delete — chuẩn cho **fuzzy nhanh** (F2, F3). Port sang Rust. |
| `google/mozc` | C++ | BSD-3 ✅ | Google Japanese Input: chuyển đổi thống kê + **học lịch sử người dùng** — chuẩn vàng cho F5, F6. |
| `rime/librime` | C++ | BSD-3 ✅ | IME Trung với **từ điển người dùng tự học** + engine schema YAML (F1, F6). |
| `AnySoftKeyboard/AnySoftKeyboard` | Java | Apache-2.0 ✅ | Autocorrect + next-word + **học từ cá nhân** + Incognito (tắt học) — gương cho F2, F4, F6 & toggle riêng tư. |
| `openboard-team/openboard` | Java | GPL-3.0 (⚠️) | LatinIME: từ điển cá nhân + next-word + suggestion-strip (F5, F6). |
| `Manouchehri/presage` | C++ | GPL-2.0 (⚠️, mirror) | Predictive-text + **mô hình ngôn ngữ người dùng thích nghi** — thẳng vào F6. |
| `hunspell/hunspell` | C++ | LGPL/GPL/MPL | Spell-check + phân tích hình thái (dictionary/affix) cho F2. |

**Top 3 đọc trước cho điểm khác biệt (F6 — tự học):** `mozc` (học lịch sử + chuyển đổi thống kê), `AnySoftKeyboard` (học từ cá nhân + toggle riêng tư sát mục tiêu của ta), `presage` (mô hình ngôn ngữ thích nghi). Cho core Rust: bắt đầu `vi-rs` + `goxkey`, dùng `espanso` làm blueprint bắt/chèn phím.

**Lưu ý license khi *tái dùng code* (không chỉ đọc):** an toàn để mượn → vi-rs, goxkey, vietc, mozc, librime, SymSpell, underthesea, AnySoftKeyboard, duongntbk. **Copyleft (GPL) — đọc thoải mái, nhưng chép/link code buộc dự án theo GPL** → espanso, openboard, presage, hunspell(nhánh GPL). OpenViKey là MIT nên **tránh chép code GPL**; chỉ học ý tưởng.

*Repo thứ cấp (clone thêm nếu cần):* `fcitx/fcitx5`, `nuspell/nuspell`, `keymanapp/keyman` (monorepo lớn — bắt buộc `--depth 1`), `florisboard/florisboard` (lưu ý: autocorrect **chưa** ship), `kunkel321/AutoCorrect2` (giá trị ở **dataset 7000+ hotstring** typo→fix cho F3, F4).

---

## 10. Rủi ro & câu hỏi mở

- **TSF trên Rust (GĐ 2):** ít ví dụ hơn C++ → có thể phải viết binding COM, hoặc tiến hoá sang kiến trúc lai (brain Rust + shim C++). Quyết định *hoãn* tới GĐ 2 (YAGNI).
- **Chất lượng diacritic restoration** phụ thuộc mô hình ngôn ngữ + dữ liệu → cần corpus tiếng Việt đủ tốt; giữ bộ này thiên *gợi ý* để hạn chế sửa-sai.
- **Ranh giới từ tiếng Việt** (âm tiết vs từ ghép) ảnh hưởng thời điểm chạy correction → cần thử nghiệm.
- **Cân bằng aggressiveness:** ngưỡng khởi tạo & tốc độ tốt-nghiệp cần tinh chỉnh qua thực nghiệm ở harness.

---

## 11. Bước tiếp theo
1. Hoàn tất clone repo tham khảo (2 phạm vi) → cập nhật mục 9.
2. Người dùng review spec này.
3. Chuyển sang **writing-plans** để lập kế hoạch triển khai chi tiết cho v1 (headless brain, TDD).
