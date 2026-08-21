# Ghi nhận định hướng refactor hệ thống học

- **Ngày:** 2026-08-20
- **Trạng thái:** Ghi chú thảo luận, chưa phải quyết định kiến trúc chính thức

## Cách hiểu đơn giản về hệ thống hiện tại

OpenViKey hiện có hai phần:

1. **Bộ sửa có sẵn:** luật Telex/VNI, từ điển, viết tắt và phát hiện lỗi gõ tạo ra các phương án sửa.
2. **Sổ ghi nhớ cá nhân:** ghi điểm tốt/xấu cho từng phép sửa cụ thể dựa trên nhận gợi ý, từ chối, hoàn tác và hành vi xoá rồi gõ lại.

Một phép sửa có thể đi qua ba mức:

```text
Bỏ qua → Gợi ý → Tự sửa
```

## Những điểm cần làm rõ khi refactor

1. Hiện có hai con đường tự sửa: tự sửa do model đã học đủ và tự sửa theo luật “chỉ có một đáp án”. Hai con đường này cần đi qua một nơi quyết định thống nhất.
2. Bộ nhớ thích nghi và cặp sửa cá nhân đang có hai vòng đời khác nhau; nên hợp nhất hoặc xác định ranh giới thật rõ.
3. Hệ thống có lưu từ đứng trước nhưng đang gộp điểm giữa các ngữ cảnh khi ra quyết định.
4. Hoàn tác cần phân biệt với từ chối lâu dài: hoàn tác tức thời trước hết phải quay lui thay đổi vừa ghi; nhiều lần hoàn tác mới nên trở thành tín hiệu không ưa thích.
5. Thao tác “quên” cần xoá dữ liệu thật khỏi dữ liệu lưu, không chỉ ẩn quy tắc khỏi giao diện.
6. Cần giới hạn và dọn các quy tắc cũ/yếu để model không tăng mãi.

## Bài học chính từ các dự án mã nguồn mở

Nên tách hệ thống thành bốn trách nhiệm:

```text
Bộ sinh phương án
→ Sổ ghi nhớ phép sửa
→ Bộ hiểu thói quen dùng từ/ngữ cảnh
→ Người quyết định Bỏ qua/Gợi ý/Tự sửa
```

- Sổ ghi nhớ phép sửa trả lời: “Người dùng có muốn `khogn → không` không?”
- Bộ hiểu ngữ cảnh trả lời: “Sau từ này, từ nào thường phù hợp hơn?”
- Hiểu ngữ cảnh chỉ nên hỗ trợ sinh và xếp hạng gợi ý; không được tự mình cho phép tự sửa.
- Mọi tự sửa cần có lý do rõ ràng và luôn hoàn tác được.

## Vị trí của học máy

Theo nghĩa rộng, OpenViKey đã là một hệ thống học thích nghi tại chỗ: dữ liệu hành vi làm thay đổi trạng thái và quyết định tương lai. Hiện tại đây là mô hình thống kê nhỏ, minh bạch và tất định; không phải mạng nơ-ron và không cần dịch vụ đám mây.

Cần phân biệt:

1. **Tham số chung của sản phẩm:** trọng số tín hiệu, ngưỡng gợi ý/tự sửa, thời gian giảm ảnh hưởng. Các giá trị này được cố định theo phiên bản và hiệu chỉnh bằng dữ liệu thử nghiệm chung.
2. **Trạng thái riêng của người dùng:** số lần đồng ý, từ chối, hoàn tác, thời điểm sử dụng và tần suất từ/ngữ cảnh. Phần này thay đổi theo quá trình sử dụng.

Trong giai đoạn gần, không nên để mỗi người dùng tự làm thay đổi các trọng số chung. Chỉ nên cập nhật trạng thái cá nhân có thể giải thích được, còn các trọng số chung phải được kiểm thử, hiệu chỉnh ngoại tuyến và quản lý phiên bản.

## Hướng triển khai từng bước

1. **Bước 1:** hợp nhất luồng quyết định và làm rõ vòng đời dữ liệu học hiện tại.
2. **Bước 2:** thêm thống kê từ đơn và cặp từ đứng cạnh nhau để xếp hạng gợi ý tốt hơn.
3. **Bước 3:** học các kiểu lỗi gõ tổng quát của người dùng, ban đầu chỉ chạy quan sát và không tự sửa.
4. **Chưa làm:** mô hình khó giải thích hoặc cơ chế tự thử nghiệm bằng cách sửa chữ của người dùng.

Nguyên tắc an toàn: luật bảo mật và giới hạn nguồn sửa là cố định; dữ liệu học chỉ được ảnh hưởng trong phạm vi các giới hạn đó.
