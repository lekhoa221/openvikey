# OpenViKey — Thiết kế refactor hệ thống học v2

- **Ngày:** 2026-08-20
- **Trạng thái:** v1 — accepted (2026-08-21)
- **Implementation plan:** [`../plans/2026-08-21-openvikey-learning-model-v2-implementation-plan.md`](../plans/2026-08-21-openvikey-learning-model-v2-implementation-plan.md)
- **Phạm vi:** `openvikey-core`, `openvikey-session`, `openvikey-lab`, `openvikey-win`
- **Ghi chú nền:** [`../../research/2026-08-20-learning-refactor-notes.md`](../../research/2026-08-20-learning-refactor-notes.md)
- **Spec nền:** [`2026-08-17-openvikey-design.md`](./2026-08-17-openvikey-design.md)
- **Spec hành vi hiện tại:** [`2026-08-18-openvikey-composition-rewind-learning-design.md`](./2026-08-18-openvikey-composition-rewind-learning-design.md)
- **Ràng buộc giao diện core:** [ADR 0002](../../decisions/0002-wave0-interfaces.md)

---

## 0. Tóm tắt quyết định đề xuất

OpenViKey sẽ tiếp tục là hệ thống học thích nghi nhỏ, chạy tại máy, giải thích được và không cần backend. Refactor không thay engine Telex/VNI và không đưa mạng nơ-ron vào sản phẩm.

Hệ thống đích có bốn tầng:

```text
Bộ sinh phương án cố định
→ Sổ ghi nhớ từng phép sửa
→ Bộ thống kê thói quen dùng từ
→ Bộ quyết định Bỏ qua / Gợi ý / Tự sửa
```

Các quyết định chính:

1. Chỉ có **một bộ quyết định** cho mọi can thiệp. Không còn một đường tự sửa do model và một đường tự sửa đặc biệt nằm ngoài model.
2. Tách hai câu hỏi:
   - “Người dùng có muốn `X → Y` không?” — sổ ghi nhớ phép sửa.
   - “Trong ngữ cảnh này từ nào hợp hơn?” — thống kê từ/cặp từ.
3. Thống kê ngữ cảnh chỉ được sinh hoặc xếp hạng gợi ý. Nó không được tự mình cho phép tự sửa.
4. Hoàn tác tức thời trước hết là quay lui giao dịch vừa xảy ra. Hoàn tác và từ chối lâu dài là hai tín hiệu khác nhau.
5. Ngữ cảnh được học theo hai tầng: mức toàn cục và mức từ đứng trước. Không cộng dồn mù quáng mọi ngữ cảnh và không lấy trạng thái cao nhất làm kết luận chung.
6. Cặp sửa cá nhân được đưa vào cùng vòng đời với các phép sửa khác, nhưng vẫn bị giới hạn tối đa ở Gợi ý.
7. “Quên” phải xoá vật lý dữ liệu liên quan khỏi model và phần lịch sử có thể tái tạo model.
8. Dữ liệu học có giới hạn toàn cục, có dọn dữ liệu cũ/yếu và có phép nén bằng chứng mà không đổi kết quả tính toán.
9. Luật bảo mật, giới hạn từng nguồn sửa và khả năng hoàn tác là rào chắn cố định; dữ liệu người dùng không được vượt qua các rào chắn này.
10. Học kiểu lỗi tổng quát chỉ được làm sau, chạy quan sát trước và không tự sửa trong lần phát hành đầu.
11. Token phải có ít nhất **2 ký tự chữ Unicode** mới được sinh hành vi Gợi ý/Tự sửa ở correction pipeline. Đây là luật bắt buộc, không phải sở thích tự học; engine Telex/VNI một ký tự vẫn hoạt động bình thường.
12. Undo một replacement phải bật chốt chống lặp: cùng raw token không được bị thay lại ngay khi nhấn Space. Cửa sổ nghỉ nhanh mặc định là 3 giây và lần boundary kế tiếp của token chưa đổi luôn được commit nguyên bản một lần.
13. Settings/Learning phải có biểu đồ local để xem evidence, confidence, trạng thái, cooldown và phần đóng góp của từng loại điểm theo thời gian.

---

## 1. Thuật ngữ dùng trong spec

| Thuật ngữ | Nghĩa đơn giản |
|---|---|
| Phương án sửa (`Candidate`) | Một kết quả mà hệ thống có thể đề xuất, ví dụ `không` cho `khogn` |
| Bằng chứng (`Evidence`) | Hành vi cho thấy người dùng đồng ý hoặc không đồng ý |
| Phép sửa chính xác | Một cặp cụ thể `từ gốc → từ thay thế` |
| Ngữ cảnh | Từ đứng ngay trước từ đang xét; giai đoạn đầu chỉ dùng tối đa một từ |
| Bộ thống kê từ | Bộ đếm từ nào và cặp từ nào người dùng thường dùng |
| Bộ quyết định | Nơi duy nhất chọn Bỏ qua, Gợi ý hoặc Tự sửa |
| Giao dịch | Một thay đổi có thể quay lui nguyên trạng, gồm cả chữ và phần học vừa ghi |
| Chạy quan sát | Tính thử kết quả nhưng không thay đổi chữ người dùng nhìn thấy |
| Tham số chung | Trọng số/ngưỡng áp dụng cho mọi người dùng, cố định theo phiên bản |
| Trạng thái cá nhân | Số lần đồng ý, từ chối, hoàn tác, tần suất từ và thời điểm sử dụng của một người |

Tên kiểu và module trong code vẫn dùng tiếng Anh theo quy ước Rust. Giao diện sản phẩm dùng từ tiếng Việt dễ hiểu.

---

## 2. Vấn đề cần giải quyết

### 2.1 Hai con đường tự sửa không thống nhất

Hiện tại một phép sửa có thể được áp dụng vì:

1. model đã đủ điểm để lên `Auto`; hoặc
2. `boundary_assist_candidate` cho rằng chỉ có một phương án an toàn.

Đường thứ hai có thể đặt hành vi hiện tại thành Tự sửa nhưng trạng thái lưu trong model vẫn là Gợi ý. Người dùng và giao diện không thể trả lời chính xác “vì sao chữ vừa bị sửa”.

### 2.2 Hai loại bộ nhớ có vòng đời khác nhau

`AdaptiveModel` có điểm tốt/xấu, suy giảm theo thời gian, undo và trạng thái. `PersonalCorrectionStore` chỉ đếm đủ hai lần rồi tạo phương án cá nhân. Cặp cá nhân không có suy giảm, điểm xấu, ngữ cảnh hay cơ chế dọn dữ liệu tương đương.

### 2.3 Ngữ cảnh được lưu nhưng bị gộp hoàn toàn

`RuleContextKey` có `left_token_nfc`, nhưng phép tính hiện tại bỏ qua trường này khi cộng điểm và lấy trạng thái cao nhất giữa các dòng. Evidence bị chia nhỏ trong file nhưng lại bị gộp toàn bộ khi ra quyết định. Đây không phải học riêng theo ngữ cảnh cũng không phải cơ chế quay về mức toàn cục có kiểm soát.

### 2.4 Ý nghĩa hoàn tác chưa tách khỏi không ưa thích

Backspace ngay sau Space có thể là:

