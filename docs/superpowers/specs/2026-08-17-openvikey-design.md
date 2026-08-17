# OpenViKey — Thiết kế (Design Spec)

- **Ngày:** 2026-08-17
- **Trạng thái:** Draft v2 (đã chỉnh theo review — chờ review lại)
- **Tên dự án:** OpenViKey (`openvikey`)
- **License:** **MIT** — mã nguồn mở hoàn toàn (OSI). *Tác giả không thu phí và không thương mại hoá; MIT không hạn chế người khác* (kể cả dùng thương mại). Không gọi dự án là "phi thương mại".

> **Changelog v2:** siết phạm vi v1 (defer sync/merge, OS-keyring thật, khôi phục dấu cả cụm, settings UI); biến contract engine, thuật toán học, tiêu chí thành công thành thứ *đo được & tất định*; sửa mâu thuẫn pipeline; thêm envelope-encryption & performance budget; cập nhật provenance.

---

## 1. Tầm nhìn & điểm khác biệt

OpenViKey là **bộ gõ tiếng Việt tự học cá nhân**. Khác UniKey/OpenKey (bộ gõ *tĩnh*: luật cố định + danh sách viết tắt tự khai báo), OpenViKey xây một **mô hình gõ chữ cá nhân hoá** thích nghi dần theo *cách gõ riêng của từng người* — như mỗi người một nét chữ tay.

Vấn đề gốc: gõ nhanh hay sai (đảo chữ, nuốt/đặt dấu sai chỗ, viết tắt), phải xoá đi gõ lại; công cụ hiện có bắt liệt kê luật thủ công và không học thói quen cá nhân.

**Luận điểm cốt lõi:** "chắc chắn" (khi nào tự sửa) không phải ngưỡng cố định mà là **độ tự tin học được**, riêng theo từng người, từng phép sửa.

---

## 2. Phạm vi

### 2.1 Sáu năng lực sản phẩm (bức tranh đầy đủ, không phải tất cả trong v1)
1. Engine gõ Telex/VNI.
2. Sửa lỗi gõ/đảo chữ (fuzzy): `khọgn → không`.
3. Sửa telex/dấu sai vị trí: `ch2ao → chào`.
4. Viết tắt tự bung: `ko → không`.
5. Thêm dấu cho chữ không dấu (per-token trong v1; cả cụm-câu là milestone sau).
6. Mô hình tự tin thích nghi theo từng người dùng.

### 2.2 Feature matrix — v1 (headless brain)

**v1 BẮT BUỘC làm:**
- `engine` Telex/VNI + `CompositionSnapshot` (raw_keys / rendered / normalized-NFC).
- Sinh ứng viên (candidate generation) cho cả 4 loại sửa — **diacritics giới hạn per-token top-k suggestion, chỉ ngữ cảnh trái**.
- `decision` + `model` học **mô phỏng** (deterministic), gồm undo/hoàn tác.
- **Corpus test-runner** đo các metric ở §3.
- **Lưu mã hoá local** qua `SecretProvider`/`KeyProvider` *inject* (test dùng provider in-memory/tất định).

**v1 KHÔNG làm (defer sang milestone sau — đã chốt với chủ dự án):**
- Hook bàn phím toàn hệ thống (TSF/CGEventTap) — GĐ2/3.
- **OS-keyring thật** (DPAPI/Keychain) — v1 chỉ dùng provider inject.
- **Đồng bộ & merge đa máy** (CRDT) — §7.5.
- **Khôi phục dấu cả cụm/câu** (delayed decision / beam search) — milestone riêng, §5.4.
- **Settings UI** / bảng "app đã học gì" bản GUI (v1 chỉ cần API + dump text để test).

### 2.3 Nền tảng & lộ trình
- **Đích cuối:** Windows + macOS, chung `openvikey-core`.
- **GĐ1 (v1 — spec này):** headless brain, chứng minh sự "thông minh".
- **GĐ2:** Windows qua **TSF**.
- **GĐ3:** macOS — **spike: IMKInputController (InputMethodKit) vs CGEventTap** (chưa quyết, §10).

### 2.4 Non-goals (vĩnh viễn)
Zero backend, không tài khoản, không cloud, không telemetry, không lưu data người dùng ở đâu ngoài máy họ.

---

