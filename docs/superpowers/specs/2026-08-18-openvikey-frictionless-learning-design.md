# OpenViKey — Học không gián đoạn (Frictionless Learning Design Spec)

- **Ngày:** 2026-08-18
- **Trạng thái:** v1 — đề xuất để thảo luận, chưa triển khai
- **Phạm vi:** `openvikey-core`, `openvikey-session`, `openvikey-win`, `openvikey-lab`
- **Spec nền:** [`2026-08-17-openvikey-design.md`](./2026-08-17-openvikey-design.md)
- **Personal capture:** [`2026-08-17-openvikey-part2-personal-capture-design.md`](./2026-08-17-openvikey-part2-personal-capture-design.md)
- **Windows host:** [`2026-08-18-openvikey-gd2a-hook-electron-design.md`](./2026-08-18-openvikey-gd2a-hook-electron-design.md)

---

## 0. Tóm tắt quyết định

`Ctrl+.` tạo feedback rõ ràng nhưng buộc người dùng rời nhịp gõ tự nhiên để xác nhận từng đề xuất. Phím này sẽ được giữ như **fallback tường minh**, không còn là con đường học chính.

OpenViKey sẽ học theo ba cơ chế, triển khai theo thứ tự an toàn:

1. **Implicit Correction v2:** học mạnh từ thao tác tự nhiên `gõ → xoá → gõ lại`.
2. **Boundary Assist:** sau khi đã có một ít bằng chứng cá nhân, Space hoặc dấu câu được phép áp dụng ứng viên đủ chắc chắn mà không cần `Ctrl+.`.
3. **Probation Auto:** với lỗi hình thức cực rõ, hệ thống được thử tự sửa có giới hạn để phá vòng lặp cold-start; không bị hoàn tác mới tạo bằng chứng dương yếu.

Nguyên tắc khóa:

- **Không hành động không đồng nghĩa với xác nhận mạnh.**
- Space kích hoạt một phép sửa không được tính ngay như `ExplicitAccept`.
- Sửa lại trực tiếp là tín hiệu dương mạnh; Undo/Backspace ngay sau tự sửa là tín hiệu âm mạnh.
- Mọi can thiệp chủ động phải hoàn tác được chính xác.
- `Diacritics` không được Boundary Assist hoặc Probation Auto trong phiên bản này.
- Không learning/probe trong ngữ cảnh nhạy cảm, terminal hoặc nơi `allow_learning=false`.

---

## 1. Vấn đề

### 1.1 Hành vi hiện tại

Trên Windows, người dùng phải nhấn:

- `Ctrl+.` để chấp nhận top suggestion;
- `Ctrl+,` để từ chối;
- `Ctrl+Shift+Z` để Undo một Auto edit.

Trong lab, hành vi tương ứng là Tab/Esc/Ctrl+Z.

`Ctrl+.` có ba nhược điểm:

1. thêm một chord ngoài luồng gõ thông thường;
2. yêu cầu người dùng quan sát overlay và ra quyết định liên tục;
3. tạo vòng lặp cold-start: muốn Auto cần nhiều Accept, nhưng Accept quá tốn công nên người dùng ít tạo evidence.

### 1.2 Vấn đề mô hình hành vi

Không phải mọi sự im lặng đều mang cùng ý nghĩa:

- Người dùng chủ động sửa `X → Y`: ý định rõ.
- Người dùng Undo một tự sửa: phản đối rõ.
- Người dùng nhấn Space sau `X`: chỉ cho biết họ kết thúc token, chưa chứng minh họ đồng ý với `Y`.
- Người dùng tiếp tục gõ sau khi hệ thống tự sửa: chỉ là xác nhận yếu; có thể họ chưa nhìn thấy lỗi.
- Người dùng bỏ qua suggestion nhưng không sửa: tín hiệu bị kiểm duyệt (`censored`), không phải reject.

Spec này tách các tín hiệu trên thay vì quy tất cả thành Accept/Reject nhị phân.

---

## 2. Mục tiêu và non-goals

### 2.1 Mục tiêu