- phản đối phép sửa;
- chỉ xoá dấu cách thừa;
- tiếp tục chỉnh từ;
- đổi ý về cả câu.

Nếu luôn coi đó là điểm xấu mạnh, model có thể học sai. Tuy nhiên text vẫn phải được khôi phục ngay để bảo vệ người dùng.

### 2.5 Vòng đời dữ liệu chưa đầy đủ

- `forget_rule()` hiện ẩn rule khỏi inspection nhưng vẫn giữ chuỗi trong `RuleEntry` đã serialize.
- Chưa có giới hạn tổng số adaptive rule.
- Hết 512 personal pair thì từ chối học pair mới thay vì thay row cũ/yếu.
- Cắt event cũ ở mốc 512 làm điểm thay đổi đột ngột.
- Capture có thể còn dữ liệu đủ để lộ hoặc tái tạo nội dung đã “quên”.

### 2.6 Chưa có bộ học thói quen dùng từ

OpenViKey biết người dùng có thích một phép sửa chính xác hay không, nhưng chưa học tốt các câu hỏi như:

```text
Sau “cảm” thường là “ơn”
Sau “Việt” thường là “Nam”
```

Lexicon và bigram hiện là dữ liệu tĩnh, không phải thói quen cá nhân tích luỹ từ các lần commit an toàn.

### 2.7 Tham số chưa có quy trình hiệu chỉnh đầy đủ

Các trọng số `+1.0`, `-1.5`, half-life 30 ngày và ngưỡng hiện tại là cấu hình thiết kế ban đầu. Corpus release vẫn chưa đạt gate G3. Không được tăng độ phức tạp của công thức trước khi có quy trình đo, phiên bản hoá và so sánh rõ ràng.

### 2.8 Gợi ý xuất hiện quá sớm với token một ký tự

Correction pipeline chưa có một product invariant chung rằng token phải đủ dài mới được gợi ý. Raw Telex/VNI có thể chứa nhiều phím nhưng chỉ tạo một chữ hiển thị, ví dụ `dd → đ` hoặc `a1 → á`; không được lấy số raw key làm độ dài từ.

Yêu cầu bắt buộc của v2: chỉ token có ít nhất hai grapheme chữ Unicode trong `snapshot.normalized` mới được hiển thị suggestion hoặc correction intervention. Luật này không cản engine biến đổi một chữ Telex/VNI thông thường.

### 2.9 Undo tạo vòng lặp thay thế vô tận

Hành vi hiện tại có thể lặp:

```text
khogn6| + Space → không |
Backspace       → khogn6|
Space           → không |
Backspace       → khogn6|
...
```

Nguyên nhân: Backspace khôi phục raw composition nhưng planner không giữ một chốt “người dùng vừa huỷ đúng replacement này”. Cùng token lập tức thỏa lại policy auto ở Space tiếp theo.

V2 cần một chốt chống lặp ngắn hạn theo đúng correction identity + raw token + focus/composition. Undo trong cửa sổ 3 giây phải khôi phục original và buộc boundary kế tiếp của token chưa đổi commit original một lần, không áp lại replacement.

### 2.10 Thiếu cách quan sát mô hình bằng biểu đồ

Khi model có decay, context blending, score margin và nhiều loại evidence, chỉ hiển thị tổng `+P/-N` không đủ để hiểu hệ thống. Người phát triển và người dùng nâng cao cần nhìn được đường confidence, sự kiện accept/revert, vùng Suggest/Auto, cooldown và score breakdown mà không phải đọc JSON.

---

## 3. Mục tiêu và phạm vi loại trừ

### 3.1 Mục tiêu

- Một nơi duy nhất giải thích và quyết định mọi Gợi ý/Tự sửa.
- Giữ đường gõ tất định, nhanh và không I/O.
- Học phép sửa chính xác bằng tín hiệu có ý nghĩa rõ.
- Dùng ngữ cảnh nhưng không để một ngữ cảnh ít dữ liệu lấn át toàn cục.
- Học tần suất từ/cặp từ local để xếp hạng gợi ý tốt hơn.
- Mọi can thiệp chủ động hoàn tác được cả text và thay đổi học chưa chốt.
- “Quên” thực sự loại dữ liệu khỏi payload bền vững.
- Model có giới hạn, dọn dữ liệu và migration không mất evidence.
- Token dưới hai ký tự chữ không tạo correction suggestion/intervention.
- Undo replacement không thể rơi vào vòng lặp Space → replace → Backspace → restore → Space.
- Có biểu đồ local giải thích điểm, confidence, context và trạng thái theo thời gian.
- Cùng input, model, config và thời gian do caller cấp phải cho cùng kết quả.
- Không học/capture trong ngữ cảnh nhạy cảm, terminal hoặc khi policy chặn.
- Có đường triển khai từng lát nhỏ; mỗi lát giữ workspace hoạt động.

### 3.2 Không làm trong refactor này

- Không thay engine Telex/VNI.
- Không dùng mạng nơ-ron, dịch vụ đám mây, tài khoản hoặc telemetry.
- Không dùng mô hình tự thử nghiệm bằng cách cố ý sửa ngẫu nhiên chữ người dùng.
- Không tự khôi phục dấu cả câu/cụm bằng tìm kiếm nhiều bước.
- Không cho thống kê từ/cặp từ tự quyết định Auto.
- Không thêm học theo app/domain trong schema v2 đầu tiên.
- Không tự import lịch sử trình duyệt, tài liệu hay clipboard.
- Không thay thế corpus/license gate hiện có.
- Không triển khai đồng bộ đa máy trong spec này.
- Không sao chép code/data GPL hoặc dữ liệu chưa rõ giấy phép từ prior art.

---

## 4. Nguyên tắc kiến trúc

### 4.1 Tách dữ liệu tĩnh và dữ liệu cá nhân

- Engine, lexicon nền, generator và score nền là dữ liệu tĩnh/versioned.
- Sổ phép sửa và thống kê từ là dữ liệu cá nhân/versioned.
- Xoá toàn bộ dữ liệu cá nhân phải trả hệ thống về hành vi cold-start có thể dự đoán.

### 4.2 Tham số chung không tự thay đổi theo từng người

Người dùng chỉ cập nhật trạng thái cá nhân. Các giá trị như trọng số tín hiệu, thời gian suy giảm, ngưỡng và hệ số trộn được hiệu chỉnh ngoại tuyến, ghi phiên bản/hash và thay đổi qua release có migration rõ ràng.

### 4.3 Luật an toàn đứng ngoài model

Model không thể học để vượt qua:

- `allow_transform=false`;
- `allow_learning=false` đối với mutation;
- password/PIN/denylist;
- source cap;
- edit range/revision không hợp lệ;
- từ nguồn Diacritics/Personal bị giới hạn Gợi ý;
- yêu cầu hoàn tác được đối với mọi can thiệp.

### 4.4 Tín hiệu yếu không được tự khuếch đại vô hạn

Việc một auto tồn tại mà chưa bị undo chỉ là tín hiệu yếu. Tổng ảnh hưởng của loại tín hiệu này có trần; một rule không được lên learned Auto chỉ nhờ nhiều settlement thụ động.

### 4.5 Không kết luận từ sự im lặng