## 3. Tiêu chí thành công (đo được) — v1

> Nguyên tắc: **held-out corpus** — dữ liệu build lexicon/model KHÁC dữ liệu đánh giá. Tách metric cho *auto-replace* và *suggestion*. Ngưỡng dưới là **mục tiêu khởi điểm để calibrate**, không phải hằng số bất biến.

### 3.1 Engine (tất định)
- Vượt **ma trận golden** `(chuỗi phím → tiếng Việt kỳ vọng)` phủ các trục: **input method** {Telex, VNI}; **đặt dấu** {modern `oà` / classic `òa`}; casing; reset/escape (double-key, phím khôi phục); **NFC/NFD**; tiếng Anh passthrough; URL/code/mixed. 100% pass.

### 3.2 Chất lượng sửa (theo từng loại lỗi)
Taxonomy lỗi: (a) đặt dấu sai vị trí, (b) đảo chữ/phím liền kề, (c) thiếu dấu, (d) viết tắt.
- **Auto-replace:** precision ≥ **0.99** (⇒ **FPR ≤ 1%**) trên held-out; báo cả **coverage** (tỉ lệ lỗi được auto-sửa).
- **Suggestion:** **top-1 ≥ 0.85**, **top-3 ≥ 0.95** trên các lỗi thuộc coverage.
- **Recall theo từng loại lỗi** (a)–(d) báo riêng.
- **Không sửa bừa:** từ *đã có dấu & hợp lệ* không bao giờ bị auto-đổi. Chữ *không dấu / viết tắt hợp lệ-mà-mơ-hồ* → đi **suggestion**, và tính vào FPR nếu auto-đổi sai.

### 3.3 Học (mô phỏng tất định)
- Phép sửa nhất quán **promote** lên auto sau **≤ K_promote** lần net-positive (mặc định K=8, calibrate).
- Phép sửa bị từ chối **demote** khỏi auto sau **≤ K_demote** lần undo (mặc định K=2).
- **Convergence:** cùng chuỗi sự kiện → cùng trạng thái model (property test).

### 3.4 Undo & riêng tư
- Mọi auto-replace **hoàn tác được**; chốt-rồi-undo trả nguyên trạng grapheme.
- Không lời gọi mạng nào phát sinh khi chạy (test bằng network sandbox/asserts).

---

## 4. Kiến trúc

### 4.1 Crate & module (`openvikey-core`, Rust thuần, KHÔNG phụ thuộc OS)

| Module | Nhiệm vụ | Phụ thuộc |
|---|---|---|
| `types` | Kiểu chung: `InputEvent`, `CompositionSnapshot`, `Candidate`, `EngineAction`. | — |
| `engine` | Telex/VNI: phím → `CompositionSnapshot{raw_keys, rendered, normalized}`. Tất định. | `types` |
| `lexicon` | Từ điển nền + tần suất từ/bigram (data-driven, nạp từ file). | `types` |
| `generate` | **Generators thuần** (telex_fix, fuzzy, abbrev, diacritics) → ứng viên có điểm + **evidence/rule nguồn**. KHÔNG đọc `model`. | `lexicon`, `engine` |
| `rank` | normalize thang điểm → **dedupe** → base rank → **personal rerank** (đọc `model`). | `model` |
| `model` | Thống kê cá nhân: đếm evidence theo **rule-context key**, confidence, hysteresis, decay. | `types` |
| `decision` | Policy: `auto / suggest / ignore` theo confidence + hysteresis. | `model` |
| `feedback` | Tín hiệu ngầm (gõ-xoá-gõ lại) + tường minh → cập nhật `model`. | `model` |
| `store` | Envelope-encryption; persistence sau trait `KeyProvider`/`SecretProvider` (inject). | `model` |

**Provider traits** (inject từ ngoài; v1 dùng bản in-memory/tất định):
```
trait SecretProvider { fn wrap(&self, dek: &Dek) -> Wrapped; fn unwrap(&self, w: &Wrapped) -> Option<Dek>; }
```
→ core **không** biết DPAPI/Keychain; adapter GĐ2/3 cấp bản thật.

**`openvikey-lab`** (harness v1): CLI/TUI nạp `InputEvent`, hiển thị composition + ứng viên + gợi ý, cho accept/reject; **test-runner** chạy corpus đo §3; **dump model** dạng text để kiểm tra "đã học gì".

