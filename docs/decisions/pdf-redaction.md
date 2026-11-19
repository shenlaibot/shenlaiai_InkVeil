# ADR: PDF 深度脱敏方案（内容流级删除）

- 状态: **可行（已验证）**
- 日期: 2026-08-10
- 决策: 采用"pypdfium2 定位 + pikepdf 内容流编辑"实现 PDF 深度脱敏——从页面内容流中真正删除命中的敏感文本，删除后该文本**不可再被选中/复制/提取**，而非仅用白框覆盖。
- 参考实现: `spike/pdf-redaction/test_redact.py`（可运行完整脚本）

---

## 1. 结论

**方案可行。** 通过内容流字节级编辑，敏感文本（中文姓名、手机号、身份证号）被彻底删除：
- 脱敏后重新提取文本，原敏感串 100% 不可见、不可选中、不可复制；
- 对脱敏后 PDF 的内容流做**字节级扫描**，敏感文本的 UTF-16BE 字节序列零残留；
- 掩码文本（如 `138****8000`）在原文位置可见，且 bbox 与原文本 bbox 几乎重合（偏差 <1pt）；
- 同页其它文字、段落位置、版面完全保留。

## 2. 实现原理简述

三层叠加（缺一不可）：

1. **定位层（pypdfium2，BSD-3-Clause）**：对每页调用 PDFium `FPDFText_*` 底层 API——
   - `get_text_bounded()` 取全页文本（其字符索引与 PDFium 全局字符索引一一对应，含 `\r\n` 占位字符）；
   - `str.find` 找到敏感子串的字符索引范围；
   - `get_charbox(index)` 取每个字符的 bbox，合并为敏感文本的精确 bbox（PDF 坐标）。
2. **删除层（pikepdf，MPL-2.0，底层 qpdf）**：读取页面内容流字节，解析为 token 流（字符串/数组/名字/数字/操作符），用状态机模拟 `BT/Tf/Tm/Td/TL/Tj/TJ` 定位每个文本块。将 `Tj`/`TJ` 的字符串 **UTF-16BE 解码 → 敏感子串替换为空 → UTF-16BE 重新编码写回**，从内容流中彻底移除敏感字符。
   - 关键点：reportlab 的 CID 字体（`UnicodeCIDFont`）写 PDF 时字符串编码即 **UTF-16BE（去 BOM）**（见 `reportlab.pdfbase.cidfonts.CIDFont.formatForPdf`），故内容流字符串可无损双向转换。
   - 若文本块为 `TJ` 数组（真实 PDF 常见）且被命中，则整块改写为单个 `Tj` 字符串（丢弃原字距调整，位置仍由 `Tm` 决定）。
3. **覆盖/掩码层（内容流指令追加）**：在内容流末尾追加——
   - 白矩形：`q 1 1 1 rg x y w h re f Q`，覆盖命中 bbox（+0.5pt padding），双保险遮住视觉残留；
   - 掩码文本：`BT /F <size> Tf 1 0 0 1 x y Tm (mask) Tj ET`，在原敏感文本起点绘制掩码（如 `138****8000`、`张*`、`110101**********34`）。

## 3. 关键 API

| 库 | API | 作用 |
|---|---|---|
| pypdfium2 | `PdfDocument` / `page.get_textpage()` | 打开 PDF、建文本页 |
| pypdfium2 | `textpage.get_text_bounded()` | 全页文本（字符索引与 PDFium 索引一致） |
| pypdfium2 | `textpage.get_charbox(i)` | 第 i 个字符的 bbox（l,b,r,t，PDF 坐标） |
| pypdfium2 | `textpage.count_chars()` / `get_text_range(i,n)` | 字符级遍历 |
| pikepdf | `page.Contents.read_bytes()` | 读页面内容流（自动解压） |
| pikepdf | `page.Contents = pikepdf.Stream(pdf, bytes)` | 写回新内容流 |
| pikepdf | `pdf.save(path)` | 输出 PDF（qpdf 重写，自动修复 xref/压缩） |
| reportlab | `UnicodeCIDFont('STSong-Light')` | 生成含中文的测试 PDF |
| reportlab | `pdfmetrics.getFont(...).formatForPdf`（源码参考） | 确认 CID 字体字符串 = UTF-16BE |

