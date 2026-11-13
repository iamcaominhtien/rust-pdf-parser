# Đóng góp

Cấu trúc code và quy ước viết code: xem [ARCHITECTURE.md](ARCHITECTURE.md). Cách chạy test: xem
mục "Kiểm thử" trong [README.md](README.md).

## Commit message: Conventional Commits

Dòng đầu của commit theo dạng `<loại>: <mô tả>` (tiếng Anh, thể mệnh lệnh). Loại commit quyết
định phiên bản tiếp theo và nhóm trong `CHANGELOG.md`:

| Loại | Dùng khi | Tăng phiên bản (khi còn 0.x) | Nhóm trong changelog |
| --- | --- | --- | --- |
| `feat` | Thêm tính năng | minor: 0.1.0 → 0.2.0 | Added |
| `fix` | Sửa lỗi | patch: 0.1.0 → 0.1.1 | Fixed |
| `perf` | Tăng tốc, giảm bộ nhớ | patch | Performance |
| `refactor` | Sắp xếp lại code, không đổi hành vi | patch | Changed |
| `docs` | Tài liệu | patch | Documentation |
| `test` | Test | patch | Tests |
| `ci`, `build` | CI, build, dependency | patch | Build and CI |
| `chore` | Việc vặt khác | patch | Other |

- Thay đổi phá vỡ tương thích (đổi tham số CLI, đổi định dạng JSON đầu ra, đổi API công khai):
  thêm `!` sau loại (`feat!: ...`) hoặc dòng `BREAKING CHANGE: ...` trong phần thân.
- Có thể ghi phạm vi trong ngoặc: `fix(tables): keep header rows at the top of a page`.
- Ví dụ: `feat: add a batch command`, `perf: keep embedded JPEGs instead of re-encoding them`.

Commit cũ trước khi có quy ước này dùng dạng "Add ...", "Fix ...". Changelog vẫn xếp chúng vào
đúng nhóm.

## Phát hành

Phát hành tự động bằng [release-plz](https://release-plz.dev) (`release-plz.toml`,
`.github/workflows/release.yml`):

1. Mỗi lần `main` có commit mới, bot mở hoặc cập nhật một **release PR** tên
   `chore: release vX.Y.Z`. PR này tăng phiên bản trong `Cargo.toml` theo các commit kể từ lần
   phát hành trước, và thêm mục mới vào `CHANGELOG.md`. Nội dung changelog trong PR sửa tay được
   trước khi merge.
2. Khi muốn phát hành, **merge release PR**. Workflow tạo tag `vX.Y.Z` và GitHub Release với nội
   dung changelog, rồi build file chạy cho Linux (x86_64, aarch64), macOS (Apple Silicon, Intel) và
   Windows (x86_64), kèm mã sha256, đính vào release.

Không đăng lên crates.io: phiên bản xác định từ tag git (`git_only = true`).

### Thiết lập một lần trên GitHub

- **Settings → Actions → General → Workflow permissions:** chọn "Read and write permissions" và
  bật "Allow GitHub Actions to create and approve pull requests". Không bật thì bot không mở được
  release PR.
- **Tuỳ chọn:** tạo secret `RELEASE_PLZ_TOKEN` (fine-grained personal access token của repo, quyền
  Contents và Pull requests: read and write). Nếu không có, workflow dùng token mặc định của GitHub
  Actions; khi đó CI **không tự chạy trên release PR** (giới hạn của GitHub với PR do token mặc
  định mở). Release PR chỉ sửa `Cargo.toml` và `CHANGELOG.md`, và CI vẫn chạy lại trên `main` sau
  khi merge.
