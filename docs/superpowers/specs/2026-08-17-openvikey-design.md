# OpenViKey — Thiết kế (Design Spec)

- **Ngày:** 2026-08-17
- **Trạng thái:** v3 — implementation-ready
- **Tên dự án:** OpenViKey (`openvikey`)
- **License:** **MIT** — mã nguồn mở hoàn toàn (OSI). *Tác giả không thu phí và không thương mại hoá; MIT không hạn chế người khác* (kể cả dùng thương mại). Không gọi dự án là "phi thương mại".

> **Changelog v3:** đóng review v2: sửa metric denominator; làm Beta evidence/decay tất định; chốt state machine `ignore/suggest/auto`; hoàn thiện `InputEvent`/semantic edit/inverse undo; chốt passphrase persistence và sensitive-context contract. Implementation plan: [`../plans/2026-08-17-openvikey-v1-implementation-plan.md`](../plans/2026-08-17-openvikey-v1-implementation-plan.md).
>
> **Changelog v3.1 (2026-08-18):** GĐ2 không còn “Windows qua TSF” thuần. Chốt hybrid hook-first + TSF sau — [`2026-08-18-openvikey-gd2-windows-host-design.md`](./2026-08-18-openvikey-gd2-windows-host-design.md), ADR [`0007`](../../decisions/0007-gd2-windows-hybrid-host.md).

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
- **Lưu mã hoá local** qua `SecretProvider` inject: lab dùng passphrase file provider; unit test dùng provider in-memory/tất định.

**v1 KHÔNG làm (defer sang milestone sau — đã chốt với chủ dự án):**
- Hook bàn phím toàn hệ thống (TSF/CGEventTap) — GĐ2/3.
- **OS-keyring thật** (DPAPI/Keychain) — v1 chỉ dùng provider inject.
- **Đồng bộ & merge đa máy** (CRDT) — §7.5.
- **Khôi phục dấu cả cụm/câu** (delayed decision / beam search) — milestone riêng, §5.4.
- **Settings UI** / bảng "app đã học gì" bản GUI (v1 chỉ cần API + dump text để test).

### 2.3 Nền tảng & lộ trình
- **Đích cuối:** Windows + macOS, chung `openvikey-core`.
- **GĐ1 (v1 — spec này):** headless brain, chứng minh sự "thông minh".
- **GĐ2:** Windows **hybrid** — hook nhập chính (2a), TSF ngữ cảnh (2b), TSF nhập chính theo app (2c). Không TSF-only. Chi tiết: [`2026-08-18-openvikey-gd2-windows-host-design.md`](./2026-08-18-openvikey-gd2-windows-host-design.md).
- **GĐ3:** macOS — **spike: IMKInputController (InputMethodKit) vs CGEventTap** (chưa quyết, §10).

### 2.4 Non-goals (vĩnh viễn)
Zero backend, không tài khoản, không telemetry, không **automatic upload**. Plaintext model và khoá không rời máy; chỉ ciphertext do người dùng chủ động export mới có thể rời máy (§7).

---

## 3. Tiêu chí thành công (đo được) — v1

> Nguyên tắc: **held-out corpus** — dữ liệu build lexicon/model KHÁC dữ liệu đánh giá. Tách metric cho *auto-replace* và *suggestion*. Ngưỡng dưới là **mục tiêu khởi điểm để calibrate**, không phải hằng số bất biến. Corpus đánh giá được đóng băng bằng manifest (version + SHA-256 + provenance + split seed) trước khi calibrate.

### 3.1 Engine (tất định)
- Vượt **ma trận golden** `(chuỗi phím → tiếng Việt kỳ vọng)` phủ các trục: **input method** {Telex, VNI}; **đặt dấu** {modern `oà` / classic `òa`}; casing; reset/escape (double-key, phím khôi phục); **NFC/NFD**; tiếng Anh passthrough; URL/code/mixed. 100% pass.