- Với một correction đủ điều kiện, người dùng không phải nhấn thêm phím ngoài thao tác gõ bình thường.
- Học được từ sửa lỗi tự nhiên mà không yêu cầu mở settings hoặc khai báo rule.
- Có đường bootstrap an toàn từ cold-start đến personalized Auto.
- Một can thiệp sai phải có cách hoàn tác tự nhiên, nhanh và tạo feedback đúng rule-context.
- Cùng input/capture/model/config phải replay ra cùng kết quả.
- Không tăng I/O, sleep, serialization hoặc blocking lock trên Windows hook path.

### 2.2 Non-goals

- Không bỏ `Ctrl+.`/`Ctrl+,`; chúng vẫn là fallback và công cụ debug.
- Không tự động thêm dấu cả câu/cụm.
- Không học một cặp tùy ý nếu không ánh xạ được về candidate/rule có thật.
- Không tự sửa `Diacritics` trong milestone này.
- Không dùng cloud, telemetry, account hoặc backend.
- Không giải quyết chỉnh sửa tùy ý sau caret move/selection/paste nhiều dòng.
- Không thêm settings GUI trong lần triển khai đầu; policy nằm trong versioned config và có thể expose UI sau.

---

## 3. Nguyên tắc feedback

### 3.1 Thứ tự độ mạnh

Từ mạnh nhất đến yếu nhất:

```text
Undo/Backspace ngay sau can thiệp  → phản hồi âm mạnh
Sửa X thành Y bằng thao tác tự nhiên → phản hồi dương mạnh
Ctrl+. / Ctrl+,                    → phản hồi tường minh mạnh
Can thiệp tồn tại qua 10 event      → phản hồi dương yếu
Không phản ứng với suggestion       → không kết luận
```

### 3.2 Trọng lượng v1

Giữ tương thích với mô hình Beta mass hiện tại:

| Tín hiệu | Feedback | Mass |
|---|---|---:|
| `Ctrl+.` | `Accept` | positive `+1.0` |
| `Ctrl+,` | `ExplicitReject` | negative `+1.0` |
| Xóa X và gõ lại candidate Y | `ImplicitCorrection` | positive `+1.0` |
| Undo/Backspace ngay sau assist/auto | `Undo` | negative `+1.5` |
| Assist/Auto tồn tại qua 10 event hợp lệ | `AutoSettled` | positive `+0.3` |
| Suggestion chỉ được hiển thị rồi bị bỏ qua | không phát event | `0` |

**Quyết định khóa:** Boundary Assist hoặc Probation Auto **không phát `Accept` ngay lúc Space/dấu câu**. Nó chỉ tạo một pending intervention. Nếu tồn tại qua settlement window mới nhận `AutoSettled +0.3`.

### 3.3 Đơn vị học

Mọi feedback phải quay về đúng `RuleContextKey` hiện tại:

```text
{
  input_method,
  source,
  original_nfc,
  candidate_nfc,
  left_token_nfc,
  source_rule_id
}
```

Không tạo key giả và không học chỉ bằng cặp text thiếu source/rule identity.

---

## 4. Cơ chế 1 — Implicit Correction v2

### 4.1 UX

Người dùng làm đúng hành vi vốn có:

```text
khogn␠ → Backspace… → không␠
```

Nếu `không` là candidate hợp lệ của snapshot `khogn`, hệ thống ghi một `ImplicitCorrection{khogn→không}`. Không overlay xác nhận và không hotkey.

### 4.2 Máy trạng thái edit transaction

`openvikey-session` sở hữu detector thuần, không phụ thuộc Windows API:

```text
Idle
  └─ Backspace vào committed token X
       → Deleting { original_unit, candidate_snapshot, started_at_ms }

Deleting
  ├─ tiếp tục Backspace cùng token → giữ nguyên full X, chỉ cập nhật range
  ├─ nhập ký tự                  → Retyping { ... }
  └─ caret/selection/focus/paste → Cancelled

Retyping
  ├─ nhập ký tự                  → tiếp tục Y
  ├─ Space/dấu câu               → Evaluate(X, Y)
  └─ caret/selection/focus/paste → Cancelled
```

