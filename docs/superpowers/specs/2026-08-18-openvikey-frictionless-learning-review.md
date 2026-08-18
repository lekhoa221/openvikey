# Review — Học không gián đoạn (Frictionless Learning Design Spec)

- **Ngày:** 2026-08-18
- **Trạng thái:** review v1 — ghi nhận để thảo luận, chưa sửa spec gốc
- **Spec được review:** [`2026-08-18-openvikey-frictionless-learning-design.md`](./2026-08-18-openvikey-frictionless-learning-design.md)
- **Phạm vi đối chiếu:** `openvikey-core`, `openvikey-session`, `openvikey-win`, `data/fixtures/corpus`
- **Cách làm:** đọc spec 631 dòng, đối chiếu với `model.rs`, `decision.rs`, `feedback.rs`, `correction.rs`, `rank.rs`, `session.rs`, `host.rs`, `classify.rs` và corpus fixtures hiện có.

---

## 0. Kết luận

Nhu cầu ở §1 của spec là thật và spec **chẩn đoán đúng**: `Ctrl+.` là ma sát, vòng lặp cold-start là nút thắt. Nguyên tắc §3.1 — tách Undo / sửa tự nhiên / Space / im lặng thành các mức tín hiệu khác nhau — là phần mạnh nhất và đúng nhất của spec.

Nhưng spec **giải sai tầng của vấn đề**. Nó thêm hai cơ chế *phân phối* (Boundary Assist, Probation Auto) lên trên một tầng *tích lũy bằng chứng* vốn không thể tích lũy được với code hiện tại. Nếu triển khai nguyên trạng, cả ba cơ chế sẽ không kích hoạt trong thực tế.

Ba điều kiện tiên quyết phải xử lý trước khi bất kỳ giai đoạn nào của §12 có nghĩa: **A1** (learning bị tắt ở Electron/browser), **A2** (`RuleContextKey` quá thưa), **A3** (quality gate không có đường đi tới).

---

## 1. Nhóm A — Chặn cứng nhu cầu

### A1. Tính năng bị tắt ở đúng nơi người dùng gõ nhiều nhất

`crates/openvikey-win/src/host.rs:519-521`

```rust
fn allow_learning_for_foreground(&self) -> bool {
    self.profile == InjectProfile::Win32 && !crate::policy::is_terminal_exe(...)
}
```

`crates/openvikey-win/src/classify.rs:8-17` xếp `chrome.exe`, `msedge.exe`, `firefox.exe`, `Code.exe`, `Cursor.exe`, `Discord.exe`, `Slack.exe`, `WhatsApp.exe` vào `InjectProfile::Electron` → `allow_learning = false`.

Spec §7.2 khóa: `allow_learning=false` ⇒ không Boundary Assist, không Probation Auto.

**Hệ quả:** Facebook/Messenger/Zalo web, Gmail, Discord, Slack, VS Code — nơi tiếng Việt informal (`ko`, `dc`, `khogn`) xuất hiện dày nhất — sẽ không bao giờ có tính năng này. Spec không nhắc tới ràng buộc này.

**Nhận xét:** gate hiện tại đang trộn hai khái niệm khác nhau — "inject không tin cậy trên Electron" và "không được phép học". `allow_transform` vẫn `true` ở Electron. `ImplicitCorrection` không cần inject nên hoàn toàn có thể học ở Electron ngay cả khi tạm chưa cho Boundary Assist.

**Đề xuất:** tách `allow_learning` khỏi `InjectProfile`. Giữ gate riêng cho can thiệp chủ động (cần inject tin cậy) và gate riêng cho thu thập bằng chứng (không cần inject).

---

### A2. `RuleContextKey` quá thưa để bằng chứng tích lũy được

`crates/openvikey-core/src/model.rs:16-24` — key gồm cả `left_token_nfc`.
`crates/openvikey-session/src/document.rs:87-99` — `left_token` là **từ đứng ngay trước**.
`crates/openvikey-core/src/rank.rs:80-88` — dựng key với `left_token` nguyên vẹn, **không có back-off**.

Nghĩa là `khogn` sau `tôi` và `khogn` sau `rất` là hai rule khác nhau, không chia sẻ evidence.

Đối chiếu với ngưỡng của chính spec và code:

| Mốc | Yêu cầu | Số lần cần, trên **cùng một** cặp (typo, sửa, từ-đứng-trước) |
|---|---|---|
| Boundary Assist (spec §5.3) | positive mass ≥ 2.0 | 2 lần tự sửa |
| Learned Auto (`decision.rs:48`) | positive mass ≥ 18 | 18 lần tự sửa, hoặc 60 lần `AutoSettled` |