### 3.2 Chất lượng sửa (theo từng loại lỗi)
Taxonomy lỗi: (a) đặt dấu sai vị trí, (b) đảo chữ/phím liền kề, (c) thiếu dấu, (d) viết tắt.
- **Auto precision:** `TP / (TP + FP)` ≥ **0.99** trên toàn bộ labeled held-out stream (gồm cả token đúng và token cài lỗi).
- **Correct-token FPR:** `false_auto_replacements / total_correct_tokens` ≤ **0.1%** trên tập câu đúng held-out (khác denominator với precision).
- **Auto coverage/recall:** `TP / total_labeled_errors`; báo tổng và riêng theo (a)–(d), không đặt ngưỡng tối thiểu ở v1 để tránh đổi precision lấy recall.
- **Suggestion:** top-1 ≥ **0.85**, top-3 ≥ **0.95**, tính trên **toàn bộ labeled errors thuộc loại generator hỗ trợ**; báo riêng candidate-coverage (có sinh ít nhất một ứng viên).
- **Kích thước tối thiểu:** correct-token corpus ≥ **50.000 token**; error corpus ≥ **1.000 ca** và ≥ **200 ca cho mỗi loại** (a)–(d). Báo point estimate + khoảng tin cậy Wilson 95%; gate dùng point estimate, CI dùng để cảnh báo độ chắc chắn.
- **Không sửa bừa:** từ *đã có dấu & hợp lệ* không bao giờ bị auto-đổi. Chữ *không dấu / viết tắt hợp lệ-mà-mơ-hồ* → tối đa **suggestion** ở cold-start; nếu auto-đổi sai thì tính là `FP` cho precision và là false correction trên correct-token corpus tương ứng.

### 3.3 Học (mô phỏng tất định)
- Trong canonical script (18 accept cùng `at_ms`, không negative), prior Beta(1,1) phải **promote** sau đúng `K_promote=18 explicit accepts +1.0`, vì `(1+18)/(1+1+18)=0.95`. Khi sự kiện trải theo thời gian và bị decay, có thể cần nhiều hơn; tín hiệu dương yếu cũng cần nhiều sự kiện hơn.
- Phép sửa đang auto bị **demote** sau `K_demote=2` undo trong **10 auto-emission gần nhất của cùng rule-context**, bất kể confidence tổng.
- **Convergence:** cùng chuỗi `InputEvent`/`FeedbackEvent` (gồm `seq` và `at_ms`) + cùng `evaluate_at_ms` → cùng trạng thái model (property test); model không tự đọc wall-clock.

### 3.4 Undo & riêng tư
- Mọi auto-replace **hoàn tác được**; chốt-rồi-undo trả đúng chuỗi gốc + delimiter và vị trí caret, kể cả replacement nhiều grapheme/nhiều từ.
- Không lời gọi mạng nào phát sinh khi chạy (test bằng network sandbox/asserts).

---

## 4. Kiến trúc

### 4.1 Crate & module (`openvikey-core`, Rust thuần, KHÔNG phụ thuộc OS)

| Module | Nhiệm vụ | Phụ thuộc |
|---|---|---|
| `types` | Kiểu chung: `InputEvent`, `FeedbackEvent`, `InputContext`, `CompositionSnapshot`, `Candidate`, `EngineAction`. | — |
| `engine` | Telex/VNI: phím → `CompositionSnapshot{raw_keys, rendered, normalized}`. Tất định. | `types` |
| `lexicon` | Từ điển nền + tần suất từ/bigram (data-driven, nạp từ file). | `types` |
| `generate` | **Generators thuần** (telex_fix, fuzzy, abbrev, diacritics) → ứng viên có điểm + **evidence/rule nguồn**. KHÔNG đọc `model`. | `lexicon`, `types` |
| `rank` | normalize điểm về `[0,1]` → **dedupe** → base rank → **personal rerank** (đọc `model`). | `model`, `types` |
| `model` | Thống kê cá nhân: đếm evidence theo **rule-context key**, confidence, hysteresis, decay. | `types` |
| `decision` | Policy: `auto / suggest / ignore` theo confidence + hysteresis. | `model` |
| `feedback` | Tín hiệu ngầm (gõ-xoá-gõ lại) + tường minh → cập nhật `model`. | `model` |
| `store` | Envelope-encryption; persistence sau trait `SecretProvider` (inject). | `model` |