Gợi ý được hiển thị nhưng không chọn là dữ liệu “đã nhìn thấy”, không mặc định là từ chối. Impression được dùng cho inspection, dọn dữ liệu hoặc nghiên cứu offline; chưa cộng điểm xấu vào confidence ở v2.

### 4.6 Độ dài tối thiểu là luật sản phẩm

`minimum_correction_graphemes = 2` là guard cố định của correction pipeline v2. Model không học để hạ guard này và Settings không cho người dùng đặt về 1. Độ dài được tính trên grapheme có ký tự chữ trong text NFC đã render, không tính phím dấu/raw modifier.

---

## 5. Kiến trúc đích

```text
CompositionSnapshot + LeftContext
        │
        ▼
┌────────────────────────────┐
│ 1. Candidate generators    │  Thuần, không đọc model
│ TelexFix/Fuzzy/Abbrev/...  │
└─────────────┬──────────────┘
              ▼
┌────────────────────────────┐
│ 2. Candidate assessment    │  Điểm nền + thống kê từ/ngữ cảnh
│ rank + score breakdown     │
└─────────────┬──────────────┘
              ▼
┌────────────────────────────┐
│ 3. Exact correction memory│  Đồng ý/từ chối/hoàn tác/cooldown
└─────────────┬──────────────┘
              ▼
┌────────────────────────────┐
│ 4. Intervention planner    │  Nơi duy nhất ra quyết định
│ None/Suggest/Auto + reason │
└─────────────┬──────────────┘
              ▼
     semantic action + transaction
              │
              ▼
 feedback / settle / rollback / persist
```

### 5.1 Trách nhiệm module

| Thành phần | Trách nhiệm |
|---|---|
| `engine` | Phím → composition Telex/VNI; không biết model |
| `generate` | Sinh phương án thuần từ snapshot/lexicon |
| `rank` | Dedupe, điểm nền, thêm ảnh hưởng ngữ cảnh và exact preference có giới hạn |
| `correction_memory` | Lưu/đọc trạng thái từng phép sửa và context refinement |
| `user_language` | Lưu thống kê từ/cặp từ output đã commit an toàn |
| `intervention` | Áp guard và chọn None/Suggest/Auto kèm lý do |
| `feedback` | Quản lý giao dịch, settlement, rollback và evidence |
| `session` | Kết nối engine → candidate → planner → document → feedback |
| adapter OS | Cấp context/safety, áp semantic action, không tự ra quyết định học |

Tên module cuối cùng có thể tinh chỉnh trong implementation plan, nhưng ranh giới trách nhiệm là bắt buộc.

---

## 6. Sổ ghi nhớ phép sửa chính xác

### 6.1 Định danh toàn cục

Định danh chính không chứa từ đứng trước:

```text
CorrectionIdentity {
  input_method,
  source,
  original_nfc,
  candidate_nfc,
  source_rule_id
}
```

Hai cặp original/candidate khác nhau không chia sẻ evidence. Telex và VNI vẫn tách biệt vì lỗi raw-key khác nhau.

### 6.2 Định danh ngữ cảnh

```text
CorrectionContext {
  correction_identity,
  left_token_nfc: Option<String>
}
```

Mỗi feedback được ghi một lần vào thống kê toàn cục và một lần vào bucket ngữ cảnh tương ứng. Khi query, hai thống kê được **trộn theo độ hỗ trợ**, không cộng lần nữa.

Công thức khái niệm:

```text
context_weight = context_support / (context_support + shrinkage_k)
confidence = context_weight × context_confidence
           + (1 - context_weight) × global_confidence
```

- Context chưa có dữ liệu → dùng toàn cục.
- Context có ít dữ liệu → nghiêng về toàn cục.
- Context có đủ dữ liệu → ảnh hưởng riêng tăng dần.
- `shrinkage_k` là tham số chung có phiên bản, không tự đổi theo user.

### 6.3 Trạng thái lưu

Mỗi correction lưu tối thiểu:

```text
- positive/negative summary toàn cục
- summaries theo context có giới hạn
- last_feedback_at_ms
- last_used_at_ms
- recent intervention outcomes
- handled sequence/edit identities có giới hạn
- explicit suppression
- cooldown/demotion marker
- personal probation count nếu source = Personal
```

`Ignore/Suggest/Auto` được tính tại thời điểm query từ model + config + source policy. Không lấy `max Auto` từ một context rồi lưu như kết luận cho mọi context.

### 6.4 Nén evidence

Thay vì bỏ thẳng event cũ ở mốc 512, mỗi bucket có summary:

```text
decayed_positive_at_checkpoint
decayed_negative_at_checkpoint
checkpoint_at_ms
recent_events[]
```

Khi nén tại thời điểm caller truyền vào:

1. suy giảm evidence cũ về checkpoint;
2. cộng vào summary;
3. giữ một cửa sổ event gần đây đủ cho idempotency/undo/audit;
4. query tương lai tiếp tục suy giảm summary từ checkpoint.

Cùng event và checkpoint phải cho cùng kết quả trước/sau nén trong sai số số thực đã định nghĩa bằng test.

### 6.5 Personal pair

Cặp sửa chưa có trong candidate snapshot được tạo dưới source `Personal` trong cùng store:

- lần quan sát mạnh đầu tiên → probation, chưa sinh candidate;
- lần quan sát độc lập thứ hai → được phép sinh Gợi ý;
- không bao giờ Auto trong model v2;
- có positive/negative, decay, suppression, context và forget như rule khác;
- không dùng một `PersonalCorrectionStore` có vòng đời riêng sau migration.

“Hai lần độc lập” nghĩa là hai correction transaction khác `seq/edit/session anchor`, không phải replay trùng.

---

## 7. Ý nghĩa feedback và giao dịch

### 7.1 Bảng tín hiệu mục tiêu

Các trọng số dưới đây giữ mặc định hành vi code hiện tại trong lát migration đầu; calibration có thể thay đổi bằng config version mới.

| Hành vi | Ý nghĩa | Thay đổi mặc định |
|---|---|---:|
| Nhận gợi ý rõ ràng | đồng ý mạnh | positive `+1.0` |
| Từ chối rõ ràng | không muốn phép sửa này | negative `+1.0` |
| Xoá X rồi gõ candidate Y | ý định sửa mạnh | positive `+1.5` |
| Auto tồn tại đủ cửa sổ | đồng ý yếu | positive `+0.3` |
| Undo bằng hotkey dành cho Auto | phản đối rõ | negative `+1.5` |
| Backspace tức thời | yêu cầu quay lui trước, ý định dài hạn chưa chắc | rollback + marker, chưa cộng negative mạnh |
| Commit lại original sau rollback | xác nhận không muốn correction | negative `+1.5` |
| Gợi ý hiện nhưng không chọn | không đủ kết luận | `0`, chỉ tăng impression |

### 7.2 Backspace tức thời và chốt chống vòng lặp

Mỗi replacement giữ một `immediate_revert_window_ms`, mặc định **3.000 ms** từ lúc replacement được áp. Nếu Backspace xảy ra trong cửa sổ này, intervention được xem là edit gần nhất để semantic revert khi cùng focus/caret/revision và chưa bắt đầu token mới.

Khi semantic revert:

1. khôi phục raw composition/original chính xác;
2. rollback mọi cập nhật thống kê từ chưa settlement;
3. huỷ pending settlement;
4. tạo `RevertGuard` gắn với correction identity, raw token, focus generation và composition revision;
5. đặt `reapply_cooldown_until_ms = undo_at_ms + 3.000`;
6. đặt `bypass_next_boundary = true`;
7. chưa cộng negative mạnh chỉ dựa vào một Backspace.

Hành vi bắt buộc của `RevertGuard`:

```text
khogn6| + Space → không |
Backspace       → khogn6|   (arm guard)
Space           → khogn6 |  (commit original, không replace)
```

- Trong 3 giây cooldown, planner loại chính correction vừa bị huỷ khỏi cả Replace và overlay để không nháy lại cùng gợi ý. Candidate khác nếu có chỉ được Suggest, không Auto.
- Dù 3 giây đã hết, boundary đầu tiên của **raw token chưa đổi** vẫn bypass mọi replacement một lần. Điều này loại vòng lặp ngay cả khi người dùng dừng suy nghĩ lâu hơn 3 giây.
- Sau khi boundary đó commit original, guard được xoá.
- Nếu người dùng thay đổi raw token, đổi focus/caret/method/mode hoặc Reset, guard được xoá vì đây là ý định mới.
- Guard ngắn hạn thuộc session, không cần persist qua restart.
- Hai revert cùng correction trong cửa sổ lịch sử bounded vẫn tạo cooldown/demotion dài hạn để tránh gây phiền ở lần gõ sau.

Nếu người dùng commit lại original trong cùng correction transaction, mới phát feedback âm mạnh. Immediate Backspace vẫn là rollback trước, không tự động đồng nghĩa với reject lâu dài.

### 7.3 Undo tường minh

Hotkey “Hoàn tác tự sửa” thể hiện ý định rõ hơn Backspace thông thường:

- inverse text;
- rollback thống kê từ liên quan;
- negative exact correction;
- cập nhật cooldown/demotion;
- một edit không được feedback hai lần.

### 7.4 Settlement

Một intervention chỉ settlement khi:

- đã qua ít nhất 10 input/edit event hợp lệ;
- đã qua ít nhất 3 giây caller-time;
- edit chưa undo/revert;
- focus/caret identity vẫn đủ để tin rằng text còn tồn tại;
- learning vẫn được phép.

Mỗi edit settlement đúng một lần. Tổng positive mass từ settlement cho một correction không vượt `weak_positive_cap`, mặc định giữ mức 7.2 hiện tại. Vì ngưỡng promote hiện là 18, settlement một mình không đủ tạo learned Auto.

### 7.5 Impression

Khi gợi ý thực sự hiển thị, model có thể tăng:

```text
shown_count
last_shown_at_ms
```

Khi được chọn:

```text
selected_count
```

Trong v2, tỷ lệ selected/shown chưa tác động trực tiếp đến Beta confidence. Nó phục vụ:

- tìm suggestion gây phiền để inspection;
- ưu tiên dọn row yếu;
- nghiên cứu/calibration offline;
- chuẩn bị explicit suppression sau này.

Variant `SuggestionSettled -0.2` cũ không được runtime phát. Migration phải giữ khả năng đọc capture cũ nhưng không tiếp tục tạo signal này.

---

## 8. Bộ thống kê thói quen dùng từ

### 8.1 Mục đích

Bộ này trả lời:

```text
Người dùng thường dùng từ nào?
Sau từ A, người dùng thường dùng từ B nào?
```

Nó không trả lời người dùng có thích phép sửa `X → Y` hay không.

### 8.2 Dữ liệu v2

Giai đoạn đầu chỉ có:

```text
Unigram { token_nfc, count, last_seen_at_ms }
Bigram  { left_nfc, token_nfc, count, last_seen_at_ms }
```

- Identity luôn là Unicode NFC đầy đủ.
- Không bỏ dấu để làm identity.
- Không tách theo Telex/VNI vì đây là thói quen output, không phải lỗi raw key.
- Chỉ dùng tối đa một từ trái trong v2; trigram là milestone sau.

### 8.3 Khi nào cập nhật

Được cập nhật khi:

- người dùng tự commit một token hợp lệ trong context cho phép learning;
- người dùng nhận suggestion rõ ràng;
- assisted/auto token đã settlement.

Không cập nhật bền vững ngay lúc Auto vừa được áp. Có hai cách hợp lệ:

1. trì hoãn đến settlement; hoặc
2. cập nhật trong transaction và rollback nguyên trạng khi undo.

Spec chọn cách 1 cho v2 vì đơn giản và giảm mutation không cần thiết.

### 8.4 Khi nào không cập nhật

- password/PIN/sensitive/terminal/no-learning;
- paste nhiều token không chứng minh được boundary;
- context/focus không chắc chắn;
- text chỉ mới là composition chưa commit;
- intervention vừa áp nhưng chưa settlement;
- raw keystroke, URL, secret-like surface hoặc token vượt giới hạn dữ liệu.

URL/code heuristic không thay thế sensitive classification; nó chỉ có thể chặn học để giảm nhiễu.

### 8.5 Xếp hạng

Bộ thống kê từ trả một tín hiệu ngữ cảnh có giới hạn. Rank kết hợp:

```text
điểm nền của generator
+ ảnh hưởng exact correction preference
+ ảnh hưởng unigram/bigram
+ tie-break tất định
```

Các tín hiệu không được giả vờ là cùng một xác suất nếu chưa calibration. Kết quả phải có `ScoreBreakdown` nội bộ để test/inspection biết phần đóng góp của từng nguồn.

Thống kê từ được phép:

- đổi thứ tự suggestion;
- hỗ trợ chọn top candidate;
- hỗ trợ kiểm tra khoảng cách giữa top 1 và top 2.

Thống kê từ không được:

- tạo Auto khi exact correction chưa đủ điều kiện;
- vượt explicit suppression/recent revert;
- làm Diacritics hoặc Personal vượt source cap.

### 8.6 Giới hạn dữ liệu ban đầu

Config mặc định đề xuất:

```text
max_unigrams = 10_000
max_bigrams = 30_000
```

Khi đầy, thay vì từ chối dữ liệu mới, dọn row theo thứ tự:

1. row hết hạn hoặc count rất thấp và cũ;
2. row chưa dùng lại;
3. row ít ảnh hưởng nhất;
4. giữ row gần đây/tần suất cao.

Các giới hạn này phải benchmark và có thể thay đổi bằng config version; không phải cam kết chất lượng cố định.

---

## 9. Bộ quyết định can thiệp thống nhất

### 9.1 Kết quả bắt buộc

Mọi lần đánh giá trả về:

```text
InterventionPlan {
  action: None | DisplaySuggestion | Replace,
  reason,
  candidate_id,
  score_breakdown,
  undo_contract,
}
```

`reason` là mã ổn định, tối thiểu gồm:

```text
NoCandidate
UnsafeContext
SourceSuggestOnly
LowScore
LowMargin
ExplicitlySuppressed
RecentRevertCooldown
SafeStructuralFix
UniqueHeuristicAssist
LearnedCorrection
ContextSupportedSuggestion
```

Reason chỉ dùng local test/UI nâng cao; production không log token.

### 9.2 Thứ tự guard