Phân bố từ đứng trước trong tiếng Việt là heavy-tail: gõ sai `khogn` 50 lần sẽ rải ra hàng chục `left_token` khác nhau. Evidence không bao giờ đạt 2.0, chứ chưa nói 18.

**Đây mới là nguyên nhân gốc của cold-start — không phải `Ctrl+.`.** Spec §3.3 lại khóa chặt điều này bằng câu *"Không tạo key giả"*.

**Đề xuất:** key phân tầng có back-off.

- Key chính: `(input_method, source, original_nfc, candidate_nfc, source_rule_id)` — nơi mass tích lũy.
- `left_token_nfc`: tầng refinement, chỉ cộng thêm khi đã đủ dữ liệu riêng, hoặc dùng làm prior.

Đây là smoothing chuẩn trong mô hình thống kê, không phải "key giả": mọi tầng vẫn ánh xạ về candidate/rule có thật.

---

### A3. Quality gate §11.2 không có đường đi tới — Probation Auto chết yểu

Hai lý do độc lập:

**1. Thống kê.** Wilson lower bound 95% với 0 lỗi:

| Ngưỡng spec | Số quan sát sạch tối thiểu |
|---|---:|
| Probation Auto precision ≥ 99.7% | `n ≥ ~1.280` |
| Boundary Assist precision ≥ 99.5% | `n ≥ ~765` |

Budget của spec §6.3 là 3 probe/session + 1 probe/rule/24h → cần cỡ **430 phiên sạch** để chứng minh Probation Auto bằng dữ liệu sống.

**2. Không có dữ liệu để replay.** `data/fixtures/corpus/` hiện có tổng cộng **7 dòng** (`train` 3, `held_out` 3, `calibration` 1).

Spec §6.3 fail-closed nếu chưa chứng minh → `probation_enabled = false` vĩnh viễn. §5.3 cũng nói ngưỡng Boundary Assist *"phải được đo lại bằng corpus/replay trước khi bật mặc định"* → Giai đoạn B cũng bị chặn theo.

**Vấn đề sâu hơn — sai loại metric.** Precision 99.7% là bar đúng cho *auto sửa im lặng, khó hoàn tác*. Nhưng Boundary Assist là **nhìn thấy được và hoàn tác bằng 1 phím**. Metric đúng là *regret có trọng số chi phí*, không phải precision thô. Ví dụ dạng đúng hơn:

```text
≤ 1 can thiệp không mong muốn / 500 token   AND   chi phí hoàn tác ≤ 1 phím
```

Giữ nguyên 99.7% là nhầm lẫn giữa hai chế độ rủi ro khác nhau.

---

## 2. Nhóm B — Sai về hành vi

### B1. Backspace va chạm với thao tác xóa dấu cách thông thường

Spec §5.4: sau `không␠|`, Backspace = semantic undo + `Undo +1.5`.

Nhưng "Backspace ngay sau khi vừa gõ space" cũng chính là thao tác xóa dấu cách thừa — rất phổ biến. Điều kiện §5.4 (*"chưa có ký tự mới sau delimiter"*) không phân biệt được hai ý định; nó chính là trạng thái chung của cả hai.

**Hệ quả:** kênh bằng chứng **âm** bị nhiễu nặng hơn kênh dương, và lại nặng hơn 1.5 lần.

- Kênh dương (`ImplicitCorrection`, §4.3 điều kiện 5) đòi Y phải khớp candidate thật — tiêu chuẩn chứng minh cao.
- Kênh âm chỉ cần một phím Backspace mơ hồ.

Bất đối xứng sai hướng.

**Đề xuất:** giữ nguyên hành vi khôi phục text (đúng UX), nhưng áp cùng chuẩn chứng minh cho cả hai kênh — chỉ ghi `Undo` mạnh khi người dùng **commit lại dạng original** sau khi khôi phục. Nếu họ chỉ gõ tiếp bình thường thì ghi `Undo` yếu hoặc không ghi.

### B2. Chưa định nghĩa trạng thái composition sau khi khôi phục

Spec §5.4 nói kết quả là `ko|` nhưng không nói `ko` nằm ở đâu: committed text hay composition buffer?

Code hiện tại đi đường committed — `crates/openvikey-session/src/session.rs:307` (`undo_last_with_learning` → `document.replace_last_token`). Raw keys bị mất.

**Hệ quả:** sau undo, gõ tiếp `s` sẽ không tạo được tổ hợp Telex trên `ko` — engine chỉ thấy một ký tự mới. Người dùng bị kẹt giữa hai trạng thái: hệ thống vừa rút lại phép sửa của nó, vừa không cho gõ tiếp bình thường.

**Đề xuất:** spec phải có contract rõ — undo khôi phục **composition buffer với `raw_keys` gốc**, không chỉ text hiển thị.