**Provider traits** (inject từ ngoài):
```
trait SecretProvider {
    fn wrap(&self, dek: &Dek) -> Result<WrappedKey, StoreError>;
    fn unwrap(&self, wrapped: &WrappedKey) -> Result<Dek, StoreError>;
}
```
→ core **không** biết DPAPI/Keychain; adapter GĐ2/3 cấp bản thật. `openvikey-lab` v1 dùng **passphrase file provider** để mở lại model qua process restart; unit test dùng provider in-memory/tất định. Chỉ dùng tên `SecretProvider` trong code/spec.

**`openvikey-lab`** (harness v1): CLI nạp `InputEvent`, hiển thị composition + ứng viên + gợi ý, cho accept/reject; **test-runner** chạy corpus đo §3; **dump model** dạng text để kiểm tra "đã học gì". Full-screen TUI defer sau v1.

**Về sau:** `openvikey-win` (GĐ2a hook + `SendInput`; GĐ2b/2c TSF) và `openvikey-mac` (IMK/CGEventTap) — bọc core; TSF/IMK dịch `ReplaceRange` sang UTF-16; hook giả lập bằng backspace + Unicode.

### 4.2 Core contract (ngữ nghĩa)
- **`CompositionSnapshot`**: `revision`, `raw_keys` (chuỗi phím thô — `telex_fix` cần), `rendered` (text đang hiển thị), `normalized` (**NFC**).
- **Mọi action tự chứa payload**; adapter không cần đọc state ẩn để áp edit.
- **Đơn vị `EditRange`**: tính bằng **grapheme cluster**, có `basis = ActiveComposition | CommittedBeforeCaret` và `revision` để từ chối edit stale. TSF/IMK adapter dịch sang UTF-16 range; CGEvent adapter mô phỏng delete/insert từ semantic edit. Chuẩn hoá NFC trước khi so lexicon, nhưng giữ `original` để undo byte/text-exact theo contract hiển thị.
- **Undo autocorrect** nhận `edit_id`, tìm edit log gần nhất còn hợp lệ và phát **inverse `ReplaceRange`** (`original`/`replacement` đổi chỗ), đồng thời sinh `FeedbackEvent::Undo`. Không có action trống `UndoReplacement`.

Contract tối thiểu (tên field chuẩn cho plan; Rust syntax cụ thể có thể tinh chỉnh mà không đổi nghĩa):
```
InputEvent {
  seq: u64, at_ms: i64,
  kind: Key{logical, physical?} | Backspace | Boundary{delimiter}
      | InsertText{text} | CursorMoved | SelectionChanged | Reset,
  modifiers, is_repeat,
  context: InputContext{allow_transform, allow_learning}
}

FeedbackEvent {
  seq: u64, at_ms: i64,
  kind: Accept{candidate_id} | ExplicitReject{candidate_id}
      | Undo{edit_id} | AutoSettled{edit_id}
      | SuggestionSettled{candidate_id}
      | ImplicitCorrection{original, replacement}
}

EditRange { basis, start_grapheme, length_grapheme, revision }

EngineAction =
  UpdateComposition{revision, text}
  | Commit{revision, text, delimiter?}
  | ReplaceRange{edit_id, range, original, replacement}
  | ShowSuggestions{revision, candidates}
```

`CursorMoved`/`SelectionChanged` làm invalid composition và edit log liên quan; sau đó feedback miner không được suy ra cặp `X→Y` qua ranh giới này.

