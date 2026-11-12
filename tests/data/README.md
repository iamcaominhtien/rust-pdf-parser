# Dữ liệu test

`papers.txt` liệt kê các bài arXiv dùng cho `tests/real_papers.rs`, ghim phiên bản và mã sha256.

Chỉ bài có giấy phép cho phép phân phối lại mới được đưa vào repo:

- `1602.03837v1.pdf`: B. P. Abbott et al. (LIGO Scientific Collaboration and Virgo Collaboration),
  "Observation of Gravitational Waves from a Binary Black Hole Merger", arXiv:1602.03837,
  giấy phép [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). File được giữ nguyên, không sửa đổi.

`snapshots/` chứa mã sha256 của đầu ra từng bài (không chứa nội dung bài). Riêng
`snapshots/1602.03837v1.json` là đầu ra JSON đầy đủ của bài LIGO, tức nội dung bài đó đã qua
chuyển đổi; vẫn theo giấy phép CC BY 4.0 với cùng ghi nguồn như trên.

Các bài còn lại dùng giấy phép mặc định của arXiv (chỉ arXiv được phân phối), nên không có trong
repo. Tải một lần về thư mục này bằng:

```bash
scripts/fetch-test-pdfs.sh
```

Bài nào chưa có thì test bỏ qua và in ra danh sách còn thiếu. Khi đặt biến môi trường
`P2H_REQUIRE_ALL_PAPERS=1` (CI làm vậy), thiếu bài là lỗi.