### B3. Dấu câu cuối câu + Enter trong app chat

Spec §5.1 loại Enter vì "nguy cơ gửi trước khi kịp nhận ra" — lập luận đúng.

Nhưng `khogn.` + Enter trong Messenger/Discord có cùng rủi ro: assist bắn ở `.`, Enter cách đó ~100ms. Cửa sổ Undo trên thực tế bằng không.

**Đề xuất:** với profile Electron/chat, giới hạn Boundary Assist ở **Space**, không dùng `.` `,` `?` `!`. Đã có sẵn `profile_for_exe` để phân biệt.

---

## 3. Nhóm C — Rủi ro mô hình

### C1. `AutoSettled` tự khuếch đại, không có trần

`crates/openvikey-core/src/model.rs:161-167` — `AutoSettled` cộng `+0.3` không giới hạn tổng.

60 lần settle → đủ mass 18 → promote lên learned Auto **mà không có một hành động xác nhận nào của người dùng**.

Spec §3.2 nói đúng rằng đây là "tín hiệu dương yếu", nhưng không đặt trần. Vòng lặp tự củng cố: can thiệp → người dùng không để ý → settle → mass tăng → can thiệp thêm. Một phép sửa sai mà người dùng *chịu đựng* sẽ bị khóa cứng thành Auto.

**Đề xuất:** trần cứng cho mass đến từ settlement — ví dụ không quá 40% ngưỡng promote; phần còn lại bắt buộc đến từ `Accept` hoặc `ImplicitCorrection`.

### C2. Settlement 10 event không có chiều thời gian

`crates/openvikey-core/src/feedback.rs:83` — `remaining_events: 10`.

Gõ nhanh thì 10 event ≈ 2 giây, mắt chưa kịp tới chỗ vừa sửa. Kết hợp với C1, hệ thống hội tụ về *"người dùng gõ nhanh"* chứ không phải *"người dùng đồng ý"*.

**Đề xuất:** thêm điều kiện thời gian tối thiểu. `at_ms` do caller cấp đã có sẵn nên vẫn deterministic, không cần đọc wall clock trong core.

### C3. Mâu thuẫn nhỏ — `SuggestionSettled`

Spec §3.2 ghi "suggestion bị bỏ qua → không phát event → mass `0`". Đúng với runtime hiện tại (session không bao giờ phát nó), nhưng `model.rs:162-167` vẫn cài `-0.2`.

**Đề xuất:** hoặc xóa variant, hoặc spec ghi rõ "cố ý không phát, giữ để tương thích replay".

### C4. Không có cách nhìn / quên cái đã học

Spec §2.2 loại settings GUI — hợp lý cho v1. Nhưng khi hệ thống bắt đầu tự sửa, người dùng cần trả lời được "nó đã học gì" và "quên cái vừa rồi đi".

Hiện chỉ có đường 2-lần-Undo (`model.rs:150-156`). Tối thiểu nên có: một lệnh dump model local, và một hotkey "quên rule vừa áp dụng".

---

## 4. Ý kiến khác về phương pháp tự học

Phần này **không đồng ý** với cách tiếp cận của spec.

### 4.1 Đừng học cái vốn không cần học

Spec đối xử với `TelexFix`, `Fuzzy`, `Abbreviation` như cùng một bài toán, đều phải leo thang bằng chứng cá nhân. Nhưng chúng khác bản chất:

| Loại | Ví dụ | Bản chất |
|---|---|---|
| Sự thật máy móc | `khogn → không` | Vị trí dấu Telex. Không phải sở thích. Không cần bằng chứng cá nhân. |
| Sở thích cá nhân | `ko → không`, `dc → được` | Có người cố ý gõ `ko`. Cần bằng chứng cá nhân thật. |

Toàn bộ bộ máy Probation Auto — cooldown 24h, budget 3/session, quality gate 99.7%, schema migration cho `last_probe_at_ms` — được dựng chủ yếu để bootstrap `TelexFix`, thứ đáng lẽ chỉ cần một gate tất định:

```text
kết quả ∈ lexicon
  ∧ original ∉ lexicon
  ∧ chỉ có 1 candidate
  ∧ syllable-shape hợp lệ
→ auto, bật/tắt theo source, mặc định bật
```

**Đề xuất:** bỏ Probation Auto khỏi v1. Giữ tầng học **chỉ cho `Fuzzy`/`Abbreviation`**, nơi bằng chứng cá nhân thực sự có nghĩa. Cắt giảm lớn về diện tích triển khai lẫn rủi ro schema, mà vẫn giải đúng nhu cầu chính.

### 4.2 Dữ liệu quý nhất đang bị vứt đi