```text
1. Không được transform / sensitive / stale edit       → None
2. Token có dưới 2 grapheme chữ                         → None
3. Không có candidate hợp lệ                            → None
4. RevertGuard khớp correction + raw token              → chặn candidate đó; boundary commit original
5. Explicit suppression hoặc cooldown                   → tối đa Suggest
6. Source chỉ cho Suggest                               → tối đa Suggest
7. Edit không hoàn tác chính xác được                   → tối đa Suggest
8. Structural fix đủ guard                              → Replace
9. Learned correction đủ evidence + score + margin     → Replace
10. Heuristic assist được feature flag + quality gate  → Replace
11. Candidate đủ ngưỡng gợi ý                           → Suggest
12. Còn lại                                             → None
```

Độ dài token được tính bằng Unicode grapheme có ít nhất một ký tự alphabetic trong `snapshot.normalized`. Raw key Telex/VNI, combining mark và digit modifier không làm token một chữ vượt guard. Guard áp cho correction suggestion/intervention; `engine` vẫn compose `á`, `đ`, `ô` bình thường.

Không module nào ngoài planner được đổi `Suggest` thành `Auto` sau bước này.

### 9.3 Chính sách theo nguồn — đích v2

| Nguồn | Cold-start | Có thể tự sửa | Điều kiện chính |
|---|---|---|---|
| `TelexFix` | structural fix | Có | unique, âm tiết hợp lệ, original không lexicon, target có lexicon, hoàn tác được |
| `Fuzzy` | Gợi ý mặc định | Có sau học; heuristic riêng chỉ khi gate chất lượng bật | exact evidence, score, margin, original không hợp lệ |
| `Abbreviation` | Gợi ý | Có với viết tắt một từ sau evidence cá nhân | không cold-start auto; cụm nhiều từ Suggest-only |
| `Diacritics` | Gợi ý | Không trong v2 | source cap |
| `Personal` | probation rồi Gợi ý | Không trong v2 | hai correction độc lập + source cap |

Để refactor an toàn, lát planner đầu tiên phải mô phỏng hành vi hiện tại bằng config tương thích. Việc tắt cold-start auto của Abbreviation/Fuzzy là thay đổi policy riêng, chỉ bật sau khi test/UX được duyệt.

### 9.4 Score và khoảng cách ứng viên

Auto không chỉ kiểm top score. Nó còn cần:

- khoảng cách top 1 so với top 2;
- top candidate tốt hơn typed/original đủ rõ;
- lexicon/source validity;
- exact correction confidence/support;
- không có recent veto.

Ngưỡng cụ thể nằm trong `LearningConfigV2`, không hard-code trong generator hoặc session. Giá trị release phải được hiệu chỉnh trên calibration split, không chọn từ held-out split.

### 9.5 Trạng thái UI

UI hiển thị hành vi thực tế tại thời điểm query:

```text
Đang quan sát
Đang gợi ý
Có thể tự sửa
Đã tạm dừng vì bạn vừa hoàn tác
Đã bị chặn theo yêu cầu
```

Không hiển thị một state persisted đã lỗi thời hoặc không khớp planner.

---

## 10. Cấu hình và hiệu chỉnh tham số

### 10.1 Cấu hình versioned

```text
LearningConfigV2 {
  version,
  minimum_correction_graphemes: 2, // invariant, không cho user hạ
  immediate_revert_window_ms: 3_000,
  reapply_cooldown_ms: 3_000,
  source_policy,
  evidence_weights,
  decay,
  context_shrinkage,
  score_weights,
  suggest_thresholds,
  auto_thresholds,
  margin_thresholds,
  cooldowns,
  settlement,
  storage_limits,
}
```

Payload/capture ghi version và hash config đã dùng khi tạo quyết định cần replay.

### 10.2 Quy tắc thay đổi

- Đổi config không sửa ngược evidence lịch sử.
- Không cho model cá nhân tự thay `LearningConfigV2`.
- Mọi giá trị mới phải có lý do, test biên và báo cáo calibration.
- Nếu không đủ dữ liệu, policy rủi ro cao mặc định tắt.
- Có thể expose một lựa chọn UX “thận trọng / cân bằng” sau này, nhưng mỗi mức ánh xạ tới config đã kiểm thử, không phải slider tuỳ ý không đo được.

### 10.3 Quy trình hiệu chỉnh

1. Đóng băng train/calibration/held-out bằng hash và provenance.
2. Chọn trọng số/ngưỡng chỉ trên calibration hoặc replay development.
3. Chạy held-out đúng một cấu hình đã chốt.
4. Báo riêng Gợi ý, structural fix, heuristic assist và learned Auto.
5. Lưu config hash cùng report.
6. Không bật mặc định nếu corpus chưa đủ floor tương ứng.

---

## 11. Persistence, migration và quên dữ liệu

### 11.1 Model payload v2

Đây là thay đổi schema thật; phải tăng model payload version. Không dùng `#[serde(default)]` để âm thầm thay nghĩa v1.

Payload v2 chứa ba namespace:

```text
correction_memory
user_language_model
maintenance_metadata
```

Static lexicon/config không được nhúng lẫn vào personal state ngoài version/hash cần xác minh.

### 11.2 Migration v1 → v2

Migration tất định:

1. nhóm v1 entries theo `CorrectionIdentity` không có left token;
2. tính summary toàn cục từ toàn bộ evidence một lần;
3. tạo context summaries từ từng left-token bucket;
4. không cộng global và context vào cùng mass query; query dùng phép trộn §6.2;
5. chuyển Personal count/promoted sang source Personal trong correction memory;
6. giữ source cap Suggest cho Personal/Diacritics;
7. chuyển recent undo/demotion/idempotency còn hợp lệ;
8. không tự nâng hành vi rule sau migration;
9. nếu v2 guard không chứng minh Auto, rule v1 Auto được hạ về Suggest an toàn;
10. ghi v2 bằng coherent atomic save, giữ backup v1 cho recovery.

Migration không thay encrypted envelope của core/lab; chỉ thay payload bên trong. Windows development JSON tiếp tục theo ADR 0008 cho đến production storage gate.

### 11.3 Physical forget

“Quên một phép sửa” phải:

- remove correction global row;
- remove mọi context row;
- remove recent edit/settlement/idempotency metadata liên quan;
- remove explicit suppression nếu command yêu cầu reset hoàn toàn;
- remove Personal probation/promoted row tương ứng;
- rewrite/compact capture journal để dữ liệu đó không thể tái tạo lại;
- save coherent model/capture pair.

Test bắt buộc serialize model và capture rồi chứng minh các chuỗi original/candidate/left-context đã chọn không còn trong payload. Nếu token vẫn xuất hiện độc lập trong user language model do người dùng dùng nó như từ bình thường, UI phải phân biệt “quên phép sửa” với “xoá từ khỏi lịch sử dùng từ”.

### 11.4 Các lệnh xoá rõ nghĩa

```text
Quên phép sửa X→Y
Xoá từ Y khỏi lịch sử dùng từ
Không gợi ý phép sửa này
Xoá toàn bộ dữ liệu học
```

V2 UI có thể chỉ expose lệnh đầu và cuối, nhưng API/schema phải không nhập nhằng các nghĩa trên.

