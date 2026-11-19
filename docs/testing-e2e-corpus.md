# 复杂语料端到端验证

目的：用**模拟生成的复杂文档**（含隐藏内容、异常结构）验证“检测 → 脱敏 → 复检 → 还原”全链路，
而不是只测手写的小样本。

## 运行

```bash
cargo test -p anonymizer-pipeline --test corpus_e2e -- --nocapture
# 保留产物目录便于排查：
ANONKIT_E2E_KEEP=1 cargo test -p anonymizer-pipeline --test corpus_e2e -- --nocapture
```

依赖 `sidecar/.venv`（缺失时测试跳过）。单独生成语料：

```bash
sidecar/.venv/bin/python scripts/gen_complex_corpus.py /tmp/corpus
```

## 组成

- `scripts/gen_complex_corpus.py`：生成语料与标准答案 `truth.json`（`[{file, secret, kind, detectable}]`）。
- `crates/anonymizer-pipeline/tests/corpus_e2e.rs`：启动 sidecar，跑 `run_batch`，把产物与还原文本交给校验脚本。
- `scripts/verify_corpus.py`：判定。失败类别：残留 / 还原 / 内容丢失 / 无法打开 / 容错。

## 语料覆盖

| 格式 | 覆盖的真实场景 |
|---|---|
| DOCX | 拆 run 的号码、嵌套表格、页眉页脚（含多节/首页页眉）、脚注、批注（含作者）、修订插入/删除、超链接（含 mailto 目标）、内容控件、域代码、隐藏文字、文本框 + mc:Fallback、图片替代文字、合并单元格、核心/扩展/自定义属性、3000 段大文档 |
| XLSX | openpyxl 与 XlsxWriter 两种生成器、共享字符串、数值型手机号、公式字面量、批注、页眉页脚、数据验证列表、超链接、富文本、隐藏行/表、定义名称、图表、合并单元格、6000 行大表 |
| PDF | 真实 CJK 子集化 TrueType + Identity-H 十六进制字符串、英文 Helvetica、TJ 数组字距拆分、旋转页、25 页长文档、链接注释/书签/附件/元数据 |
| TXT/MD | UTF-8 CRLF、GBK、UTF-8 BOM、仅 CR 换行 |
| 号码格式 | 全角数字、+86/0086/（86）、空格/连字符分组、座机括号/空格/分机、身份证分组空格/小写 x、对公账号、护照/港澳通行证、港澳台证件、繁体前缀、邮箱大小写、IP/URL/车牌/信用代码、案号 |
| 一致性 | 简称联动（“以下简称”）、生僻姓名仅首次带前缀、占位符与汉字紧邻 |
| 容错 | 截断/损坏的 docx、非 zip 的 xlsx、空文件、无文本层 PDF |

`detectable=False` 的条目是**已知检测盲区**（如元数据里的作者），只统计不判失败；
作者类（`kind=author`）必须被清理，残留即失败。

## 校验项

1. **残留**：可检出项不得出现在输出任何可提取部位（复检器：OOXML 全部 part、PDF 对象流/PDFium 文字/元数据、文本）。
2. **还原**：解析输出全文 → 映射表还原 → 出现次数不少于原文，且不残留占位符。
3. **内容保留**：标题、工作表、图表、非敏感文字仍可检索。
4. **容错**：损坏文件不崩溃、不产出半成品、给出明确错误。

## 这套语料已经发现并修复的缺陷（部分）

- 批注/页眉页脚/超链接目标/图片替代文字/注释链接/数据验证从未进入检测。
- PDF 安全模式只涂黑不写占位符 → 无法还原；旋转页没有文字层。
- 简称联动不生效；生僻姓名只在首次出现处被识别（后文泄露）。
- 占位符两侧紧贴汉字时还原失败（“甲方[公司1]委托乙方”）。
- “任何敏感信息”被识别成人名“何敏”。
- 全角数字手机号、分组空格身份证、括号座机、对公账号、护照/通行证漏检。