### 4.3 Luồng dữ liệu
```
InputEvent → engine (compose → CompositionSnapshot)
   → [ranh giới từ]
   → generate (generators thuần → candidates + evidence)
   → rank (normalize [0,1] → dedupe → base rank → personal rerank[model])
   → decision (state machine: ignore / suggest / auto)
        ├─ auto    → ReplaceRange + ghi log undo
        └─ suggest → ShowSuggestions (top-k)
   ← feedback (accept / explicit-reject / undo / ignore*) → model → store (debounced, encrypted)
   (* ignore = tín hiệu YẾU/censored, không phải reject cứng)
```

`openvikey-lab`/adapter sở hữu debounce worker và I/O scheduling; đường xử lý phím của core không chờ ghi đĩa. Mọi phép tính phụ thuộc thời gian nhận `at_ms`/`evaluate_at_ms` từ caller, không gọi wall-clock trực tiếp.

---

## 5. Pipeline sinh & xếp hạng ứng viên

### 5.1 Generators (thuần, độc lập model)
Đầu vào: `CompositionSnapshot` (+ ngữ cảnh trái vài token). Đầu ra: `Candidate{text, source_rule, evidence, base_score}`.
- **`telex_fix`** — đọc `raw_keys`, phát hiện dấu/mũ đặt sai vị trí, tái dựng theo luật đặt dấu TV.
- **`fuzzy`** — edit distance **có trọng số** với lexicon: phím liền kề & transposition rẻ hơn; ràng buộc âm tiết TV hợp lệ.
- **`abbrev`** — bung viết tắt (bộ mồi + đã học).
- **`diacritics`** — **per-token top-k**, xếp theo bigram (nền + cá nhân), *chỉ ngữ cảnh trái*. Mặc định ra **suggestion** (mơ hồ cao).

### 5.2 Chuẩn hoá & hợp nhất
- Đưa điểm mọi generator về **cùng thang `[0,1]`** bằng calibration config có `version` và hash, được fit chỉ trên calibration split (không dùng held-out test split).
- **Dedupe** khi nhiều generator ra cùng text (gộp evidence, giữ nguồn mạnh nhất).
- **Tie-break/priority** khi xung đột (vd abbrev vs fuzzy): theo evidence cá nhân rồi base score.

### 5.3 Personal rerank
`rank` áp thống kê cá nhân (`model`) lên danh sách đã hợp nhất → `final_score ∈ [0,1]` + thứ hạng cuối. Candidate mang **evidence + source_rule** để `feedback` quy tín hiệu về **đúng rule-context**. Cùng input + model + calibration config phải cho cùng score/order; tie cuối cùng dùng thứ tự lexical NFC để tất định.

### 5.4 Giới hạn v1 (đã chốt)
Khôi phục dấu **cả cụm/câu** (`khong the nao → không thể nào`) cần *delayed decision / beam search / sửa token đã commit* → **milestone riêng**. v1 chỉ per-token suggestion với ngữ cảnh trái.

---

## 6. Hệ thống tự học (thuật toán tất định)

### 6.1 Đơn vị học: rule-context key
Mỗi phép sửa được khoá theo context: `{input_method, correction_type, original_nfc, candidate_nfc, từ_trái_nfc?, source_rule_id}` (app thêm ở GĐ2). Cặp `original→candidate` là bắt buộc để hai phép fuzzy/abbrev khác nhau không dùng chung confidence. Evidence tích theo key ổn định này; session/revision chỉ thuộc event identity, không thuộc learning key lâu dài.

### 6.2 Confidence (Beta mass, không dùng net-count)
Tại thời điểm caller truyền `evaluate_at_ms`:
```
positive_mass = Σ positive_add(event) × decay(event.at_ms, evaluate_at_ms)
negative_mass = Σ negative_add(event) × decay(event.at_ms, evaluate_at_ms)
α = α₀ + positive_mass       (α₀ = 1)
β = β₀ + negative_mass       (β₀ = 1)
confidence = α / (α + β)
```
`positive_mass` và `negative_mass` luôn không âm; không trừ tín hiệu âm trực tiếp khỏi α và không gọi tổng chênh lệch là “net count”.