**Về sau:** `openvikey-win` (TSF), `openvikey-mac` (IMK/CGEventTap) — bọc core, tự dịch `ReplaceRange` sang **UTF-16/CGEvent**.

### 4.2 Core contract (ngữ nghĩa)
- **`CompositionSnapshot`**: `raw_keys` (chuỗi phím thô — `telex_fix` cần), `rendered` (text đang hiển thị), `normalized` (**NFC**).
- **`EngineAction`**: `UpdateComposition | Commit | ReplaceRange{range, text} | ShowSuggestions(Vec<Candidate>) | UndoReplacement`.
- **Đơn vị `range`**: tính bằng **grapheme cluster** ở core (adapter dịch sang UTF-16 code unit cho TSF / CGEvent cho macOS). Chuẩn hoá **NFC** trước khi so khớp lexicon.
- **Undo của autocorrect** là action riêng (`UndoReplacement`) — khác backspace thường; một lần undo hoàn nguyên đúng phần đã replace và ghi tín hiệu reject.

### 4.3 Luồng dữ liệu
```
InputEvent → engine (compose → CompositionSnapshot)
   → [ranh giới từ]
   → generate (generators thuần → candidates + evidence)
   → rank (normalize thang điểm → dedupe → base rank → personal rerank[model])
   → decision (hysteresis: auto / suggest / ignore)
        ├─ auto    → ReplaceRange + ghi log undo
        └─ suggest → ShowSuggestions (top-k)
   ← feedback (accept / explicit-reject / undo / ignore*) → model → store (debounced, encrypted)
   (* ignore = tín hiệu YẾU/censored, không phải reject cứng)
```

---

## 5. Pipeline sinh & xếp hạng ứng viên

### 5.1 Generators (thuần, độc lập model)
Đầu vào: `CompositionSnapshot` (+ ngữ cảnh trái vài token). Đầu ra: `Candidate{text, source_rule, evidence, base_score}`.
- **`telex_fix`** — đọc `raw_keys`, phát hiện dấu/mũ đặt sai vị trí, tái dựng theo luật đặt dấu TV.
- **`fuzzy`** — edit distance **có trọng số** với lexicon: phím liền kề & transposition rẻ hơn; ràng buộc âm tiết TV hợp lệ.
- **`abbrev`** — bung viết tắt (bộ mồi + đã học).
- **`diacritics`** — **per-token top-k**, xếp theo bigram (nền + cá nhân), *chỉ ngữ cảnh trái*. Mặc định ra **suggestion** (mơ hồ cao).

### 5.2 Chuẩn hoá & hợp nhất
- Đưa điểm mọi generator về **cùng thang** (calibrated score).
- **Dedupe** khi nhiều generator ra cùng text (gộp evidence, giữ nguồn mạnh nhất).
- **Tie-break/priority** khi xung đột (vd abbrev vs fuzzy): theo evidence cá nhân rồi base score.

### 5.3 Personal rerank
`rank` áp thống kê cá nhân (`model`) lên danh sách đã hợp nhất → thứ hạng cuối. Candidate mang **evidence + source_rule** để `feedback` quy tín hiệu về **đúng rule-context**.

### 5.4 Giới hạn v1 (đã chốt)
Khôi phục dấu **cả cụm/câu** (`khong the nao → không thể nào`) cần *delayed decision / beam search / sửa token đã commit* → **milestone riêng**. v1 chỉ per-token suggestion với ngữ cảnh trái.

---

## 6. Hệ thống tự học (thuật toán tất định)

### 6.1 Đơn vị học: rule-context key
Mỗi phép sửa được khoá theo context: `{input_method, correction_type, từ_trái?, source_rule}` (app/session thêm ở GĐ2). Evidence tích theo key này.

### 6.2 Confidence & hysteresis (state-dependent)
- Confidence = hậu nghiệm Beta với **prior α₀=β₀=1** (calibrate) trên đếm net.
- **Hysteresis phụ thuộc trạng thái:**
  - đang *suggest* → **promote** lên *auto* khi `conf ≥ 0.95` **và** evidence ≥ `min_evidence` (mặc định 8).
  - đang *auto* → **demote** về *suggest* khi `conf < 0.85` **hoặc** ≥ `K_demote` undo gần đây.