### 11.5 Dọn dữ liệu

Giới hạn mặc định đề xuất:

```text
max_corrections = 10_000
max_context_rows = 30_000
max_recent_events_per_bucket = 64 sau compaction
```

Ưu tiên giữ:

1. explicit suppression/user-authored data;
2. rule đang được dùng gần đây;
3. rule có evidence mạnh;
4. rule đang đủ điều kiện can thiệp;
5. Personal promoted.

Dọn trước:

1. Ignore/probation cũ không đủ support;
2. suggestion hiện nhiều nhưng chưa từng chọn;
3. context refinement yếu đã có global fallback;
4. rule hết hạn và không còn recent veto.

Maintenance chạy ngoài hook path, có snapshot/worker ownership rõ và deterministic fake-clock tests.

---

## 12. Capture và replay

### 12.1 Capture v2

Replay cần biết quyết định thật đã xảy ra, không chỉ chạy lại config mới trên input cũ. Capture bổ sung record khái niệm:

```text
CandidateSetEvaluated { ids, source, base/final score, config hash }
InterventionApplied { edit_id, candidate_id, reason }
InterventionReverted { edit_id, kind }
InterventionSettled { edit_id }
CorrectionConfirmed { correction identity, context }
LanguageCommitSettled { token, left token, transaction_id }
DataForgotten { stable identity }
```

Payload thực tế phải tối thiểu hoá text và tránh duplicate raw stream không cần thiết. Record IDs/version cho phép idempotent replay.

### 12.2 Compaction checkpoint

Model snapshot là trạng thái chính. Capture là journal bounded sau snapshot, không phải lịch sử gõ vĩnh viễn.

Khi compaction hoặc Forget:

1. tạo model snapshot mới;
2. đặt cursor/checkpoint mới;
3. chỉ giữ journal cần cho pending transaction/recovery;
4. coherent-save pair mới;
5. backup theo policy recovery hiện hành.

### 12.3 Bất biến replay

- Cùng snapshot + journal → cùng model hash.
- Một feedback/edit không áp hai lần.
- Config mới không làm thay quyết định lịch sử đã record.
- Migration + replay không tăng quyền can thiệp.
- Forget sau restart không khôi phục row đã xoá.

---

## 13. Privacy và an toàn

### 13.1 Capability bắt buộc

Tách rõ:

```text
allow_transform
allow_suggestions
allow_learning
allow_context_read
allow_active_intervention
```

Không nhất thiết thêm ngay tất cả field vào `InputContext` đã đóng băng. Adapter/session có thể truyền policy riêng; nếu đổi `types.rs` phải có ADR mới theo ADR 0002.

### 13.2 Private/no-learning

Trong mode không học:

- có thể dùng model đã học để gợi ý nếu product policy cho phép;
- không mutate correction memory;
- không mutate language model;
- không tăng impression;
- không capture text;
- không tạo pending transaction cần persist;
- sensitive/password mặc định không hiển thị suggestion.

### 13.3 Dữ liệu được phép lưu

- correction exact cần thiết cho tính năng;
- token/cặp token NFC bounded cho thống kê từ;
- timestamps/counters/IDs;
- source/config/schema metadata.

Không lưu:

- toàn bộ document;
- clipboard;
- surrounding text dài;
- secrets/password;
- telemetry/network identifiers;
- app/document identity trong model v2 đầu tiên.

### 13.4 Không log dữ liệu gõ

Score breakdown/reason có thể log trong test bằng fixture tự biên soạn. Production diagnostics chỉ log mã lỗi, version, count tổng và latency; không log original/candidate/token/context.

---

## 14. Học kiểu lỗi tổng quát — giai đoạn sau

### 14.1 Mục đích

Sau khi exact model và thống kê ngữ cảnh ổn định, có thể học:

```text
hay đảo hai ký tự
hay bấm phím liền kề
hay bấm thừa một phím
hay đặt phím dấu Telex/VNI quá sớm
```

Đây là `generalized_error_model`, tách khỏi exact correction memory.

### 14.2 Dữ liệu được học

Chỉ học từ intent mạnh:

- explicit accept có provenance rõ;
- delete/retype contiguous đã xác nhận;
- correction alignment duy nhất/đáng tin.

Không học pattern tổng quát từ:

- suggestion bị bỏ qua;
- AutoSettled yếu;
- một immediate Backspace mơ hồ;
- paste;
- sensitive/private context.

### 14.3 Rào chắn

- chạy quan sát trước;
- sau đó tối đa hỗ trợ Gợi ý;
- chưa cho Auto trong v2;
- exact negative/suppression luôn thắng;
- cần support trên nhiều cặp từ khác nhau trước khi generalize;
- đóng trần ảnh hưởng của learned operation;
- không sửa original đã hợp lệ chỉ bằng error pattern;
- static fuzzy cost vẫn là fallback versioned.

### 14.4 Chưa dùng bandit

Không triển khai cơ chế tự thử nhiều hành động để xem người dùng phản ứng. OpenViKey không được khám phá bằng cách tự ý sửa chữ. Nếu nghiên cứu reranker online sau này, chỉ thay thứ tự gợi ý và phải có capture đầy đủ candidate set/outcome; không nằm trong spec v2 release.

---

## 15. Giao diện người dùng và khả năng giải thích

### 15.1 Thông báo ngắn

Người dùng thấy hành vi, không phải công thức:

```text
Gợi ý: khogn → không
Đã sửa lỗi dấu: ch2ao → chào
Đã sửa theo thói quen của bạn: khogn → không
Đã ghi nhớ: aaa → bbb (1/2 lần)
Đã tạo gợi ý cá nhân: aaa → bbb
Đã tạm dừng phép sửa vì bạn vừa hoàn tác
Đã quên: X → Y
```

Raw mass/confidence chỉ nằm trong chi tiết nâng cao.

### 15.2 Learned-rules UI

Mỗi dòng hiển thị:

- original/replacement;
- nguồn;
- kiểu gõ;
- trạng thái hiện tại do planner tính;
- bằng chứng tốt/xấu;
- lần dùng gần nhất;
- lý do bị cooldown/suppression;
- mức ảnh hưởng ngữ cảnh nếu có.

Không hiển thị surrounding context mặc định để giảm lộ nội dung. Context chi tiết chỉ có trong inspector development nếu policy cho phép.

### 15.3 Lệnh người dùng

- Quên rule được chọn.
- Quên rule gần nhất.
- Xoá toàn bộ dữ liệu học.
- Tắt learning nhưng vẫn dùng bộ gõ.
- Tắt suggestions riêng.

Explicit suppression “không bao giờ gợi ý cặp này” là P1 sau khi schema/API ổn định.

### 15.4 Biểu đồ theo dõi model

Settings → Learning có hai mức hiển thị.

#### A. Biểu đồ của một phép sửa

Khi chọn một dòng `X → Y`, hiển thị một biểu đồ thời gian duy nhất, dễ đọc:

- trục ngang: thời gian hoặc thứ tự sự kiện;
- trục dọc: confidence từ 0% đến 100%;
- nền xám: Đang quan sát;
- nền vàng: Gợi ý;
- nền xanh: Có thể tự sửa;
- vùng gạch chéo: cooldown/chốt do revert;
- marker `+`: Accept hoặc correction tự nhiên;
- marker `−`: Reject/confirmed rejection;
- marker `↩`: Revert/Undo;
- marker nhỏ: weak settlement;
- đường confidence toàn cục và đường confidence sau khi trộn ngữ cảnh.