### 6.3 Decision state machine & hysteresis
Mỗi rule-context bắt đầu ở `ignore`. `final_score` là score đã calibrate/rerank ở §5.

| Trạng thái | Điều kiện | Trạng thái/action mới |
|---|---|---|
| `ignore` | `final_score ≥ S_suggest_on` (mặc định **0.70**) | `suggest` |
| `suggest` | `final_score < S_suggest_off` (mặc định **0.60**) | `ignore` |
| `suggest` | `final_score ≥ S_auto` (**0.90**) AND `confidence ≥ 0.95` AND `positive_mass ≥ 18` AND source policy cho auto | `auto` |
| `auto` | `confidence < 0.85` OR `final_score < S_auto` OR 2 undo trong 10 auto-emission gần nhất | `suggest` |

- `S_suggest_off < S_suggest_on` tạo hysteresis cho `ignore ↔ suggest`; `0.85 < 0.95` tạo hysteresis cho `suggest ↔ auto`.
- Candidate cold-start mơ hồ (unaccented/abbrev hợp lệ) bắt đầu tối đa ở `suggest` dù base score cao.
- `diacritics` có source policy `max_action=Suggest` trong v1, nên không thể auto dù confidence cao.
- Mọi threshold nằm trong versioned decision config; đổi config không sửa lịch sử evidence.

### 6.4 Trọng số tín hiệu (calibrate)
| Tín hiệu | `positive_add` | `negative_add` | Ghi chú |
|---|---:|---:|---|
| Chọn suggestion / accept | 1.0 | 0 | tường minh |
| `AutoSettled` (không undo) | 0.3 | 0 | ghi đúng 1 lần sau **10 input/edit event** kế tiếp |
| Undo autocorrect | 0 | 1.5 | tường minh, mạnh |
| `SuggestionSettled` (lờ suggestion đã hiển thị) | 0 | 0.2 | yếu/censored; không ghi nếu UI bị đóng/reset trước settle |
| Ngầm gõ-xoá-gõ lại `X→Y` | 1.0 | 0 | chỉ cho đúng rule `X→Y` |

### 6.5 Decay, clock & ổn định
- **Decay áp trên evidence event**, không trên confidence trực tiếp: `decay = 2^(-age_ms / half_life_ms)`, mặc định `half_life=30 ngày` và clamp `age_ms ≥ 0`.
- Core không gọi system clock. `InputEvent`/`FeedbackEvent` mang `at_ms`; query confidence nhận `evaluate_at_ms`. Test dùng logical clock tất định.
- **Sàn nền:** lexicon nền luôn có tiếng nói (regularization) → chống "học chết" một lỗi.
- **Xử lý cursor/edit:** nếu con trỏ nhảy/không liền mạch giữa X và Y → **không** coi là cặp sửa (tránh học nhầm).

### 6.6 Cold start & minh bạch
- Ship kèm lexicon nền + viết tắt mồi + mẫu lỗi phổ biến; lớp cá nhân đắp lên, dần lấn át.
- API xem/sửa/xoá từng mục đã học + "quên tất cả" (GUI để GĐ sau; v1 có API + text dump).

---

## 7. Bảo mật & lưu trữ (envelope encryption)

Ràng buộc: **local-first, zero backend.** Diễn đạt chính xác: **không tự động gửi plaintext hay khoá đi đâu; chỉ *ciphertext* do người dùng chủ động export mới có thể rời máy.**

