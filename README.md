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

## 致谢

本项目是 [caj2pdf](https://github.com/JeziL/caj2pdf) 的 Rust 重写版本。
感谢原作者 **Hin-Tak Leung** 对 CAJ 容器格式和私有 JBIG1 编码器的逆向分析工作，
没有 `JBigDecode.cc` 等参考实现就没有这个项目。
也感谢 [caj2pdf](https://github.com/JeziL/caj2pdf) 维护者和所有贡献者。

## 许可

GPL-2.0-or-later。JBIG1 解码器部分源自 FreeType 项目的 `JBigDecode.cc：`。