Ngay dưới biểu đồ có thanh phân rã điểm hiện tại:

```text
Điểm nền generator             ███████░░░
Ảnh hưởng correction cá nhân   +██
Ảnh hưởng từ đứng trước        +█
Phạt do recent revert          -███
Khoảng cách top1-top2          0.14
Kết luận                       Gợi ý — đang cooldown
```

Người dùng không cần đọc công thức. Nút “Chi tiết kỹ thuật” mới hiện positive/negative mass, half-life, config version/hash và công thức dùng tại điểm đang chọn.

#### B. Tổng quan hệ thống

Trang tổng quan hiển thị tối thiểu:

- số rule Đang quan sát / Gợi ý / Có thể tự sửa / Cooldown;
- phân bố theo TelexFix/Fuzzy/Abbreviation/Diacritics/Personal;
- các rule bị revert nhiều nhất;
- tỷ lệ can thiệp được giữ lại so với bị undo;
- phân bố confidence theo các dải 0–20–40–60–80–100%;
- kích thước model và số row đã prune.

#### C. Nguồn dữ liệu và giới hạn riêng tư

Biểu đồ được tính local từ summary checkpoint + tối đa 64 recent events mỗi bucket. Không lưu thêm toàn bộ lịch sử chỉ để vẽ chart. Sau compaction, chart bắt đầu bằng một marker “Tổng hợp dữ liệu cũ” rồi tiếp tục với event gần đây.

Chart không đọc capture thô, không gửi network và không log token. “Xuất dữ liệu biểu đồ” nếu có phải là thao tác chủ động, cảnh báo dữ liệu cá nhân và mặc định tắt trong product preview.

#### D. Cách vẽ

- Settings native dùng custom chart control/GDI hiện có, không thêm web runtime.
- Mọi series được chuẩn bị từ read-only snapshot ngoài hook path.
- Chart có bảng text thay thế để hỗ trợ accessibility và test.
- `openvikey-lab` có thể xuất cùng `ChartSnapshot` dạng JSON cho golden test; JSON không phải giao diện product mặc định.

---

## 16. Kiểm thử

### 16.1 Characterization trước refactor

Trước thay kiến trúc, đóng băng hành vi hiện tại bằng test:

- TelexFix/Fuzzy/Abbreviation policy auto;
- learned Auto;
- immediate Backspace và hotkey Undo;
- accept/reject;
- composition rewind matched/unmatched;
- left-context backoff hiện tại;
- model/capture restart;
- sensitive/terminal/English zero mutation.

Lát đầu chỉ di chuyển logic vào planner và phải giữ toàn bộ test xanh.

### 16.2 Correction memory

- global/context blend ở 0, ít và nhiều support;
- context negative không bị context khác `Auto` lấn bằng `max`;
- evidence decay và compaction tương đương;
- duplicate seq/edit idempotent;
- Personal promote đúng hai transaction độc lập;
- weak settlement cap;
- recent revert cooldown;
- source caps không thể vượt qua;
- physical forget payload scrub.

### 16.3 Giao dịch

- apply → immediate Backspace trả exact raw composition;
- immediate Backspace chưa tự cộng negative mạnh;
- recommit original tạo đúng một negative;
- explicit Undo tạo đúng một negative;
- apply → settle tạo đúng một weak positive;
- focus/caret/paste/inject failure rollback đúng;
- model và language update cùng rollback hoặc cùng commit.

### 16.4 Thống kê từ

- NFC/NFD hội tụ cùng identity;
- phân biệt `a/ă/â`, `o/ô/ơ`, `u/ư`, `d/đ`;
- unigram/bigram count và backoff;
- user commit khác accepted suggestion và unsettled Auto;
- private mode zero write;
- pruning deterministic;
- restart giữ kết quả;
- 10k/30k stress latency.

### 16.5 Planner

Ma trận:

```text
source × token length × context safety × score × margin × exact evidence
× revert guard × cooldown × source cap × edit validity × config mode
```

Test bắt buộc:

- 0 hoặc 1 grapheme chữ → không suggestion/intervention;
- 2 grapheme chữ → bắt đầu đủ điều kiện;
- `dd → đ`, `a1 → á` vẫn được tính là một grapheme và không gợi ý;
- raw key dài nhưng output một chữ không vượt guard;
- revert trong 3 giây → cùng Space không replace;
- Space đầu tiên sau guard commit original kể cả đã quá 3 giây;
- raw token thay đổi → guard được xoá;
- focus/caret/method/mode/reset → guard được xoá;
- hai revert tạo long cooldown/demotion theo policy.

Mỗi kết quả assert cả action và reason. Không test private helper thay vì hành vi public.

### 16.6 Migration

- v1 fixture thật → v2 deterministic;
- evidence totals không mất/nhân đôi;
- Personal rows giữ trạng thái Suggest-only;
- Auto không được nâng quyền sau migration;
- encrypted lab payload và open Windows payload đều round-trip;
- corrupt/interrupted migration phục hồi backup;
- old capture version đọc fail rõ hoặc migrate có kiểm soát.

### 16.7 Chất lượng và hiệu chỉnh

Báo riêng:

- độ chính xác top-1/top-3 của Gợi ý;
- precision/FPR của Structural Fix;
- precision/FPR của Heuristic Assist;
- precision/FPR của Learned Auto;
- undo/revert rate theo mode;
- số bằng chứng cần để thích nghi;
- cải thiện xếp hạng khi có bigram cá nhân;
- calibration theo bucket confidence/support;
- storage growth và prune rate.

Giữ gate corpus hiện tại tối thiểu. Mọi policy can thiệp mới có thể đặt gate nghiêm hơn, nhưng chỉ chốt con số release sau calibration có provenance.

### 16.8 Biểu đồ

- `ChartSnapshot` từ cùng model/config/time cho byte-identical JSON test output;
- confidence line khớp query model tại từng recent event;
- state bands khớp planner thresholds/config version;
- score breakdown cộng lại đúng final assessment trong sai số cho phép;
- cooldown/revert regions bắt đầu và kết thúc đúng caller-time;
- compaction tạo checkpoint marker, không làm đổi điểm cuối;
- physical forget làm series của rule biến mất;
- private/sensitive data không xuất hiện trong snapshot;
- chart snapshot của 10.000 rules được tạo ngoài hook path trong budget UI đã định.

### 16.9 Hiệu năng

- Không I/O, network, serialize, sleep hoặc blocking lock trên hook path.
- Candidate generation budget hiện tại giữ nguyên.
- Planner và in-memory lookup có budget riêng; mục tiêu khởi điểm P95 < 1 ms sau khi candidates đã có.
- Không quét tuyến tính toàn bộ model theo số rule; dùng index/map hoặc immutable snapshot phù hợp.
- Maintenance/compaction chạy worker hoặc thời điểm idle.
- Save vẫn debounce và coherent.

---

## 17. Rollout theo lát an toàn

### Lát 0 — Đóng băng hành vi và ADR