## 4. 实测结果

测试文件：2 页 A4，含 3 类敏感文本（姓名/手机号/身份证号），其中一页敏感文本与普通文本混排。

脱敏后（`output/sample_after.pdf`）用 pypdfium2 重新提取文本：

| 验证项 | 结果 |
|---|---|
| 原敏感文本 `张三` / `13800138000` / `110101198403201234` 是否可再提取 | **不可**（全部 PASS） |
| 脱敏 PDF 内容流字节级扫描是否含敏感 UTF-16BE 序列 | **零残留**（全部 PASS） |
| 掩码 `张*` / `138****8000` / `110101**********34` 是否可见 | **可见**（全部 PASS） |
| 掩码 bbox 与原文本 bbox 位置 | 重合（偏差 <1pt） |
| 版面（其它行文字、位置） | 完全保留（全部 PASS） |
| `TJ` 数组形式文本块（模拟真实 PDF） | 同样通过全部断言 |

验证文件：`spike/pdf-redaction/output/`（`before.txt` / `after.txt` / `before_page*.png` / `after_page*.png` 可直接目检）。

## 5. 性能粗测（本机 Apple Silicon，Python 3.11）

- 处理 2 页（含 5 处命中）：**总耗时 ~18 ms，平均 ~9 ms/页**（含 pypdfium2 定位 + pikepdf 编辑 + 保存）。
- 对 1 页而言，耗时量级为毫秒级，可作为在线脱敏流水线的一环。

## 6. 局限与风险

1. **跨文本块的敏感串**：若敏感串横跨两个 `Tj`/`TJ` 块（如中间被换行/字距断开），当前"块内子串匹配"会漏删。需升级为跨块拼接匹配 + 逐块部分删除（复杂度高，spike 未覆盖；测试数据均为单块内命中）。白矩形层仍会覆盖视觉，但文本层可能残留——**产品化前必须处理**。
2. **字体编码假设**：当前实现假设内容流文本为 UTF-16BE（reportlab CID 字体、部分子集字体）。真实 PDF 常见 `WinAnsi`（单字节，如 Helvetica/中文 GBK 编码）或带 `ToUnicode` CMap 的任意编码。产品化需要：
   - 读取页面资源的字体 `Encoding`/`ToUnicode`，做通用编码解码；或
   - 对未知编码退化为"白矩形覆盖 + 文本层尽力删除"，并在文档中声明。
3. **`TJ` 数组改写**：命中块改写为单 `Tj` 会丢失原字距调整（视觉宽度略变，测试中 <1pt 级，可接受；白矩形 + 掩码重绘已消除可见差异）。
4. **表单字段 / XFA / 注释中的文本**：不在页面内容流中，本方案不覆盖；表单值需单独用 `pikepdf` 清字段（`/V` 置空）或另做处理。
5. **元数据 / 附件**：`pdf.save()` 保留元数据；若敏感文本出现在元数据（`/Title`、`/Subject`、XMP）中需另行清理。本 spike 未覆盖。
6. **权限/加密 PDF**：pikepdf 打开加密 PDF 需提供密码；无密码的受限 PDF 一般可编辑内容流（qpdf 支持），需实测。
7. **不透明背景问题**：白矩形覆盖只对白底文档视觉有效。若原文档有底色/图片背景，白矩形会露出色块。产品化应改为"读取被覆盖区域背景色/纹理再绘制掩码背景"或接受色块。

## 7. 许可证合规（可商用开源）

- pypdfium2：BSD-3-Clause ✓
- pikepdf：MPL-2.0 ✓（qpdf：Apache-2.0 ✓）
- reportlab：BSD ✓
- 明确排除：PyMuPDF（AGPL）、pdf2image/poppler 工具链（GPL，仅可作为可选本地工具而非依赖）。

## 8. 建议

- 架构方向：`pypdfium2 定位 → pikepdf 内容流编辑` 组合可行，无 AGPL 依赖，可安全商用。
- 产品化前必做：跨块匹配、通用字体编码解码（ToUnicode）、表单字段与元数据清理、背景感知的掩码覆盖。