### 4.3 Điều kiện phát feedback

Chỉ phát `ImplicitCorrection` khi tất cả đúng:

1. X và Y khác nhau, không rỗng, được chuẩn hóa NFC;
2. diễn ra trong cùng document/focus identity;
3. không có `CursorMoved`, `SelectionChanged`, mouse caret-break, app switch hoặc `Reset` ở giữa;
4. không có paste/`InsertText` nhiều grapheme hoặc shortcut chỉnh sửa;
5. Y khớp chính xác `candidate.text` trong candidate snapshot của X;
6. `allow_learning=true` ở lúc bắt đầu và kết thúc transaction;
7. transaction hoàn tất trong `implicit_max_duration_ms` mặc định 10 giây.

Nếu Y không khớp candidate có thật: vẫn hiển thị/commit bình thường nhưng **không mutate model**.

### 4.4 Giới hạn v1

- Một token committed tại caret.
- Cho phép xóa delimiter và một token; chưa mine phrase nhiều token.
- Candidate snapshot phải lưu đủ field để dựng lại chính xác `RuleContextKey`.
- `record_deleted_token(X)` chỉ được gọi một lần khi bắt đầu xóa token, không ghi đè X khi xóa từng grapheme.

### 4.5 Cải thiện so với code hiện tại

Cơ chế hiện tại đã có `ImplicitCorrectionMiner`, nhưng logic orchestration còn gắn chặt vào một contiguous delete đơn giản. v2 phải:

- biểu diễn transaction rõ ràng;
- kiểm tra focus/document identity;
- có timeout caller-supplied;
- test paste, caret-break, app-switch và partial delete;
- giữ deterministic replay bằng `at_ms` từ event, không tự đọc wall clock trong core/session.

---

## 5. Cơ chế 2 — Boundary Assist

### 5.1 UX

Khi người dùng kết thúc token bằng Space hoặc dấu câu, top candidate có thể được áp dụng ngay:

```text
ko[Space] → không␠
khogn[,]  → không,
```

Không cần `Ctrl+.`. Nếu candidate chưa đủ điều kiện, hệ thống commit nguyên bản và suggestion biến mất như hiện tại.

Enter/newline **không** kích hoạt Boundary Assist ở phiên bản đầu vì nguy cơ gửi nội dung trước khi người dùng kịp nhận ra correction.

### 5.2 Điều kiện chung

Boundary Assist chỉ được phép khi:

- `allow_transform=true` và `allow_learning=true`;
- boundary thuộc `{Space, '.', ',', ';', ':', '?', '!'}`;
- snapshot/revision/range còn hợp lệ;
- có ít nhất một candidate;
- top candidate vượt ngưỡng theo source;
- khoảng cách `top1.final_score - top2.final_score` đủ lớn; nếu không có top2, dùng margin `1.0`;
- rule không có Undo gần đây và không bị auto-demotion block;
- focus không đổi và context không thuộc deny/sensitive policy.

### 5.3 Ngưỡng khởi điểm

Các giá trị nằm trong `InterventionConfigV1`, không hard-code rải rác:

| Source | positive mass | confidence | top score | top1-top2 | Cho Boundary Assist |
|---|---:|---:|---:|---:|---|
| `TelexFix` | `≥ 2.0` | `≥ 0.75` | `≥ 0.90` | `≥ 0.10` | Có |
| `Fuzzy` | `≥ 2.0` | `≥ 0.75` | `≥ 0.90` | `≥ 0.12` | Có |
| `Abbreviation` | `≥ 5.0` | `≥ 0.85` | `≥ 0.92` | `≥ 0.15` | Có |
| `Diacritics` | — | — | — | — | Không |

Các ngưỡng là default triển khai ban đầu, phải được đo lại bằng corpus/replay trước khi bật mặc định cho release.

### 5.4 Hoàn tác tự nhiên

Sau Boundary Assist, session tạo `PendingIntervention` chứa:

```text
edit_id, rule_key, mode, original, replacement,
delimiter, revision, focus_id, remaining_events
```

