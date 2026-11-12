# paper2html

Công cụ viết bằng Rust để chuyển bài báo khoa học dạng PDF (lấy từ arXiv hoặc file có sẵn)
sang HTML đẹp, dễ đọc, câu chữ liền mạch — phù hợp làm đầu vào cho các công cụ dịch thuật.

- **Thuần Rust**, một file thực thi duy nhất, không cần cài `poppler`, `mupdf` hay Python.
  Phần đọc/render PDF dùng [hayro](https://github.com/LaurenzV/hayro).
- **Câu chữ liền mạch**: nối dòng thành đoạn, bỏ gạch nối cuối dòng (`multi-`/`plicative` →
  `multiplicative`), nối đoạn văn bị cắt qua cột và qua trang, bỏ header/footer/số trang,
  tách footnote ra khỏi mạch văn.
- **Hình ảnh rõ nét**: hình, bảng và công thức được cắt trực tiếp từ PDF gốc (vector) và
  render ở độ phân giải cao (mặc định 216 dpi); chế độ `hard` dùng SVG nên phóng to bao nhiêu
  cũng nét.
- **Ba chế độ** trung thành với bản gốc ở mức khác nhau (xem bên dưới).
- Nhanh: bài 15 trang mất khoảng 1–2 giây cho cả ba chế độ.

## Ba chế độ

| Chế độ | Bố cục | Văn bản | Dùng khi |
| --- | --- | --- | --- |
| `hard` | Giống 1-1 từng trang (trang gốc vẽ lại bằng SVG/PNG) | Lớp chữ ẩn, chọn/copy/tìm kiếm được, tổ chức theo từng đoạn | Cần đối chiếu chính xác với bản PDF |
| `medium` | Giữ trang, số cột, vị trí tương đối của hình/bảng | HTML thật, tự dàn lại; đoạn văn không bị cắt giữa cột/trang | Muốn "cảm giác bài báo" nhưng chữ là chữ thật |
| `friendly` | Một cột, responsive, có mục lục, dark mode | HTML thật, ưu tiên dễ đọc | Đọc trên web/điện thoại, đưa vào công cụ dịch |

Ở cả ba chế độ, văn bản của một đoạn luôn nằm liền nhau trong DOM:

- `friendly`, `medium`: mỗi đoạn là một thẻ `<p>`; heading là `<h2>`–`<h5>`; chú thích hình là
  `<figcaption>`; tài liệu tham khảo là `<ol class="refs">`.
- `hard`: mỗi đoạn là một `<div class="b" data-kind="paragraph" data-block="N">` chứa các dòng;
  phần đoạn văn vắt sang trang sau mang cùng `data-block` và có `data-continued="1"`.

## Cài đặt

```bash
cargo build --release
```

File thực thi nằm ở `target/release/paper2html`.

## Sử dụng

Chuyển một bài trên arXiv (tự tải PDF về, có cache):

```bash
paper2html convert 1706.03762
```

Chọn chế độ và file đầu ra:

```bash
paper2html convert 1706.03762 --mode medium -o attention.html
```

Xuất cả ba chế độ vào một thư mục, kèm JSON cấu trúc:

```bash
paper2html convert paper.pdf --all --json -o out/
```

Đầu vào có thể là file PDF, mã arXiv (`1706.03762`, `arXiv:1706.03762v7`, `hep-th/9901001`)
hoặc URL arXiv (`https://arxiv.org/abs/1706.03762`).

| Tuỳ chọn | Ý nghĩa |
| --- | --- |
| `-m, --mode <hard\|medium\|friendly>` | Chế độ (mặc định `friendly`) |
| `--all` | Xuất cả ba chế độ: `<tên>.hard.html`, `<tên>.medium.html`, `<tên>.friendly.html` |
| `-o, --output <đường dẫn>` | File HTML, hoặc thư mục |
| `--images <inline\|files>` | `inline` (mặc định): nhúng ảnh vào HTML, một file duy nhất. `files`: ghi ảnh ra thư mục `<tên>_assets/` |
| `--scale <số>` | Độ phân giải ảnh hình/công thức, pixel trên mỗi point PDF (mặc định 3 ≈ 216 dpi) |
| `--page-image <svg\|png>` | Nền trang ở chế độ `hard` (mặc định `svg`) |
| `--json` | Ghi thêm `<tên>.json` — cấu trúc tài liệu |

Các lệnh khác:

```bash
paper2html fetch 1706.03762 -o attention.pdf
```

```bash
paper2html inspect paper.pdf
```

`fetch` chỉ tải PDF. `inspect` in ra cấu trúc nhận diện được (tiêu đề, heading, đoạn, hình,
công thức…), tiện để kiểm tra khi kết quả chưa như ý.

PDF tải từ arXiv được cache ở `~/.cache/paper2html` (đổi bằng biến môi trường
`PAPER2HTML_CACHE`).

## JSON cho công cụ dịch

`--json` ghi ra danh sách block theo đúng thứ tự đọc. Mỗi block có `type`
(`title`, `authors`, `heading`, `paragraph`, `list_item`, `equation`, `figure`, `footnote`,
`reference`, `note`), số trang `page` (đếm từ 0), `bbox` và `spans`:

```json
{
  "id": 22,
  "type": "paragraph",
  "in_abstract": false,
  "page": 1,
  "bbox": { "x0": 107.7, "y0": 389.5, "x1": 505.2, "y1": 486.7 },
  "spans": [
    { "text": "The goal of reducing sequential computation ... Extended Neural GPU [", "style": 0 },
    { "text": "16", "style": 0, "href": "#ref-16" }
  ]
}
```

`style` là tổ hợp bit: 1 = đậm, 2 = nghiêng, 4 = monospace, 8 = font toán, 16 = chỉ số trên,
32 = chỉ số dưới. Nối `text` của các span là ra nguyên văn đoạn. Công thức (`equation`) và hình
(`figure`) mang toạ độ vùng ảnh trên trang; `spans` của hình là chú thích.

## Dùng như thư viện

```rust
use paper2html::{HtmlOptions, Mode, Paper};

let paper = Paper::from_bytes(std::fs::read("paper.pdf")?)?;
let analysis = paper.analyze();            // phân tích một lần
for block in &analysis.doc.blocks {
    println!("{:?}: {}", block.kind, block.text());
}
let out = paper.to_html(&analysis, &HtmlOptions { mode: Mode::Friendly, ..Default::default() });
std::fs::write("paper.html", out.html)?;
```

## Cách hoạt động

```
PDF ──► trích xuất ──► dòng chữ ──► hình/bảng ──► thứ tự đọc ──► block ──► tài liệu ──► HTML
        (pdf/)        (layout/lines) (floats)     (order)       (blocks)  (document)   (html/)
```

1. **Trích xuất** (`src/pdf`): chạy trình thông dịch PDF, ghi lại từng glyph (ký tự Unicode, vị
   trí, cỡ chữ, font), ảnh và hình vẽ vector. Tên font cho biết đậm/nghiêng/monospace/font toán.
2. **Dòng chữ** (`layout/lines.rs`): gom glyph theo baseline, tách từ theo khoảng trống, gắn chỉ
   số trên/dưới vào dòng, ghép dấu rời của TeX (`¨` + `o` → `ö`).
3. **Hình và bảng** (`layout/floats.rs`): tìm chú thích (`Figure 3:`, `Table 1.`), gom hình vẽ
   thành cụm, ghép mỗi chú thích với cụm gần nhất, nhận diện bảng qua các đường kẻ ngang. Chữ nằm
   trong hình/bảng bị loại khỏi mạch văn; cả vùng được render thành ảnh.
4. **Thứ tự đọc** (`layout/order.rs`): cắt trang thành các vùng ngang, mỗi vùng một hoặc nhiều
   cột (tìm khe giữa các cột), đệ quy.
5. **Block** (`layout/blocks.rs`): tách đoạn văn (thụt đầu dòng, dòng cuối ngắn, khoảng cách dọc),
   nhận diện công thức hiển thị (render thành ảnh nền trong suốt) và footnote.
6. **Tài liệu** (`document.rs`): tiêu đề, tác giả, abstract, cấp heading, danh sách, tài liệu tham
   khảo; nối đoạn qua cột/trang; bỏ gạch nối cuối dòng dựa trên từ vựng của chính bài báo; tạo
   liên kết chéo (`[12]`, `Figure 2`, `Section 3.1`, `Eq. (4)`, footnote, URL).
7. **HTML** (`src/html`): ba renderer dùng chung một mô hình tài liệu (`model.rs`).

## Giới hạn hiện tại

- **Bảng và công thức hiển thị được xuất dưới dạng ảnh** (nét, đúng như bản gốc) chứ không phải
  HTML/MathML, nên công cụ dịch sẽ không dịch nội dung bên trong. Chú thích bảng/hình là chữ thật.
- Công thức *trong dòng* là chữ thật với `<sub>`/`<sup>`; biểu thức phức tạp (phân số, căn nhiều
  tầng) có thể bị dàn phẳng.
- Trích dẫn kiểu tác giả–năm ("Vaswani et al., 2017") chưa được liên kết tới mục tham khảo; kiểu
  số `[12]` thì có.
- PDF scan (chỉ có ảnh, không có lớp chữ) và PDF mã hoá chưa được hỗ trợ.
- Bố cục được kiểm thử chủ yếu với bài 1–2 cột của arXiv (NeurIPS, ICLR, CVPR, ACL, IEEE, APS);
  bố cục lạ có thể nhận diện sai — dùng `inspect` để xem.

## Kiểm thử

```bash
cargo test
```

Bộ test gồm test đơn vị (bỏ gạch nối, liên kết chéo, nhận diện font, mã arXiv) và test đầu–cuối
trên một PDF nhỏ được sinh ngay trong test.

## Giấy phép

MIT. Các thư viện phụ thuộc chính (hayro, vello) dùng giấy phép Apache-2.0 hoặc MIT.
