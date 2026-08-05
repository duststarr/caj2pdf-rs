# caj2pdf-rs

将知网（CNKI）的 CAJ / HN / C8 / KDH 格式文件转换为 PDF 的小工具。纯 Rust 实现。

## 功能

| 格式 | 转换 | 解码 | 提取大纲 |
|---|---|---|---|
| CAJ | ✓ | — (内嵌 PDF) | ✓ |
| HN / C8 | ✓ | JBIG1 / JBIG2 / JPEG | ✓ |
| KDH | ✓ | XOR 解密 | — (内嵌 PDF) |
| PDF | pass-through | — | — |

无外部 C 依赖（JBIG1 纯 Rust 自定义 5-context 算术编码器；JBIG2 走 `pdfluent-jbig2`；PDF 走 `lopdf`；GBK 走 `encoding_rs`）。

## 安装

```bash
cargo build --release
```

- CLI 二进制：`target/release/caj2pdf`（约 2.7 MB）
- GUI 二进制：`target/release/caj2pdf-gui`（约 16 MB，纯 Rust / egui）

## 用法

```bash
# CLI
caj2pdf show 论文.caj
caj2pdf convert 论文.caj -o 论文.pdf
caj2pdf text-extract 论文.hn
caj2pdf outlines 论文.caj -o 打印版.pdf

# GUI: 拖入 .caj / .hn / .c8 / .kdh / .pdf 文件，点 Convert all
./target/release/caj2pdf-gui
```

## 文档

- [`docs/architecture.md`](docs/architecture.md) — 模块依赖图 + 数据流图
- [`docs/format-analysis.md`](docs/format-analysis.md) — CAJ / HN / C8 / KDH 字节级格式
- [`docs/cross-compile.md`](docs/cross-compile.md) — 跨平台构建、依赖、预编译二进制
- [`docs/development.md`](docs/development.md) — 开发流程、CI、git flow
- [`docs/jbig1-reverse-notes.md`](docs/jbig1-reverse-notes.md) — JBIG1 逆向细节
- [`docs/jbig2-notes.md`](docs/jbig2-notes.md) — JBIG2 走 `pdfluent-jbig2` 的说明
- [`docs/pdf-assembly.md`](docs/pdf-assembly.md) — lopdf PDF 装配说明

## 致谢

本项目是 [caj2pdf](https://github.com/JeziL/caj2pdf) 的 Rust 重写版本。
感谢原作者 **Hin-Tak Leung** 对 CAJ 容器格式和私有 JBIG1 编码器的逆向分析工作，
没有 `JBigDecode.cc` 等参考实现就没有这个项目。
也感谢 [caj2pdf](https://github.com/JeziL/caj2pdf) 维护者和所有贡献者。

## 许可

GPL-2.0-or-later。JBIG1 解码器部分源自 FreeType 项目的 `JBigDecode.cc`。