Backspace được hiểu là semantic Undo nếu:

- composition hiện tại rỗng;
- intervention là thao tác committed gần nhất;
- chưa có ký tự mới sau delimiter;
- focus/caret/revision vẫn khớp.

Kết quả:

```text
không␠| + Backspace → ko|
```

Hệ thống khôi phục nguyên bản, bỏ delimiter vừa kích hoạt assist và phát `Undo` negative `+1.5`. Nếu bất kỳ điều kiện nào không đúng, Backspace giữ hành vi xóa bình thường.

`Ctrl+Shift+Z` vẫn Undo được intervention theo semantic edit log.

### 5.5 Phản hồi trực quan

Overlay hiển thị không blocking trong thời gian ngắn:

```text
Đã sửa: ko → không · Backspace để hoàn tác
```

- Không focus overlay.
- Không yêu cầu click.
- Timer chạy ở UI thread, không chạy/sleep trong keyboard hook.
- Overlay chỉ hỗ trợ nhận biết; tính đúng đắn của Undo không phụ thuộc overlay còn hiển thị.

### 5.6 Settlement

Sau 10 input/edit event hợp lệ mà không Undo/caret-break:

- phát đúng một `AutoSettled{edit_id}`;
- thêm positive mass `+0.3`;
- xóa pending intervention.

Caret-break không được coi là xác nhận; nó chỉ hủy khả năng immediate Backspace và settlement nếu không còn chứng minh được edit vẫn tồn tại.

---

## 6. Cơ chế 3 — Probation Auto

### 6.1 Mục đích

Boundary Assist cần evidence cá nhân; evidence lại thường đến từ Accept hoặc implicit correction. Probation Auto tạo một số thử nghiệm rất hạn chế cho lỗi có độ chính xác nền cao để phá vòng lặp cold-start.

Đây là **delivery policy**, không phải trạng thái học lâu dài mới. `DecisionState` bền vững vẫn là:

```text
Ignore → Suggest → Auto
```

Planner tạo `InterventionMode::ProbationAuto` khi rule vẫn ở Suggest/Ignore nhưng thỏa policy nghiêm ngặt.

### 6.2 Source policy

- `TelexFix`: được probe vì rule tái dựng dấu sai vị trí bị giới hạn bởi hình dạng âm tiết.
- `Fuzzy`: được probe chỉ khi top score và margin cực cao.
- `Abbreviation`: không probe ở cold-start vì `ko`, `dc`, `ntn` có thể là chủ ý.
- `Diacritics`: không probe vì mơ hồ ngữ nghĩa cao.

### 6.3 Điều kiện probe

Điều kiện chung:

- mọi điều kiện an toàn của Boundary Assist;
- rule chưa có negative evidence;
- rule không bị demote hoặc cooldown;
- chưa probe rule này trong 24 giờ caller-time;
- chưa vượt quá 3 probe trong session hiện tại;
- chỉ probe tại Space/dấu câu, không tại Enter.

Ngưỡng source-specific ban đầu:

| Source | top score | top1-top2 | Điều kiện thêm |
|---|---:|---:|---|
| `TelexFix` | `≥ 0.92` | `≥ 0.20` hoặc chỉ có 1 candidate | syllable-shape check đã pass |
| `Fuzzy` | `≥ 0.96` | `≥ 0.18` | target có trong lexicon; original không phải từ hợp lệ |

Nếu dữ liệu calibration không chứng minh precision yêu cầu ở §11, `probation_enabled` mặc định phải là `false` (fail closed).

### 6.4 Học từ probe

- Không cộng positive mass tại thời điểm probe.
- Tồn tại 10 event: `AutoSettled +0.3`.
- Immediate Backspace/Undo: `Undo +1.5`, đặt cooldown rule tối thiểu 30 ngày caller-time.
- Một Undo probe đủ dừng probe cho rule đó; không chờ điều kiện “2 Undo trong 10 Auto” của learned Auto.
- Nhiều settlement dần giúp rule đủ điều kiện Boundary Assist; promotion lên learned Auto vẫn dùng ngưỡng hiện tại: confidence `≥0.95`, positive mass `≥18`, final score `≥0.90`.

