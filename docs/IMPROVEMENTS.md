# Hướng cải thiện (chưa làm)

Ghi lại những điểm đã đo hoặc phát hiện nhưng chưa sửa, kèm số liệu và các phương án đã cân
nhắc, để quay lại sau.

## 1. Ảnh chụp bị nén hai lần ở `hard` / `hard-95`

**Hiện trạng.** Nền trang của `hard` và `hard-95` là SVG do thư viện `hayro-svg` vẽ. Thư viện
giải mã mọi ảnh trong PDF ra điểm ảnh rồi ghi lại thành PNG (an toàn với mọi định dạng ảnh,
hệ màu, mặt nạ trong suốt của PDF). Vì PNG của ảnh chụp rất nặng, `compact_images` trong
`src/pdf/render.rs` giải mã lại các PNG lớn và nén thành JPEG (chất lượng 88, cạnh dài tối đa
2600 px). Kết quả: ảnh vốn là JPEG trong PDF bị nén JPEG lần thứ hai, và tốn một vòng
JPEG → điểm ảnh → PNG → điểm ảnh → JPEG.

**Số liệu** (bài Faster R-CNN, 1506.01497, đo bằng callgrind, một luồng): khoảng 70% thời gian
convert `hard-95` nằm ở vòng này: mã hoá PNG trong thư viện ≈ 12%, giải mã PNG ≈ 16%, mã hoá
JPEG ≈ 40%. Thời gian: 2,4 s (1 luồng), 1,2 s (4 luồng); `medium`/`friendly` chỉ 0,7–0,8 s.

Số ảnh có thể giữ nguyên JPEG gốc (JPEG, RGB hoặc xám, không có mặt nạ trong suốt):

| Bài | Ảnh đủ điều kiện / tổng |
| --- | --- |
| Faster R-CNN (1506.01497) | khoảng 520 / 681 |
| ViT (2010.11929) | 254 / 304 |
| GPT-4 (2303.08774) | 7 / 13 |
| BERT (1810.04805) | 0 / 68 (ảnh JPEG đều có mặt nạ trong suốt) |
| Attention, Adam, ResNet, LIGO | không có ảnh JPEG |

**Vì sao không làm được từ bên ngoài thư viện.** `hayro-svg` không công khai phần vẽ ảnh
(`SvgRenderer` là kiểu nội bộ, ảnh được giải mã trước khi tới bất kỳ chỗ nào mình can thiệp
được). `hayro-interpret` thì có cho lấy dữ liệu gốc: `RasterImage::stream()`.

**Các phương án:**

| | Cách | Sửa thư viện? | Lợi | Hại |
| --- | --- | --- | --- | --- |
| A | Thay bộ mã hoá JPEG nhanh hơn cho bước nén lại | Không | Ước tính nhanh hơn 20–30% ở bài nhiều ảnh | Vẫn nén hai lần |
| B | Trước khi vẽ, thay mỗi ảnh JPEG trong PDF bằng ảnh 1 điểm ảnh mang mã số (bằng incremental update), vẽ SVG, rồi thay lại bằng JPEG gốc | Không | Gần bằng D | Phức tạp, dễ lỗi với PDF lạ, khó bảo trì |
| C | Gửi pull request cho hayro: tuỳ chọn nhúng JPEG gốc khi được | Không (sửa ở thượng nguồn) | Sạch nhất | Phụ thuộc vào việc họ có nhận không |
| D | Chép `hayro-svg` vào repo (`[patch.crates-io]`), sửa khoảng 30 dòng trong `draw_image` | Có | Nhanh, ảnh giống hệt PDF, file nhỏ hơn | Phải áp lại bản sửa mỗi lần nâng cấp hayro |

Giấy phép của `hayro-svg` là `Apache-2.0 OR MIT`, nên D hợp lệ nếu giữ thông báo bản quyền,
kèm `LICENSE-MIT`/`LICENSE-APACHE` và ghi rõ file nào đã sửa.

