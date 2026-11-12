# paper2html

Công cụ viết bằng Rust để chuyển bài báo khoa học dạng PDF (lấy từ arXiv hoặc file có sẵn)
sang HTML đẹp, dễ đọc, câu chữ liền mạch — phù hợp làm đầu vào cho các công cụ dịch thuật.

- **Thuần Rust**, một file thực thi duy nhất, không cần cài `poppler`, `mupdf` hay Python.
  Phần đọc/render PDF dùng [hayro](https://github.com/LaurenzV/hayro).
- **Câu chữ liền mạch**: nối dòng thành đoạn, bỏ gạch nối cuối dòng (`multi-`/`plicative` →
  `multiplicative`), nối đoạn văn bị cắt qua cột và qua trang, bỏ header/footer/số trang,
  tách footnote ra khỏi mạch văn.
- **Bảng là bảng thật**: hàng, cột, ô gộp ngang/dọc và đường kẻ được dựng lại thành `<table>`.
- **Hình ảnh rõ nét**: hình vẽ giữ dạng vector (SVG) hoặc render ở độ phân giải cao (mặc định
  216 dpi).
- **Bốn chế độ** trung thành với bản gốc ở mức khác nhau (xem bên dưới).
- Nhanh: bài 15 trang mất khoảng 1–2 giây cho cả bốn chế độ.

## Bốn chế độ

| Chế độ | Bố cục | Văn bản | Dùng khi |
| --- | --- | --- | --- |
| `hard` | Giống 1-1 từng trang (trang gốc vẽ lại bằng SVG/PNG) | Lớp chữ ẩn, chọn/copy/tìm kiếm được | Cần đối chiếu chính xác với bản PDF |
| `hard-95` | Như `hard`: từng trang, đúng vị trí, đúng ngắt dòng | **Chữ HTML thật, hiển thị**; bảng là `<table>`; công thức là chữ | Muốn giống bản gốc mà vẫn dịch/sửa/copy được |
| `medium` | Giữ trang, số cột, vị trí tương đối của hình/bảng | HTML thật, tự dàn lại; đoạn văn không bị cắt giữa cột/trang | Muốn "cảm giác bài báo", chữ dàn tự nhiên |
| `friendly` | Một cột, responsive, có mục lục, dark mode | HTML thật, ưu tiên dễ đọc | Đọc trên web/điện thoại, đưa vào công cụ dịch |

Ở cả bốn chế độ, văn bản của một đoạn luôn nằm liền nhau trong DOM:

- `friendly`, `medium`: mỗi đoạn là một thẻ `<p>`; heading là `<h2>`–`<h5>`; chú thích hình là
  `<figcaption>`; tài liệu tham khảo là `<ol class="refs">`; bảng là `<table class="tbl">`.
- `hard-95`: mỗi đoạn là một `<div class="fb" data-kind="paragraph" data-block="N">`.
- `hard`: mỗi đoạn là một `<div class="b" data-kind="paragraph" data-block="N">` chứa các dòng;
  phần đoạn văn vắt sang trang sau mang cùng `data-block` và có `data-continued="1"`.

### `hard-95` hoạt động thế nào

Mỗi trang được vẽ lại từ PDF **sau khi gỡ hết chữ**, nên nền trang chỉ còn nét vẽ thật (đồ thị,
sơ đồ, ảnh, đường kẻ, gạch phân số). Chữ được đặt lại bằng HTML đúng chỗ cũ:

- **Đoạn văn, heading, chú thích, footnote, tài liệu tham khảo**: chữ thật trong khung đúng vị
  trí, ngắt dòng y như PDF, đúng cỡ chữ, giãn dòng, màu chữ, đậm/nghiêng. Đoạn vắt qua cột hoặc
  qua trang vẫn là **một phần tử duy nhất** trong DOM; khung ở cột/trang sau hiển thị phần tiếp
  theo (bản sao bị cắt khung, đánh dấu `translate="no"`).
- **Bảng**: `<table>` thật đặt trên đúng lưới hàng/cột gốc, đường kẻ vẽ bằng CSS. Bảng nào không
  dựng được lưới thì chữ từng ô vẫn là chữ thật đặt đúng vị trí.
- **Công thức hiển thị**: chữ thật, đặt theo từng cụm ký hiệu (tử số, mẫu số, chỉ số, cận tổng).
  Ký hiệu lớn (∑, ngoặc cao, dấu căn) được co giãn cho khớp kích thước gốc.
- **Chữ trong hình, chữ xoay (nhãn trục), số trang, header**: chữ thật đặt đúng vị trí và góc.

Khi nội dung một khối bị thay đổi (ví dụ sau khi dịch), khối đó bỏ ngắt dòng gốc và tự dàn lại
trong khung của nó, thu nhỏ cỡ chữ nếu bản dịch dài hơn. Phần này cần JavaScript; không có
JavaScript trang vẫn hiển thị đúng, chỉ là độ rộng dòng không được căn khít.

Thứ duy nhất không thành chữ: ký tự mà PDF không cho biết mã Unicode (giữ nguyên dạng hình).

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

Xuất cả bốn chế độ vào một thư mục, kèm JSON cấu trúc:

```bash
paper2html convert paper.pdf --all --json -o out/
```

Đầu vào có thể là file PDF, mã arXiv (`1706.03762`, `arXiv:1706.03762v7`, `hep-th/9901001`)
hoặc URL arXiv (`https://arxiv.org/abs/1706.03762`).

| Tuỳ chọn | Ý nghĩa |
| --- | --- |
| `-m, --mode <hard\|hard-95\|medium\|friendly>` | Chế độ (mặc định `friendly`) |
| `--all` | Xuất cả bốn chế độ: `<tên>.hard.html`, `<tên>.hard-95.html`, `<tên>.medium.html`, `<tên>.friendly.html` |
| `-o, --output <đường dẫn>` | File HTML, hoặc thư mục |
| `--images <inline\|files>` | `inline` (mặc định): nhúng ảnh vào HTML, một file duy nhất. `files`: ghi ảnh ra thư mục `<tên>_assets/` |
| `--scale <số>` | Độ phân giải ảnh hình/công thức, pixel trên mỗi point PDF (mặc định 3 ≈ 216 dpi) |
| `--page-image <svg\|png>` | Nền trang ở chế độ `hard` (mặc định `svg`) |
| `--tables <auto\|image>` | `auto` (mặc định): dựng `<table>` khi cấu trúc bảng rõ ràng. `image`: không dựng (bảng là ảnh ở `medium`/`friendly`, là chữ đặt theo vị trí ở `hard-95`) |
| `--json` | Ghi thêm `<tên>.json` — cấu trúc tài liệu |
| `-j, --jobs <N>` | Số luồng xử lý song song. `0` (mặc định): một luồng mỗi nhân CPU. `1`: không song song. Kết quả giống hệt nhau với mọi giá trị |

Các lệnh khác:

```bash
paper2html fetch 1706.03762 -o attention.pdf
```

```bash
paper2html inspect paper.pdf
```

`fetch` chỉ tải PDF. `inspect` in ra cấu trúc nhận diện được (tiêu đề, heading, đoạn, hình,
công thức…), tiện để kiểm tra khi kết quả chưa như ý.

### Tốc độ và tài nguyên

Đo trên máy 4 vCPU, PDF có sẵn trên đĩa, một chế độ mỗi lần chạy:

| Bài | Trang | 1 luồng | 4 luồng | RAM đỉnh (4 luồng) |
| --- | --- | --- | --- | --- |
| Attention (1706.03762) | 15 | 0,34–0,69 s | 0,20–0,36 s | 73–84 MB |
| ViT (2010.11929) | 22 | 0,61–0,70 s | 0,36–0,38 s | 77–95 MB |
| Faster R-CNN (1506.01497), nhiều ảnh chụp | 14 | 0,74–2,44 s | 0,41–1,21 s | 92–156 MB |
| Báo cáo GPT-4 (2303.08774) | 100 | 1,73–1,97 s | 0,91–1,00 s | 152–211 MB |

Mỗi trang được đọc và vẽ độc lập trên một luồng, mỗi luồng có cache riêng nên RAM tăng theo
số luồng. Bố cục và việc dựng HTML vẫn chạy một luồng. Trên máy ít RAM, dùng `-j 1` hoặc `-j 2`.

Khi mở file, ảnh nền trang của `hard`/`hard-95` chỉ được vẽ khi cuộn gần tới (lazy loading),
nên bài dài mở nhanh: báo cáo GPT-4 100 trang ở `hard` từ khoảng 7 s xuống khoảng 1–1,5 s. Chữ
của `hard-95` luôn có đủ ngay từ đầu, công cụ dịch không phải chờ cuộn. Khi in, trình duyệt tự
vẽ đủ mọi trang.

Những điểm đã đo nhưng chưa cải thiện (ảnh bị nén hai lần ở `hard`/`hard-95`, in trang khác khổ
giấy…) được ghi trong [docs/IMPROVEMENTS.md](docs/IMPROVEMENTS.md).

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
32 = chỉ số dưới, 64 = sans-serif. Nối `text` của các span là ra nguyên văn đoạn. Công thức
(`equation`) và hình (`figure`) mang toạ độ vùng ảnh trên trang; `spans` của hình là chú thích.

Bảng dựng được cấu trúc có thêm trường `table`:

```json
{
  "type": "figure", "kind": "table", "label": "Table 2",
  "table": {
    "n_cols": 5,
    "header_rows": 2,
    "col_align": ["left", "center", "center", "center", "center"],
    "rows": [
      [ { "col": 0, "colspan": 1, "rowspan": 2, "spans": [ { "text": "Model", "style": 0 } ] },
        { "col": 1, "colspan": 2, "rowspan": 1, "spans": [ { "text": "BLEU", "style": 0 } ] } ]
    ],
    "rule_above": [[2, 2, 2, 2, 2]],
    "rule_bottom": [2, 2, 2, 2, 2]
  }
}
```

`rule_above[hàng][cột]` là đường kẻ phía trên ô (0 = không, 1 = mảnh, 2 = đậm).

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
   trong hình/bảng bị loại khỏi mạch văn.
   **Cấu trúc bảng** (`layout/tables.rs`): hàng theo baseline, cột theo các "hành lang" trống chạy
   dọc bảng; ô vắt qua hành lang thành ô gộp ngang, nhãn nằm giữa các hàng thành ô gộp dọc, đường
   kẻ gắn vào cạnh ô. Nội dung không giống lưới (ví dụ ô chứa ma trận) thì không dựng.
4. **Thứ tự đọc** (`layout/order.rs`): cắt trang thành các vùng ngang, mỗi vùng một hoặc nhiều
   cột (tìm khe giữa các cột), đệ quy.
5. **Block** (`layout/blocks.rs`): tách đoạn văn (thụt đầu dòng, dòng cuối ngắn, khoảng cách dọc),
   nhận diện công thức hiển thị (render thành ảnh nền trong suốt) và footnote.
6. **Tài liệu** (`document.rs`): tiêu đề, tác giả, abstract, cấp heading, danh sách, tài liệu tham
   khảo; nối đoạn qua cột/trang; bỏ gạch nối cuối dòng dựa trên từ vựng của chính bài báo; tạo
   liên kết chéo (`[12]`, `Figure 2`, `Section 3.1`, `Eq. (4)`, footnote, URL).
7. **HTML** (`src/html`): bốn renderer dùng chung một mô hình tài liệu (`model.rs`). Riêng
   `hard-95` lấy nền trang từ SVG đã gỡ chữ (`pdf/render.rs`) rồi đặt chữ HTML lên trên.

## Giới hạn hiện tại

- **Bảng**: bảng dạng lưới dựng được thành `<table>`; trên 8 bài thử là 52/67 bảng có chú thích
  (44/48 nếu không tính báo cáo GPT-4, vốn có nhiều bảng mà ô là cả đoạn văn). Bảng có ô chứa
  ma trận/ngoặc lớn, ô là đoạn văn dài, hoặc không đủ đều thì không dựng: ở `medium` và `friendly`
  giữ ảnh, ở `hard-95` là chữ đặt theo vị trí. Ô có nhiều dòng chữ hiện thành nhiều hàng. Màu nền
  ô không được giữ.
- **Công thức hiển thị**: là chữ thật ở `hard-95`; ở `medium` và `friendly` vẫn là ảnh.
- **Font**: `hard-95` dùng font có sẵn trên máy người xem (họ Times/Helvetica/Courier), không nhúng
  font của PDF, nên nét chữ có thể hơi khác bản gốc; vị trí và độ rộng dòng thì được căn khớp.
- Công thức *trong dòng* là chữ thật với `<sub>`/`<sup>`; biểu thức phức tạp (phân số, căn nhiều
  tầng) có thể bị dàn phẳng.
- Ký tự mà PDF không cung cấp mã Unicode hiện thành `�` ở `medium`/`friendly`, và giữ dạng hình ở
  `hard-95`.
- Trích dẫn kiểu tác giả–năm ("Vaswani et al., 2017") chưa được liên kết tới mục tham khảo; kiểu
  số `[12]` thì có.
- PDF scan (chỉ có ảnh, không có lớp chữ) và PDF mã hoá chưa được hỗ trợ.
- Bố cục được kiểm thử chủ yếu với bài 1–2 cột của arXiv (NeurIPS, ICLR, CVPR, ACL, IEEE, APS);
  bố cục lạ có thể nhận diện sai — dùng `inspect` để xem.

## Kiểm thử

Tải trước các bài báo dùng để test (chỉ cần một lần), rồi chạy test:

```bash
scripts/fetch-test-pdfs.sh
```

```bash
cargo test
```

Có ba lớp test:

- **Test đơn vị:** bỏ gạch nối, liên kết chéo, nhận diện font, mã arXiv, chia việc song song.
- **Test đầu–cuối trên PDF nhỏ** sinh ngay trong test: cấu trúc tài liệu, dựng bảng, bốn chế độ,
  nền trang `hard-95` không còn ký tự, kết quả không đổi theo số luồng, ảnh nền tải lười.
- **Test trên 8 bài arXiv thật** (`tests/real_papers.rs`), ghim phiên bản trong
  `tests/data/papers.txt`. Với mỗi bài: tiêu đề, số bảng dựng được, số đoạn bị đứt giữa câu, số từ
  còn gạch nối, số ký tự còn sót trong nền trang `hard-95`, và cả bốn chế độ xuất được. Thêm vài
  kiểm tra cụ thể (cột EN-DE/EN-FR của bảng Transformer, dòng tiêu đề các bảng BERT ở đầu trang).
  Mốc là kết quả hiện tại: tốt hơn thì qua, tệ hơn thì đỏ. Khi cải thiện được, siết mốc lại
  (`P2H_PRINT_METRICS=1 cargo test --test real_papers -- --nocapture` in ra số đo).
- **Snapshot đầu ra** (cũng trong `tests/real_papers.rs`): mã sha256 của HTML từng chế độ và của
  JSON, cho từng bài, lưu trong `tests/data/snapshots/`. Đầu ra đổi dù chỉ một ký tự là test đỏ,
  kèm tên bài và chế độ bị đổi. Sắp xếp lại code thì snapshot phải giữ nguyên. Khi cố ý đổi
  đầu ra, cập nhật rồi commit cùng thay đổi code:

  ```bash
  P2H_UPDATE_SNAPSHOTS=1 cargo test --test real_papers
  ```

  Mã băm bỏ qua điểm ảnh của ảnh PNG/JPEG (có thể lệch vài đơn vị giữa các loại CPU), chỉ tính
  chữ, cấu trúc HTML, SVG vector và kích thước ảnh. Riêng bài LIGO (CC BY 4.0) lưu cả JSON đầy đủ,
  nên `git diff` cho thấy chính xác nội dung nào đã đổi.

Test không bao giờ tự tải gì. Chỉ bài LIGO (CC BY 4.0) nằm trong repo; các bài khác không được
phép phân phối lại nên phải tải bằng script trên. Bài nào chưa tải thì test bỏ qua và in ra danh
sách còn thiếu.

Phần JavaScript của `hard-95` (căn dòng, dàn lại chữ khi dịch) được kiểm tra trong trình duyệt:

```bash
node tests/browser/check-hard95.js out/*.hard-95.html
```

Cần gói `playwright` hoặc `playwright-core`. CI (`.github/workflows/ci.yml`) chạy `cargo fmt`,
`cargo clippy`, toàn bộ test và kiểm tra trình duyệt trên mỗi pull request. Các bài báo được giữ
trong bộ nhớ đệm riêng của repo trên GitHub, nên chỉ lần đầu phải tải.

`cargo run --example dump_tables -- paper.pdf` in các bảng dựng được của một PDF, tiện để soi lỗi
dựng bảng.

## Giấy phép

MIT. Các thư viện phụ thuộc chính (hayro, vello) dùng giấy phép Apache-2.0 hoặc MIT.