---

## 7. Intervention planner

### 7.1 Không mở rộng `DecisionState`

Không thêm `SoftAuto`/`ProbationAuto` vào model state bền vững để tránh trộn hai khái niệm:

- `DecisionState`: mức tin cậy đã học của rule;
- `InterventionMode`: cách UI/session phân phối candidate tại event hiện tại.

API đề xuất:

```rust
pub enum InterventionMode {
    None,
    DisplaySuggestion,
    BoundaryAssist,
    ProbationAuto,
    LearnedAuto,
}

pub struct InterventionPlan {
    pub mode: InterventionMode,
    pub candidate_id: Option<u64>,
    pub reason: InterventionReason,
}

pub fn plan_intervention(
    decision: DecisionState,
    candidates: &[Candidate],
    rule: &RuleContextKey,
    model: &dyn ModelView,
    boundary: Option<char>,
    context: InputContext,
    evaluate_at_ms: i64,
    config: &InterventionConfig,
) -> InterventionPlan;
```

`InterventionReason` là enum/mã ổn định phục vụ test và local model dump, không log nội dung gõ ra console production.

### 7.2 Thứ tự policy

```text
1. !allow_transform                       → None
2. sensitive hoặc !allow_learning         → tối đa DisplaySuggestion
3. learned DecisionState::Auto hợp lệ     → LearnedAuto
4. Boundary Assist đủ evidence            → BoundaryAssist
5. Probation policy đủ điều kiện           → ProbationAuto
6. DecisionState::Suggest                 → DisplaySuggestion
7. còn lại                                → None
```

`allow_learning=false` không được tạo Boundary/Probation intervention vì không thể cập nhật cooldown, settlement và feedback an toàn. Engine composition thông thường vẫn được phép nếu `allow_transform=true`.

### 7.3 Score margin

```text
margin = top1.final_score - top2.final_score
```

Nếu chỉ có một candidate, margin policy dùng `1.0`, nhưng vẫn phải vượt score/source/context gates. Dedupe phải diễn ra trước khi tính margin.

---

## 8. Thay đổi kiến trúc

### 8.1 `openvikey-core`

- Thêm module/policy thuần cho `InterventionConfig`, `InterventionMode`, `plan_intervention` hoặc đặt cạnh `decision.rs`.
- Mở read-only model evidence cần thiết cho planner, tối thiểu:
  - confidence;
  - positive mass;
  - negative mass hoặc `has_negative_evidence`;
  - auto demotion/cooldown/probe eligibility.
- Không đọc wall clock, foreground app hoặc Windows API.
- `ReplaceRangeAction` tiếp tục là payload semantic duy nhất cho mọi assisted replacement.

### 8.2 `openvikey-session`

- Nâng `ImplicitCorrectionMiner` thành transaction detector theo §4.
- Tích hợp planner sau generate/rank/learned decision.
- Tổng quát hóa auto log thành intervention log có `mode`.
- Hỗ trợ immediate Backspace semantic Undo.
- Settlement vẫn dùng event sequence và caller-supplied `at_ms`.
- Candidate snapshot phải giữ đúng rule identity.

### 8.3 `openvikey-win`

- Space/dấu câu tiếp tục đi qua synchronous `try_lock + session + SendInput` hiện tại.
- Sync layer phải áp `ReplaceRangeAction` cho cả Boundary Assist và Probation Auto như Learned Auto.
- Enter không kích hoạt hai mode mới.
- Overlay nhận visual event “đã sửa/cách hoàn tác”.
- Không lock blocking, serialize, I/O, sleep hoặc queue key trên LL hook.

### 8.4 `openvikey-lab`

- Dùng cùng planner/session reducer để replay và test headless.
- Giữ Tab/Esc/Ctrl+Z hiện tại.
- Render mode hiện tại để quan sát: `suggest`, `boundary`, `probe`, `auto`.

---

## 9. Persistence, schema và replay

### 9.1 Model schema