Hai điểm đánh đổi chung khi nhúng JPEG gốc: màu có thể lệch rất nhẹ khi PDF gắn hồ sơ màu
ICC cho ảnh (trình duyệt bỏ qua hồ sơ đó); JPEG CMYK phải loại trừ (trình duyệt hay hiển thị
sai màu).

**Quyết định hiện tại:** giữ nguyên. Hướng ưu tiên khi quay lại: A + C.

## 2. Song song hoá chưa hết

`-j/--jobs` chạy song song phần đọc trang và vẽ trang/ảnh. Phân tích bố cục và dựng HTML vẫn
chạy một luồng, nên trên 4 nhân chỉ nhanh hơn 1,7–2,7 lần. Bài có vài trang nhiều ảnh bị
lệch tải (một luồng ôm trang nặng nhất).

## 3. In trang có khổ khác giấy in

Khi in `hard`/`hard-95` (Ctrl+P), mỗi trang được kéo theo chiều ngang của giấy. Trang có tỉ lệ
hẹp hơn khổ Letter (ví dụ Faster R-CNN, 567 × 774 pt) bị cao hơn tờ giấy nên tràn sang tờ sau:
14 trang thành 28 tờ. Cách sửa: trong `@media print`, giới hạn trang theo cả chiều cao, hoặc đặt
`@page { size: <khổ của PDF> }`.

## 4. Còn sót trong `hard-95`

- Bài ViT (2010.11929) còn khoảng 2381 ký tự vẫn nằm trong ảnh nền thay vì là chữ HTML; chưa
  tìm hiểu nguyên nhân. Các bài khác: BERT 0, LIGO 3, Attention 85, Faster R-CNN 188.
- Một số dòng đặt theo vị trí (`.pl`) chưa khớp độ rộng gốc (Adam 49 dòng, ViT 36 dòng).

## 5. `medium` / `friendly`

- Công thức hiển thị (display equation) vẫn là ảnh.

## 6. Tài liệu rất dài (khoảng 1000 trang)

Thử với "Dive into Deep Learning" (arXiv 2106.11342, 1185 trang, PDF 39,5 MB), máy 4 vCPU:

| Chế độ | 4 luồng | 1 luồng | RAM đỉnh | File ra | Mở trong Chromium |
| --- | --- | --- | --- | --- | --- |
| `friendly` | 7,9 s | 20,5 s | 0,9–1,1 GB | 105 MB | 5,4 s |
| `medium` | 7,5 s | – | 1,1 GB | 105 MB | – |
| `hard-95` | 10,9 s | 25,1 s | 1,0–1,2 GB | 142 MB | 9,7 s |
| `hard` | 10,3 s | – | 1,7 GB | 481 MB | 13,7 s |

- **RAM:** riêng khâu đọc và phân tích đã cần khoảng 900 MB, vì toàn bộ ký tự của mọi trang được giữ
  trong bộ nhớ cùng lúc (mỗi ký tự là một struct có `String` riêng). Hướng sửa: biểu diễn ký tự
  gọn hơn; bỏ dữ liệu thô của trang ngay sau khi dựng dòng; ghi HTML ra file dần thay vì giữ cả
  chuỗi.
- **Mở file:** một file HTML 100–500 MB mở chậm (5–14 s). Hướng sửa: tách tài liệu dài thành nhiều
  file theo chương, có mục lục.

## 7. Xử lý hàng loạt

- Chưa có lệnh xử lý nhiều tài liệu. Hiện phải dùng script ngoài, ví dụ
  `ls *.pdf | xargs -P 2 -I{} paper2html convert {} -m hard-95 -j 1 -o out/`.
  Nên có lệnh `batch`: chạy song song N tài liệu, bỏ qua tài liệu đã xong, ghi log lỗi.
- Tải từ arXiv chưa giới hạn tốc độ. arXiv yêu cầu công cụ tự động tải chậm (hướng dẫn API của họ
  là khoảng 1 yêu cầu mỗi 3 giây); tải hàng nghìn bài liên tục có thể bị chặn. Cần thêm giới hạn
  tốc độ, hoặc dùng kho dữ liệu hàng loạt (bulk data) của arXiv.