Spec §4.3 điều kiện 5: nếu Y không khớp candidate nào → *"không mutate model"*. Code hiện tại cũng vậy (`session.rs:585-590`, `find(...)` fail → return sớm).

Nhưng đó chính là mỏ vàng: người dùng vừa trả giá bằng nhiều Backspace + gõ lại để có một từ mà **engine hoàn toàn không nghĩ ra**. Đó là điểm mù của engine, là từ vựng riêng, tên riêng, cách viết tắt của riêng người dùng.

Ném đi 100% dữ liệu này rồi dựng Probation Auto để "phá cold-start" là ngược.

**Đề xuất:** ghi cặp `(X → Y)` không-khớp-candidate vào một *personal correction store* riêng (không mutate rule mass, không tạo key giả). Sau `k` lần lặp lại, promote thành candidate source mới (`Personal`). Đây là đường bootstrap rẻ, an toàn, và đúng nghĩa "tự học" hơn hẳn probe.

### 4.3 Cân lại trọng số `ImplicitCorrection`

`model.rs:139` — `ImplicitCorrection` đang bằng `Accept` (`+1.0`).

Nhưng nó mạnh hơn: người dùng đã bỏ ra 5–6 phím để đạt kết quả đó, trong khi `Ctrl+.` chỉ tốn 1 phím và đôi khi bấm theo quán tính.

**Đề xuất:** `ImplicitCorrection ≥ 2.0`.

---

## 5. Những gì spec làm đúng — giữ nguyên

- **§3.1** thang độ mạnh tín hiệu — nền tảng đúng, là phần giá trị nhất của spec.
- **§7.1** tách `DecisionState` khỏi `InterventionMode` — quyết định kiến trúc chuẩn xác, tránh trộn "đã học tới đâu" với "phân phối thế nào".
- **§9.2** chọn phương án 2 (`InterventionApplied` có identity trong capture) thay vì replay theo config hash — đúng, giữ replay bất biến khi calibration đổi.
- Loại `Diacritics` khỏi mọi can thiệp chủ động — đúng, `khong` có thể là "không / khổng / khống".
- Loại `Abbreviation` khỏi probe cold-start — đúng, `ko` / `dc` có thể là chủ ý.
- **§11.3** giữ nguyên ràng buộc hook path (không lock blocking / sleep / I/O / serialize) — không được nhượng bộ.
- **§4** máy trạng thái edit transaction rõ ràng hơn `ImplicitCorrectionMiner` hiện tại — đúng hướng, nên làm.

---

## 6. Thứ tự triển khai đề nghị

Khác với §12 của spec: hai việc chặn cứng phải đi trước, nếu không Giai đoạn A sẽ thu evidence vào một cấu trúc không bao giờ đạt ngưỡng, ở những app mà learning đang bị tắt.

| # | Việc | Giải quyết |
|---:|---|---|
| 0 | Tách `allow_learning` khỏi `InjectProfile::Electron` | A1 — gỡ chặn |
| 1 | Key phân tầng + back-off | A2 — làm evidence tích lũy được |
| 2 | Implicit Correction v2 + personal correction store | Giai đoạn A của spec + §4.2 |
| 3 | `TelexFix` tất định, bỏ probation cho source này | §4.1 — phá cold-start không cần probe |
| 4 | Boundary Assist cho `Fuzzy`/`Abbreviation` | Giai đoạn B, ngưỡng đo bằng corpus thật |
| 5 | Corpus tiếng Việt thật (hiện 7 dòng fixtures) | Điều kiện tiên quyết cho §11.2 |
| — | Probation Auto | **Hoãn khỏi v1** |

Đồng thời sửa các mục nhóm B/C khi chạm vào phần tương ứng:

- B1, B2 → khi làm Boundary Assist undo path (bước 4).
- B3 → khi làm profile gate (bước 0 và 4).
- C1, C2 → khi chạm `model.rs` / `feedback.rs` (bước 1).
- C3, C4 → dọn dẹp độc lập, chi phí thấp.

---

## 7. Câu hỏi còn mở

1. Gate `allow_learning = profile == Win32` ở `host.rs:519` là quyết định bảo thủ có chủ ý (do inject Electron chưa tin cậy), hay là hệ quả không mong muốn? ADR 0007 / spec GD2a không nêu lý do.
2. Corpus tiếng Việt thật sẽ lấy từ đâu, và license/provenance ra sao? `data/corpus-manifest.toml` có khung kiểm tra nhưng chưa có dữ liệu.
3. Nếu chấp nhận key phân tầng (A2), có cần bump `MODEL_VERSION` và migration v1→v2 không — hay back-off tính được từ evidence v1 hiện có?