Probe cần lưu tối thiểu `last_probe_at_ms` và cooldown theo rule. Đây là thay đổi schema bền vững.

Yêu cầu:

- tạo model payload version mới hoặc migration v1→v2 rõ ràng;
- field mới dùng giá trị mặc định an toàn: chưa probe, không cooldown;
- không diễn giải model cũ như đã có positive evidence;
- serialization vẫn deterministic theo `RuleContextKey`.

Không âm thầm thay nghĩa `MODEL_VERSION=1` nếu payload mới không còn tương đương.

### 9.2 Capture schema

Replay phải biết policy nào đã quyết định Boundary/Probe. Capture header cần thêm:

```text
intervention_policy_version
intervention_policy_hash
```

Hai phương án hợp lệ:

1. replay bằng đúng config version/hash cũ; hoặc
2. capture explicit `InterventionApplied{mode, edit_id, rule identity}`.

Spec chọn **phương án 2** để replay không thay đổi khi calibration config tương lai đổi. Input command vẫn được giữ, nhưng quyết định can thiệp đã xảy ra phải là record có identity rõ ràng và idempotent.

Capture không được ghi nội dung ra telemetry/network. Chính sách plaintext development của Windows tiếp tục theo ADR 0008.

### 9.3 Idempotency

- Mỗi settlement/undo chỉ áp một lần theo `seq` và `edit_id`.
- Immediate Backspace và hotkey Undo không được tạo hai feedback cho cùng edit.
- Replay lặp cùng capture phải cho cùng model hash.

---

## 10. Các tình huống hành vi khóa

| Tình huống | Kết quả bắt buộc |
|---|---|
| Suggestion hiện, người dùng Space nhưng không đủ gate | commit original; không feedback |
| Boundary Assist áp dụng, người dùng tiếp tục 10 event | một `AutoSettled +0.3` |
| Boundary Assist áp dụng, Backspace ngay | exact restore original; một `Undo +1.5` |
| Đã bắt đầu token tiếp theo rồi Backspace | xóa composition bình thường; không Undo token trước |
| `Ctrl+.` trên suggestion | thay text + `Accept +1.0` như hiện tại |
| `Ctrl+,` | `ExplicitReject +1.0`; không apply candidate |
| Xóa X, gõ candidate Y, commit | `ImplicitCorrection +1.0` |
| Xóa X, gõ Y không thuộc candidate snapshot | không học |
| Caret/mouse/focus đổi giữa X và Y | hủy implicit transaction |
| `allow_learning=false` | không assist/probe/capture/model mutation |
| Diacritics dù confidence cao | tối đa suggestion |
| Abbreviation cold-start | không probe |
| Enter sau token | không Boundary/Probe; giữ ordering inject hiện tại |
| Probe bị Undo một lần | cooldown rule; không probe lại trong 30 ngày |

---

## 11. Tiêu chí thành công

### 11.1 Functional

- **F1:** Eligible Boundary Assist áp candidate bằng chính Space/dấu câu, không cần extra chord.
- **F2:** Immediate Backspace khôi phục chính xác original và delimiter/caret theo contract.
- **F3:** Implicit correction chỉ học candidate có thật và đúng rule-context.
- **F4:** Settlement phát đúng một lần sau 10 event; Undo/caret-break hủy đúng pending state.
- **F5:** Probe obey source, score, margin, per-rule cooldown và per-session budget.
- **F6:** `allow_learning=false` là strict no-op đối với evidence/probe/persistence.
- **F7:** Capture replay cho model hash giống phiên sống.
- **F8:** Model v1 hiện có migrate/load được mà không mất evidence.

### 11.2 Quality gate trước khi bật mặc định

Đo riêng từng mode trên held-out/replay stream:

- Boundary Assist precision `≥ 99.5%`.
- Probation Auto precision `≥ 99.7%`.
- Correct-token false intervention rate:
  - Boundary Assist `≤ 0.05%`;
  - Probation Auto `≤ 0.02%`.
- Báo riêng theo `TelexFix`, `Fuzzy`, `Abbreviation`.
- Không gộp suggestion accuracy vào unsolicited-intervention precision.
- Nếu sample chưa đủ hoặc CI Wilson lower bound chưa đạt ngưỡng do nhóm dự án chốt, feature tương ứng mặc định off.