- Thêm characterization tests còn thiếu.
- Viết ADR mở lại seam model/feedback cần thay; không đổi engine.
- Chốt payload/capture versioning và migration fixtures.

### Lát 1 — Bộ quyết định thống nhất, chưa đổi hành vi

- Tạo planner thuần và reason codes.
- Di chuyển learned Auto và boundary assist vào planner.
- Config compatibility giữ hành vi hiện tại.
- Không đổi payload.

### Lát 2 — Guard bắt buộc và chống vòng lặp hiện tại

- Khoá minimum correction length = 2 grapheme chữ ngay trong planner.
- Thêm `RevertGuard`, cửa sổ 3 giây và one-shot boundary bypass trên model hiện tại.
- Giữ text/undo đúng trước khi thay schema học.
- Thêm regression cho chuỗi Space → replace → Backspace → Space.

### Lát 3 — Physical forget và lifecycle bounds

- Sửa forget để purge row thật.
- Thêm global cap/eviction cho v1 trước migration nếu cần.
- Capture compaction checkpoint.
- Privacy payload tests.

### Lát 4 — Correction memory v2

- Payload version 2 và migration.
- Global identity + context refinement.
- Evidence summary/compaction.
- Hợp nhất Personal.
- State do query/planner suy ra.

### Lát 5 — Giao dịch feedback v2

- Tách immediate revert, explicit Undo và confirmed rejection.
- Settlement transaction và weak cap.
- Impression counters không ảnh hưởng confidence.
- Replay/capture v2.

### Lát 6 — Thống kê unigram

- Học output token an toàn sau commit/settlement.
- Chỉ dùng rerank suggestion.
- Bounded store, pruning, inspect/forget.

### Lát 7 — Thống kê bigram và context margin

- Một left token.
- Support-aware backoff.
- Planner dùng context margin như guard phụ, không cấp Auto một mình.

### Lát 8 — Biểu đồ model

- Thêm `ChartSnapshot`, biểu đồ rule và tổng quan local.
- Hiện confidence, evidence, score breakdown, state và cooldown.
- Test accessibility/read-only snapshot và không ảnh hưởng hook path.

### Lát 9 — Hiệu chỉnh product policy

- Đánh giá tắt cold-start Abbreviation Auto.
- Đánh giá Fuzzy heuristic assist mặc định on/off.
- Version/hash config mới.
- Chỉ đổi default sau corpus/replay gate.

### Lát 10 — Generalized error model quan sát

- Học operation từ intent mạnh.
- Không thay text/rank production.
- Báo offline/shadow; quyết định milestone mới trước khi suggestion rollout.

Mỗi lát cần implementation plan TDD riêng và commit nhỏ; không triển khai big-bang toàn spec.

---

## 18. Tương thích và ranh giới ADR

ADR 0002 đóng băng `types.rs` và engine. Spec này giữ engine contract. Nếu implementation cần:

- thêm feedback variants;
- đổi nghĩa `RuleContextKey` công khai;
- thêm capture operation;
- đổi `ModelView` contract;

phải tạo ADR mới trước khi code lát tương ứng.

Generator vẫn không nhận model. Store vẫn persist opaque bytes. Adapter OS không được sở hữu model quyết định riêng.

Các spec cũ tiếp tục mô tả hành vi lịch sử; sau khi v2 được chấp nhận, cần ADR ghi rõ phần nào được thay thế, không sửa ngược tài liệu cũ như thể hành vi đó chưa từng tồn tại.

---

## 19. Definition of done

Refactor learning v2 chỉ hoàn tất khi:

1. mọi Gợi ý/Tự sửa đi qua một planner và có reason ổn định;
2. UI state khớp hành động runtime;
3. correction evidence toàn cục/context được trộn có kiểm soát;
4. Personal dùng cùng lifecycle và vẫn Suggest-only;
5. immediate Backspace rollback chính xác nhưng không bị hiểu nhầm thành reject mạnh mặc định;
6. explicit reject/undo và confirmed correction tạo đúng evidence;
7. settlement yếu có trần và không tự promote một mình;
8. unigram/bigram cá nhân cải thiện ranking nhưng không tự cấp Auto;
9. model/capture có global bounds, compaction và deterministic replay;
10. Forget selected/all scrub dữ liệu bền vững đúng contract;
11. v1→v2 migration an toàn, có backup và không tăng quyền can thiệp;
12. private/sensitive/terminal/English tests chứng minh zero mutation;
13. hook/performance budgets giữ xanh;
14. token dưới hai grapheme chữ không tạo correction suggestion/intervention, nhưng engine một chữ vẫn hoạt động;
15. undo + Space trên cùng raw token không thể lặp replacement; 3 giây cooldown và one-shot boundary bypass được test;
16. biểu đồ local hiển thị confidence, evidence, trạng thái, cooldown và score breakdown từ read-only snapshot;
17. corpus reports tách từng loại intervention và lưu config hash;
18. workspace fmt/clippy/test/deny xanh;
19. generalized error learning nếu có vẫn chỉ ở chế độ quan sát cho đến spec rollout riêng.

---

## 20. Các điểm cần chủ dự án duyệt để nâng spec lên v1

Bản nháp đề xuất mặc định sau:

1. **Immediate Backspace:** khôi phục text và rollback, chưa tính negative mạnh; chỉ negative mạnh khi người dùng commit lại original hoặc dùng hotkey Undo rõ ràng.
2. **Abbreviation:** bỏ cold-start Auto trong policy đích; cần evidence cá nhân trước khi tự sửa một từ, cụm nhiều từ luôn chỉ Gợi ý.
3. **Fuzzy:** cold-start heuristic assist nằm sau feature flag và quality gate; learned Auto vẫn được phép khi đủ evidence.
4. **TelexFix:** structural fix được auto không cần 18 evidence nhưng phải đi qua planner và chịu cooldown khi bị revert lặp lại.
5. **Thống kê ngữ cảnh:** v2 chỉ unigram + một bigram trái; chưa làm trigram.
6. **Suggestion bị bỏ qua:** chỉ tăng impression, không cộng `-0.2`.
7. **Forget:** phải scrub cả correction memory và journal/capture có thể tái tạo rule.
8. **Tham số chung:** không tự học theo từng user; mọi thay đổi qua config version + calibration.
9. **Độ dài tối thiểu:** correction suggestion/intervention chỉ bắt đầu từ hai grapheme chữ; raw modifier không được tính tăng độ dài.
10. **Chống vòng lặp:** semantic revert dùng cửa sổ 3 giây, cooldown 3 giây và bắt buộc bypass replacement ở boundary đầu tiên của raw token chưa đổi.
11. **Biểu đồ:** Settings có timeline từng rule, score breakdown và tổng quan; dữ liệu chỉ local và bounded theo recent events.

Khi mười một điểm này được chấp nhận, spec có thể chuyển thành `v1 — accepted` và mới tách implementation plan theo từng lát.

**Quyết định 2026-08-21:** chủ dự án chấp nhận cả mười một điểm mặc định. Spec này là `v1 — accepted`. Implementation theo [`../plans/2026-08-21-openvikey-learning-model-v2-implementation-plan.md`](../plans/2026-08-21-openvikey-learning-model-v2-implementation-plan.md) (TDD, 11 lát, Task 0–27).
