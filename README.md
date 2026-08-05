# caj2pdf-rs

将知网（CNKI）的 CAJ / HN / C8 / KDH 格式文件转换为 PDF 的小工具。Rust 实现。

## 功能

| 格式 | 转换 | 解码 | 提取大纲 |
|---|---|---|---|
| CAJ | ✓ | — (内嵌 PDF) | ✓ |
| HN / C8 | ✓ | JBIG1 / JBIG2 / JPEG | ✓ |
| KDH | ✓ | XOR 解密 | — (内嵌 PDF) |
| PDF | pass-through | — | — |

## 安装

```bash
cargo build --release
```

无外部依赖。JBIG1 / JBIG2 / PDF / zlib / GBK 解码全部纯 Rust 实现，二进制可直接 `cargo install` 或交叉编译到 macOS / Windows / Linux 任意目标。

二进制：`target/release/caj2pdf`（约 2.7 MB）。

## 用法

```bash
# 查看文件信息
caj2pdf show 论文.caj

# 转换为 PDF
caj2pdf convert 论文.caj -o 论文.pdf

# 提取文字
caj2pdf text-extract 论文.hn

# 给已有 PDF 注入大纲
caj2pdf outlines 论文.caj -o 打印版.pdf
```

## 桌面 GUI

```bash
cargo build --release -p caj2pdf-gui
# 拖入 .caj / .hn / .c8 / .kdh / .pdf 文件，点 Convert all
```

二进制：`target/release/caj2pdf-gui`（约 16 MB，纯 Rust / egui）。
Linux 需 X11 或 Wayland；macOS / Windows 无额外依赖。

## 跨平台编译

| 目标 | 命令 | 输出 | 系统依赖 |
|---|---|---|---|
| Linux x86_64 | `cargo build --release -p caj2pdf-gui` | `target/release/caj2pdf-gui` (16 MB) | 系统 glibc |
| Linux aarch64 (Kylin / ARM) | `cargo build --release -p caj2pdf-gui --target aarch64-unknown-linux-musl` | `target/aarch64-unknown-linux-musl/release/caj2pdf-gui` (15 MB) | **零** — 全静态 |
| Windows x86_64 | 见下方 | `caj2pdf-gui-x86_64-windows.exe` (9.5 MB) | **零** — MinGW CRT 静态链接 |

### aarch64 跨编（Kylin / 银河麒麟）

```bash
rustup target add aarch64-unknown-linux-musl
# 一次性：把 gcc-isms → lld 的包装脚本装到系统
sudo tee /usr/local/bin/lld-aarch64-gnu.sh >/dev/null <<'EOF'
#!/bin/sh
args=""
for a in "$@"; do
    case "$a" in
        -Wl,*) args="$args ${a#-Wl,}" ;;
        -nostartfiles|-nodefaultlibs|-Bstatic|-Bdynamic) ;;
        -ldl) args="$args -lc" ;;
        *) args="$args $a" ;;
    esac
done
exec /usr/bin/lld -flavor gnu $args
EOF
sudo chmod +x /usr/local/bin/lld-aarch64-gnu.sh

CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=/usr/local/bin/lld-aarch64-gnu.sh \
  cargo build --release -p caj2pdf-gui --target aarch64-unknown-linux-musl
```

### Windows 跨编

```bash
sudo apt install mingw-w64      # 提供 x86_64-w64-mingw32-dlltool
rustup target add x86_64-pc-windows-gnu
cargo build --release -p caj2pdf-gui --target x86_64-pc-windows-gnu
# → PE32+ GUI，双击直接开窗，无控制台窗口，无运行时 DLL
```

### 预编译二进制

最新构建：`dist/` 目录

| 文件 | 目标 | 大小 |
|---|---|---|
| `caj2pdf-gui-aarch64-kylin` | aarch64-unknown-linux-musl | 15 MB |
| `caj2pdf-gui-x86_64-windows.exe` | x86_64-pc-windows-gnu | 9.5 MB |

详细的跨平台说明见 [`docs/cross-compile.md`](docs/cross-compile.md)。

## 致谢

本项目是 [caj2pdf](https://github.com/JeziL/caj2pdf) 的 Rust 重写版本。
感谢原作者 **Hin-Tak Leung** 对 CAJ 容器格式和私有 JBIG1 编码器的逆向分析工作，
没有 `JBigDecode.cc` 等参考实现就没有这个项目。
也感谢 [caj2pdf](https://github.com/JeziL/caj2pdf) 维护者和所有贡献者。

## 许可

GPL-2.0-or-later。JBIG1 解码器部分源自 FreeType 项目的 `JBigDecode.cc：`。