### 7.1 Sơ đồ khoá (envelope)
- **DEK** (Data Encryption Key) ngẫu nhiên mã hoá blob model.
- DEK được **bọc độc lập** bởi 2 wrapper (mở bằng *bất kỳ* cái nào):
  1. khoá từ **OS keyring** (DPAPI/Keychain) — tiện, GĐ2+;
  2. khoá dẫn từ **passphrase** (Argon2id) — *di động*, mở được trên máy mới; đây là wrapper persistence thật của `openvikey-lab` v1.
- → giải bài toán "máy mới mở blob khi OS-key không di chuyển".

### 7.2 Mã hoá
- **XChaCha20-Poly1305** (nonce 24-byte từ CSPRNG; mỗi lần ghi dùng nonce mới, tránh reuse).
- Container tách **immutable payload header** (`magic`, container/schema version, payload nonce, model metadata) làm AAD cho model ciphertext khỏi các **wrapped-key slot** được xác thực riêng. Mỗi key slot chứa wrapper kind/version, salt, KDF params và wrapped DEK.
- Rewrap passphrase chỉ thay key slot; immutable payload header + model ciphertext/tag giữ nguyên, nên không cần mã hoá lại model.
- Argon2id default theo [RFC 9106](https://www.ietf.org/rfc/rfc9106.html) low-memory profile: `m=64 MiB, t=3, p=4`; benchmark lúc mở file, nhưng không hạ thấp hơn [OWASP Password Storage floor](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html) `m=19 MiB, t=2, p=1`. Tham số + salt riêng nằm trong authenticated key slot.
- Schema **versioned** để migrate.

### 7.3 Độ bền
- **Atomic write** (ghi file tạm → rename) + **backup** bản trước + phục hồi khi corrupt/mất điện.
- **Key rotation** & đổi passphrase (rewrap DEK, không cần giải/mã lại toàn model).

### 7.4 Chốt chặn ngữ cảnh nhạy cảm
`InputContext` tách hai capability: `allow_transform` và `allow_learning`. Với password/terminal/denylist, harness v1 mô phỏng policy `false/false` và test rằng không generator/model/store side effect nào chạy. Adapter GĐ2+ chịu trách nhiệm phát hiện context thật; core chỉ thi hành flags, không tự đoán app/field.

### 7.5 Đồng bộ (DEFER khỏi v1)
v1 **single-device**. Khi làm sync: state-based **G/PN-Counter CRDT** (per-replica vector, merge **component-wise max**, **không cộng blob**), **tombstone** cho xoá, **decay KHÔNG áp lúc merge**, replica-id + version vector, **convergence tests**.

---

## 8. Chiến lược test (mục đích cốt lõi của v1 — TDD)

- **Golden engine matrix** (§3.1): tất định, 100% pass.
- **Corpus runner** (§3.2): manifest-pinned held-out; báo đúng denominator cho precision/FPR/coverage + suggestion top-1/3 + recall theo loại lỗi + Wilson CI.
- **Learning simulation** (§3.3/§6): canonical script promote ở accept thứ 18; demote sau 2/10 undo; **convergence property test** với injected logical clock.
- **Contract/property tests:** action payload tự chứa; stale revision bị từ chối; cursor/selection invalidates edit log; auto luôn sinh inverse edit undo trả nguyên trạng + delimiter + caret.
- **Safety policy tests:** từ có-dấu-hợp-lệ không bị auto-đổi; diacritics không auto ở v1; ignore không phạt như reject cứng; `allow_transform=false`/`allow_learning=false` không tạo transform/evidence/store mutation.
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
- **TSF trên Rust:** ít ví dụ; GĐ2a **không** làm TSF. GĐ2b/2c: DLL COM (C++ hoặc `windows` crate), brain vẫn Rust. Xem ADR 0007.
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
1. v1/Part 2: đã implement theo plan tương ứng.
2. GĐ2: triển khai **2a** theo [`../plans/2026-08-18-openvikey-gd2a-implementation-plan.md`](../plans/2026-08-18-openvikey-gd2a-implementation-plan.md) sau khi chủ dự án duyệt spec. Không gộp 2b/2c vào 2a.
