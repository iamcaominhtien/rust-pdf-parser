# Kiến trúc

`paper2html` là một **pipeline**: PDF đi qua một chuỗi giai đoạn, mỗi giai đoạn nhận một dạng dữ
liệu rõ ràng và trả ra dạng tiếp theo. Phần phân tích (PDF → `Document`) và phần xuất (`Document`
→ HTML) gặp nhau ở đúng một chỗ: mô hình `Document` trong `model`.

```
 PDF bytes
    │
    ▼  backend/          Adapter quanh thư viện hayro: thứ duy nhất biết đến hayro
 RawPage[] + FontInfo[]  (ký tự, ảnh, nét vẽ của từng trang; bảng font)
    │
    ▼  analysis/pipeline.rs   một hàm, đọc từ trên xuống là thấy toàn bộ luồng
    │   page        PageData, thống kê cỡ chữ/khoảng dòng của tài liệu
    │   lines       ký tự → dòng (chỉ số trên/dưới, dấu, chữ xoay)
    │   furniture   bỏ header/footer/số trang lặp lại
    │   floats      hình, bảng, thuật toán + chú thích
    │   tables      lưới hàng/cột của bảng
    │   order       thứ tự đọc (cột, vùng)
    │   blocks      dòng → khối thô (đoạn, công thức, chú thích chân trang)
    │   semantics/  phân loại (tiêu đề, tác giả, heading, …), nối đoạn,
    │               liên kết tham chiếu, dựng Document
    ▼
 Document (model)       ranh giới cố định giữa phân tích và xuất HTML
    │
    ▼  render/        trait Renderer: mỗi chế độ một implementation
 hard │ hard-95 │ medium │ friendly        dùng chung: assets, escape, flow, table
```

## Module

| Module | Vai trò |
| --- | --- |
| `lib.rs` | API công khai: `Paper`, `convert`, các kiểu Options, `model` |
| `main.rs` | CLI: chỉ đọc tham số rồi gọi API |
| `paper.rs` | `Paper`: nạp PDF, chạy phân tích, xuất HTML |
| `config.rs` | `AnalyzeOptions` và `LayoutConfig` (mọi ngưỡng của thuật toán) |
| `source/` | Nguồn PDF: `arxiv` (nhận dạng mã, tải, cache) |
| `backend/` | Adapter quanh hayro: đọc trang (`extract`), nhận font (`fonts`), vẽ trang/vùng (`render`) |
| `common/` | Dùng chung, không phụ thuộc module nào khác: hình học, kiểu chữ, xử lý chữ, chạy song song |
| `analysis/` | Các giai đoạn phân tích, điều phối bởi `analysis/pipeline.rs` |
| `model/` | `Document` và các kiểu con: dữ liệu ra của phân tích, dữ liệu vào của xuất HTML, cũng là JSON |
| `render/` | Xuất HTML: `Renderer` cho từng chế độ, kho ảnh `assets`, phần HTML dùng chung |

Hướng phụ thuộc chỉ đi một chiều:

```
main → paper → analysis → backend → common
              → render  → model   → common
```

`render` không biết gì về `analysis` ngoài `Analysis`/`Document`, còn `analysis` không biết gì về
HTML.

## Pattern

| Pattern | Ở đâu | Để làm gì |
| --- | --- | --- |
| Pipeline | `analysis/pipeline.rs` | Thấy toàn bộ luồng ở một chỗ; mỗi giai đoạn test được riêng |
| Configuration Object | `config::LayoutConfig`, mỗi giai đoạn một struct con | Mọi ngưỡng có tên, có doc, có đơn vị, nằm ở một chỗ |
| Rules (chuỗi quy tắc) | Phân loại khối trong `analysis/blocks` và `analysis/semantics` | Mỗi quy tắc là một hàm nhỏ; thêm/sửa một quy tắc không đụng quy tắc khác |
| Strategy | `render::Renderer` | Bốn chế độ chung một giao diện; thêm chế độ = thêm một module |
| Adapter | `backend/` | Chỉ một chỗ phụ thuộc vào hayro |
| Builder | `analysis/semantics/text.rs` (`TextBuilder`), dựng `Document` | Dựng dữ liệu phức tạp từng bước |

Không dùng cây kế thừa hay Visitor: `enum` + `match` là cách tự nhiên của Rust cho các loại khối.

## Quy ước code

**Thứ tự trong một file:** doc của module (`//!`) → `use` → hằng số và regex → kiểu dữ liệu (kèm
`impl`) → hàm public → hàm phụ private → `#[cfg(test)] mod tests`.

**Ngưỡng và hằng số.**
- Ngưỡng của thuật toán (con số do lựa chọn mà có, ví dụ "khoảng cách quá 1,45 lần cỡ chữ thì
  tách") là một trường có doc trong struct config của giai đoạn đó, tên có đơn vị:
  `_em` (bội của cỡ chữ), `_pt` (point PDF), `_ratio` (tỉ lệ 0–1).
- Hằng số không phải ngưỡng (đơn vị, kích thước bộ đệm) là `const` có doc ở đầu file.
- Những con số mang nghĩa toán học hiển nhiên (`0.5` lấy trung điểm, `2.0` gấp đôi, `100.0` ra
  phần trăm) để nguyên.
- Thứ dùng ở nhiều module (cờ kiểu chữ, regex nhận chú thích) nằm trong `common/`.

**Hàm.** Tối đa khoảng 80 dòng; dài hơn thì tách thành các bước có tên. Mỗi hàm làm một việc nói
được bằng một câu.

**Tên.** Tên có nghĩa (`line`, `block`, `line_idx`), không dùng một chữ cái, trừ chỉ số vòng lặp
ngắn (`i`, `j`, `k`), toạ độ (`x`, `y`), số lượng (`n`) và cặp so sánh `|a, b|` trong `sort_by`.

**Doc.** Mọi item (module, kiểu, trường, hàm, hằng số) có doc `///`. Doc nói *làm gì* và *vì sao*,
không lặp lại code. Với hàm không tầm thường, nêu ý nghĩa tham số, giá trị trả về và các trường hợp
đặc biệt (khi nào trả `None`, khi nào panic).

**Hiển thị.** Chỉ những gì thuộc API công khai mới là `pub`; phần nội bộ là `pub(crate)` hoặc
private.

Các quy ước này được CI kiểm tra (`clippy.toml`, lint trong `lib.rs`).

## Đảm bảo không đổi hành vi

Mọi thay đổi cấu trúc phải giữ nguyên đầu ra. `tests/real_papers.rs` so mã băm đầu ra của 8 bài thật
ở cả bốn chế độ và JSON với `tests/data/snapshots`; lệch một ký tự là test đỏ. Khi cố ý đổi đầu ra,
chạy `P2H_UPDATE_SNAPSHOTS=1 cargo test --test real_papers` và commit snapshot cùng thay đổi.