### 6.3 Trọng số tín hiệu (calibrate)
| Tín hiệu | Trọng số | Ghi chú |
|---|---|---|
| Chọn suggestion / accept | +1.0 | tường minh |
| Không-undo sau cửa sổ settle | +0.3 | **positive yếu, ghi đúng 1 lần** |
| Undo autocorrect | −1.5 | tường minh, mạnh |
| Lờ suggestion | −0.2 | **yếu/censored** (có thể user không thấy) |
| Ngầm gõ-xoá-gõ lại `X→Y` | +1.0 cho `X→Y` | mining từ hành vi |

### 6.4 Decay & ổn định
- **Decay áp trên evidence event** (không trên confidence trực tiếp): trọng số sự kiện suy giảm theo tuổi (half-life calibrate) → gần đây nặng hơn, không overfit 1 phiên.
- **Sàn nền:** lexicon nền luôn có tiếng nói (regularization) → chống "học chết" một lỗi.
- **Xử lý cursor/edit:** nếu con trỏ nhảy/không liền mạch giữa X và Y → **không** coi là cặp sửa (tránh học nhầm).

### 6.5 Cold start & minh bạch
- Ship kèm lexicon nền + viết tắt mồi + mẫu lỗi phổ biến; lớp cá nhân đắp lên, dần lấn át.
- API xem/sửa/xoá từng mục đã học + "quên tất cả" (GUI để GĐ sau; v1 có API + text dump).

---

## 7. Bảo mật & lưu trữ (envelope encryption)

Ràng buộc: **local-first, zero backend.** Diễn đạt chính xác: **không tự động gửi plaintext hay khoá đi đâu; chỉ *ciphertext* do người dùng chủ động export mới có thể rời máy.**

### 7.1 Sơ đồ khoá (envelope)
- **DEK** (Data Encryption Key) ngẫu nhiên mã hoá blob model.
- DEK được **bọc độc lập** bởi 2 wrapper (mở bằng *bất kỳ* cái nào):
  1. khoá từ **OS keyring** (DPAPI/Keychain) — tiện, GĐ2+;
  2. khoá dẫn từ **passphrase** (Argon2id) — *di động*, mở được trên máy mới.
- → giải bài toán "máy mới mở blob khi OS-key không di chuyển".

### 7.2 Mã hoá
- **XChaCha20-Poly1305** (nonce ngẫu nhiên 24-byte → an toàn với random nonce; tránh reuse).
- **Header**: magic + version + **AAD** (bind version/metadata) + salt Argon2id + tham số KDF.
- Schema **versioned** để migrate.

### 7.3 Độ bền
- **Atomic write** (ghi file tạm → rename) + **backup** bản trước + phục hồi khi corrupt/mất điện.
- **Key rotation** & đổi passphrase (rewrap DEK, không cần giải/mã lại toàn model).

### 7.4 Chốt chặn ngữ cảnh nhạy cảm
Không học & không kích hoạt ở ô mật khẩu/terminal/denylist (mô phỏng ở v1; thật ở GĐ2).

### 7.5 Đồng bộ (DEFER khỏi v1)
v1 **single-device**. Khi làm sync: state-based **G/PN-Counter CRDT** (per-replica vector, merge **component-wise max**, **không cộng blob**), **tombstone** cho xoá, **decay KHÔNG áp lúc merge**, replica-id + version vector, **convergence tests**.

---

## 8. Chiến lược test (mục đích cốt lõi của v1 — TDD)

- **Golden engine matrix** (§3.1): tất định, 100% pass.
- **Corpus runner** (§3.2): held-out; báo auto precision/FPR/coverage + suggestion top-1/3 + recall theo loại lỗi.
- **Learning simulation** (§3.3): "user script" nhất quán → khẳng định promote/demote đúng số lần; **convergence property test** (cùng events → cùng state).
- **Property tests:** từ có-dấu-hợp-lệ không bị auto-đổi; auto luôn undo được; chốt-rồi-undo trả nguyên trạng; ignore không phạt như reject cứng.
- **Perf tests** (§11).
- **Security tests:** không network call; blob mã hoá; atomic-write/recovery; rewrap khi đổi passphrase.

---

