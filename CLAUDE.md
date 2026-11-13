# CLAUDE.md

`paper2html`: CLI và thư viện Rust chuyển PDF bài báo arXiv sang HTML, với 4 chế độ:

- `hard`: bản sao từng trang, có lớp chữ chọn được.
- `hard-95`: như `hard`, nhưng toàn bộ chữ là HTML thật đặt đúng vị trí như trong PDF.
- `medium`: giữ bố cục trang và cột, chữ được dàn lại.
- `friendly`: bài viết một cột, có chế độ cuộn, chế độ sách và giao diện điện thoại.

Đọc trước khi sửa code:

- [ARCHITECTURE.md](ARCHITECTURE.md): pipeline, module, quy ước code.
- [CONTRIBUTING.md](CONTRIBUTING.md): commit message.
- [README.md](README.md), mục "Kiểm thử".

## Lệnh

```bash
cargo build --release
./target/release/paper2html convert <file.pdf | arXiv id> -m friendly -o out/x.friendly.html   # hoặc --all -o out/dir
./target/release/paper2html inspect <file.pdf>        # xem cấu trúc nhận được, để gỡ lỗi
scripts/fetch-test-pdfs.sh                            # tải 7 bài test (một lần)
cargo test --release                                  # test đơn vị + 8 bài thật + snapshot
P2H_UPDATE_SNAPSHOTS=1 cargo test --release --test real_papers   # khi CỐ Ý đổi đầu ra
P2H_PRINT_METRICS=1 cargo test --release --test real_papers -- --nocapture  # in số đo từng bài
cargo fmt --check && cargo clippy --all-targets -- -D warnings      # cổng lint của CI
node tests/browser/check-friendly.js out/*.friendly.html             # kiểm tra trình duyệt
node tests/browser/check-hard95.js out/*.hard-95.html
node tests/browser/check-stable.js out/*.medium.html out/*.friendly.html
```

Chromium và Playwright có sẵn trong môi trường cloud. Không chạy `playwright install`.

## Hook tự động (`.claude/settings.json`)

- **Sau mỗi lần Edit/Write** (`.claude/hooks/after-edit.sh`):
  - `.rs`: chạy `rustfmt`.
  - `.js`: chạy `node --check`.
  - `.json`: kiểm tra JSON hợp lệ.
  - `.sh`: chạy `bash -n`.

  Nếu có lỗi (cú pháp, rustfmt thất bại), lỗi được báo ngay để sửa.
- **Khi kết thúc lượt** (`.claude/hooks/on-stop.sh`): chỉ chạy nếu lượt đó có sửa file `.rs`.
  1. Chạy `cargo fmt`, rồi `cargo clippy --fix`, rồi `cargo clippy --all-targets -- -D warnings`.
  2. Cảnh báo clippy không tự sửa được sẽ được gửi lại cho Claude để sửa nốt (một lần mỗi lượt).

Hook không chạy test vì test chậm. Hãy tự chạy `cargo test --release` trước khi commit.

## Quy trình khi sửa phần phân tích (`src/analysis`)

- Ngưỡng mới phải là trường có doc trong struct config của giai đoạn đó (`src/config.rs`). Tên
  trường có hậu tố đơn vị: `_em`, `_pt` hoặc `_ratio`. Không viết số "thần kỳ" trong code.
- Một luật sửa được bài này rất dễ làm hỏng bài khác. Sau mỗi thay đổi:
  1. Chạy lại test và xem snapshot nào đổi.
  2. So chữ đầu ra trước và sau trên càng nhiều bài càng tốt (8 bài test, cộng các bài đa dạng
     khác nếu có).
- Snapshot (`tests/data/snapshots/*.txt`) là mã băm của HTML từng chế độ.
  - Đổi CSS, JS hay asset của renderer cũng làm đổi snapshot, vì chúng được nhúng bằng
    `include_str!`.
  - Cập nhật snapshot chỉ khi thay đổi đầu ra là cố ý, và commit cùng với code.
- Các mốc trong `tests/real_papers.rs` (số đoạn bị đứt, số từ còn gạch nối, số ký tự còn sót...)
  có ý nghĩa: tốt hơn thì siết mốc lại, tệ hơn là regression.
- Test cho một lỗi cụ thể đặt trong `tests/real_papers.rs` và đặt tên theo hành vi, ví dụ
  `ligo_radical_is_a_square_root`.

## JS và CSS của renderer

- `src/render/friendly/{friendly.js,book.js,friendly.css}` và `src/render/hard95/` được viết gọn
  có chủ ý. Không chạy prettier hay format lại toàn file: diff sẽ rất lớn và mọi snapshot đều đổi.
- Chế độ sách (`book.js`) đo bố cục bằng CSS columns. Mọi thay đổi về ngắt trang phải chạy lại
  `check-friendly.js`; script này kiểm tra trang gần trống, chú thích tách khỏi hình và heading mồ
  côi.

## Ràng buộc

- **PDF:** chỉ bài LIGO (`tests/data/1602.03837v1.pdf`, CC BY 4.0) được commit. Các bài arXiv khác
  không được phân phối lại: tải bằng `scripts/fetch-test-pdfs.sh` và giữ ngoài git
  (`.gitignore` đã có `*.pdf`).
- **Clippy:** CI dùng clippy mới hơn bản local, nên có thể báo thêm lint (ví dụ
  `needless_bool`). Xem log CI nếu job "fmt + clippy" đỏ.
- **Commit:** theo Conventional Commits (`fix(tables): ...`, `feat: ...`), tiếng Anh, thể mệnh
  lệnh.