### 11.3 Performance và host safety

- Planner/detector không I/O và không network.
- Không serialize dưới session lock trên hook path.
- Không `Mutex::lock`, sleep hoặc async key queue trong LL hook.
- P95 session planning phải nằm trong performance budget hiện hành; benchmark báo riêng generate/rank và intervention planning.
- SendInput failure phải rollback model/capture/intervention state bằng checkpoint hiện có.

### 11.4 Privacy

- Không learning/capture ở deny/sensitive contexts.
- Không log raw token trong production console.
- Windows `.ovkdev.json` vẫn là plaintext development data theo ADR 0008 và phải tiếp tục có cảnh báo.
- Lab encrypted store giữ nguyên.

---

## 12. Kế hoạch rollout

### Giai đoạn A — Implicit Correction v2

- Triển khai transaction detector và test headless.
- Không thay hành vi tự sửa của người dùng.
- Thu evidence cá nhân chất lượng cao trước.

### Giai đoạn B — Boundary Assist có evidence

- Chỉ bật cho rule đã đạt positive/confidence gates.
- `Diacritics` off; `Abbreviation` dùng gate cao hơn.
- Hoàn thiện immediate Backspace và overlay trước khi bật.

### Giai đoạn C — Probation Auto

- Mặc định off trong development config cho đến khi corpus/replay đạt §11.2.
- Bật lần lượt `TelexFix`, sau đó mới cân nhắc `Fuzzy`.
- Không bật Abbreviation/Diacritics cold-start trong spec v1.

### Giai đoạn D — Điều chỉnh ngưỡng

- Calibrate bằng split riêng; không fit trên held-out test.
- Version/hash mọi thay đổi policy.
- Không thay evidence lịch sử khi chỉ đổi threshold.

---

## 13. Test matrix tối thiểu

### Implicit miner

- full delete/retype candidate;
- partial delete;
- Y không phải candidate;
- timeout;
- caret/selection/mouse/focus break;
- paste/shortcut;
- NFC/NFD;
- cùng text nhưng khác left context/source rule.

### Boundary Assist

- mỗi source và mỗi threshold edge;
- top margin bằng/ngay dưới/ngay trên ngưỡng;
- Space và từng dấu câu cho phép;
- Enter bị loại;
- immediate Backspace exact inverse;
- Backspace sau khi đã bắt đầu token mới;
- SendInput partial failure rollback;
- learning disabled/sensitive context.

### Probation Auto

- per-rule 24h limit;
- 3 probe/session budget;
- một Undo tạo 30-day cooldown;
- settlement không cộng trùng;
- Abbreviation/Diacritics không probe;
- demoted rule không probe;
- restart giữ cooldown và replay deterministic.

### Regression

- `Ctrl+.`, `Ctrl+,`, `Ctrl+Shift+Z` giữ hành vi cũ;
- learned Auto promotion/demotion hiện tại không đổi;
- terminal/Electron/sensitive policies hiện tại không bị nới lỏng ngoài quyết định riêng;
- toàn workspace fmt/clippy/test/deny xanh.

---

## 14. Quyết định sản phẩm cuối cùng của spec v1

1. `Ctrl+.` được giữ nhưng là fallback, không phải UX học chính.
2. Không coi Space là explicit confirmation; Space chỉ cho phép planner thực hiện một intervention đã qua gate.
3. Sửa tự nhiên `X→Y` là nguồn evidence mạnh chính.
4. Sự tồn tại của auto/assist chỉ là evidence yếu.
5. Immediate Backspace là đường phản đối tự nhiên bắt buộc trước khi bật Boundary/Probe.
6. Probation Auto phải bounded, source-specific, cooldown sau một Undo và fail-closed theo quality gate.
7. Không tự sửa Diacritics và không probe Abbreviation ở cold-start.
8. Tách `DecisionState` khỏi `InterventionMode`; không thêm state bền vững chỉ để mô tả UX delivery.