## 9. Prior art & provenance

> Đã clone (shallow) **16/16 repo** để nghiên cứu (verify: bucket 2 đủ 8). Bản đồ *repo → feature*.

### 9.1 Repo tiếng Việt có sẵn (`CloneFromGit\VNKeyboard`)
OpenKey (C++/GPL, mẫu Win+Mac), bamboo-core (Go/MIT, tách core sạch), VKey (C++/GPL, `IInputEngine`+TSF), libunikey/ibus-unikey/ukengine (C++/LGPL), PHTV (Swift/AGPL, CGEventTap).

### 9.2 Repo tiếng Việt clone thêm (`CloneFromGit\VNKeyboard`)
`ZeroX-DG/vi-rs` (Rust/MIT, F1 — **early-stage "~95%", cần compatibility gate trước khi phụ thuộc**), `huytd/goxkey` (Rust/BSD-3, khung app), `vndangkhoa/vietc` (Rust/MIT, F1/F4 + password-field), `ducngg/v7` (Py/Apache-2.0, F4/F6), `lamquangminh/EVKey` (C++, **license chưa rõ → verify**, benchmark F2/F3/F4), `undertheseanlp/underthesea` (Py/Apache-2.0, data F5), `duongntbk/restore_vietnamese_diacritics` (Py/MIT, F5), `suicao/Vn-Accent-Restorer` (Py/—, F5 study).

### 9.3 Ngôn ngữ khác (`CloneFromGit\SmartInput-OtherLangs`)
`espanso` (Rust/GPL-3 ⚠️, blueprint bắt/chèn phím), `SymSpell` (C#/MIT, fuzzy F2/F3), `mozc` (C++/BSD-3, F5/F6 học lịch sử), `librime` (C++/BSD-3, từ điển user F1/F6), `AnySoftKeyboard` (Java/Apache-2.0, F2/F4/F6 + toggle riêng tư), `openboard` (Java/GPL-3 ⚠️, F5/F6), `presage` (C++/GPL-2 ⚠️ mirror, F6), `hunspell` (C++/LGPL-GPL-MPL, F2).

### 9.4 Provenance & license (khi *tái dùng*, không chỉ đọc)
- **License code ≠ license dữ liệu:** corpus/từ điển/pretrained-weights có giấy phép RIÊNG. Trước khi nhúng data (nhất là cho F5), phải kiểm license *dữ liệu*.
- OpenViKey là **MIT** → **tránh chép/link code GPL** (espanso, openboard, presage, hunspell-nhánh-GPL); chỉ học ý tưởng.
- **Bảng provenance sẽ ghi:** repo · commit hash · license code · license data · redistribution · mục đích dùng. (Lập khi bắt đầu nhúng data ở giai đoạn plan.)

---

## 10. Rủi ro & câu hỏi mở
- **macOS (GĐ3):** IMKInputController (InputMethodKit — API IME gốc, có composition/candidate/replacement-range) **vs** CGEventTap (event tap, cần Accessibility + Input Monitoring). → **spike, chưa quyết**.
- **TSF trên Rust (GĐ2):** ít ví dụ; có thể tiến hoá kiến trúc lai (brain Rust + shim C++). Hoãn (YAGNI).
- **Chất lượng diacritics** phụ thuộc data & license → giữ per-token/suggestion ở v1.
- **Calibrate ngưỡng** (hysteresis, trọng số, decay half-life, metric targets) qua thực nghiệm ở harness.
- **vi-rs compatibility gate** trước khi chọn làm dependency của `engine`.

---

## 11. Performance budget (interactive input engine)

| Chỉ số | Mục tiêu khởi điểm (calibrate) |
|---|---|
| Per-key latency | P50 < 1ms, **P95 < 5ms** |
| Candidate generation (lúc commit từ) | P95 < 15ms |
| Startup / load model | < 300ms |
| Peak memory | < 150MB |
| Kích thước lexicon+model đóng gói | < ~50MB |
| Lưu mã hoá | **async, debounce (~2s), không blocking** đường gõ |

---

## 12. Bước tiếp theo
1. Người dùng review spec v2.
2. Chuyển **writing-plans** → kế hoạch triển khai v1 (headless brain, TDD), gồm: lập **bảng provenance data**, và **vi-rs compatibility gate**.
