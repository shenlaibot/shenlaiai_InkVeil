# 第三方组件声明（THIRD-PARTY-NOTICES）

墨隐 InkVeil 自身采用自定义许可协议发布（个人学习/非商用免费，商用须授权；见 `LICENSE`、`NOTICE`）。分发的安装包中包含下列第三方组件，
各自适用其许可证。完整的依赖/许可证清单见 `DEPENDENCIES.md`（`scripts/licenses.py` 自动生成），
机器可读的 SBOM（CycloneDX）由同一脚本输出。

> 状态说明：本文件只记录**已在依赖元数据/随包许可证文本中核实**的事实；无法在本仓库内核实的地方一律标注“**待核实**”，
> 公开发布前必须逐项关闭。具体版本以发布时的 `sidecar/requirements*.lock`、`Cargo.lock`、`package-lock.json` 为准。

## 1. PDFium 及其捆绑库（经 `pypdfium2` 分发）

- 用途：PDF 文本/坐标提取与页面渲染（sidecar）。
- 分发形态：`pypdfium2` 二进制 wheel 内含预编译的 PDFium 动态库。
- 许可证：pypdfium2 元数据声明 “BSD-3-Clause, Apache-2.0, dependency licenses”；PDFium 本体为 Apache-2.0 / BSD 系（**精确归属待核实**，以随包文本为准）。
- 随 wheel 的许可证文本位于 `pypdfium2-<ver>.dist-info/licenses/`（`LICENSES/`、`BUILD_LICENSES/`）。已核实的捆绑库文本包括：
  abseil、agg23、dragonbox、fast_float、freetype、harfbuzz、highway、ICU、lcms、libjpeg-turbo、
  OpenJPEG、libpng、libtiff、simdutf、zlib、pdfium-binaries、pdfium 本体。
- **义务**：安装包须附带上述各文本（见下文“安装包内应附带的文件”）。
- 源码/来源：<https://pdfium.googlesource.com/pdfium/>、<https://github.com/bblanchon/pdfium-binaries>、<https://github.com/pypdfium2-team/pypdfium2>。

## 2. ONNX Runtime（`onnxruntime`）

- 用途：中文 NER 模型的 CPU 推理。
- 许可证：MIT（wheel 元数据 `License: MIT License`）。
- wheel 内含 `ThirdPartyNotices.txt`（已核实存在），列出 ONNX Runtime 自带的第三方许可证；随包分发时须一并附带。
- 来源：<https://github.com/microsoft/onnxruntime>。
- NER 模型文件**不随包分发**（运行时由模型市场下载到应用数据目录），模型自身的许可证：**待核实**（取决于所选模型，上架前逐个确认可商用）。

## 3. MPL-2.0 组件与源码获取方式

MPL-2.0 是文件级弱 copyleft：若分发了**经修改**的 MPL 文件，须以 MPL 公开这些修改过的文件的源码；
未修改地分发时，须告知并提供获取源码的途径。**墨隐 InkVeil 对下列组件均未做修改**，源码获取方式如下。

### 3.1 pikepdf（MPL-2.0）

- 用途：PDF 内容流编辑、元数据/注释清理。
- 源码：<https://github.com/pikepdf/pikepdf>（与所用版本同名 tag；PyPI 另提供 sdist）。
- 二进制 wheel 内同时携带下列库（随包 `third-party-licenses/` 目录已核实，列出文本）：
  qpdf（Apache-2.0；运行时 `pikepdf.__libqpdf_version__` 可查具体版本，本机 macOS 构建为 12.4.2）、
  libjpeg-turbo（IJG + BSD-3-Clause + Zlib）、OpenSSL、zlib、Microsoft Visual C++ 运行库（Windows wheel）、
  sRGB ICC 配置文件（Zlib）、veraPDF 规则数据（CC-BY-4.0）、pdfminer.six 编码表（MIT）。
- 该目录另含 `GPL-3.0.txt` 与 `GCC-exception-3.1.txt`：对应 Linux wheel 中 auditwheel 引入的 GCC 运行时库（libgcc/libstdc++，
  适用 GCC Runtime Library Exception）。**这是否影响 Linux AppImage/deb 分发：待核实**（本机为 macOS，无法验证 Linux/Windows wheel 内容）。
- qpdf 源码：<https://github.com/qpdf/qpdf>。

### 3.2 tqdm（MPL-2.0 AND MIT）

- 经 huggingface_hub 间接引入，**仅用于模型下载（安装/开发期）**，运行时处理路径不使用；是否进入最终包：待核实（PyInstaller 以静态分析为准）。
- 源码：<https://github.com/tqdm/tqdm>。

### 3.3 Rust 依赖中的 MPL-2.0 crate

由 Tauri/WebView 栈间接引入，均为 crates.io 上未修改版本，源码可从 crates.io 或其仓库（servo 组织 / 各 crate 主页）获取：
`cssparser`、`cssparser-macros`、`dtoa-short`、`selectors`、`option-ext`。

### 3.4 自动生成的 MPL 清单

下方区块由 `python3 scripts/licenses.py --write-notices` 生成；CI 中 `scripts/licenses.py` 会校验所有 MPL 依赖的名称都出现在本文件中，否则失败。

<!-- BEGIN AUTO-MPL-LIST (scripts/licenses.py --write-notices 生成，勿手改) -->

| 生态 | 包 | 版本 | 许可证 |
|---|---|---|---|
| cargo | cssparser | 0.36.0 | MPL-2.0 |
| cargo | cssparser-macros | 0.6.1 | MPL-2.0 |
| cargo | dtoa-short | 0.3.5 | MPL-2.0 |
| cargo | option-ext | 0.2.0 | MPL-2.0 |
| cargo | selectors | 0.36.1 | MPL-2.0 |
| python | pikepdf | 10.17.0 | MPL-2.0 |
| python | tqdm | 4.70.1 | MPL-2.0 AND MIT |

<!-- END AUTO-MPL-LIST -->

## 4. PyInstaller（构建工具，GPL-2.0-or-later WITH Bootloader-exception）

- 仅用于构建 sidecar；其引导加载器会嵌入产物，但 PyInstaller 的特别例外明确允许用其构建和分发非自由/商业程序（已在 6.x 的 `COPYING.txt` 与元数据核实）。
- `pyinstaller-hooks-contrib`：标准 hooks 为 GPL-2.0-or-later，runtime hooks 为 Apache-2.0；hooks 仅在构建机运行。已在 `scripts/license-overrides.json` 作为“构建期工具豁免”登记，**需项目负责人/法务确认**。

## 5. 字体（OFL 等）

- 当前仓库内**未发现随安装包分发的字体文件**（前端使用系统字体；PDF 掩码使用无字形占位字体 `AnonKitGlyphless`，无字库）。
- 若后续引入字体（如 Noto Sans CJK / 思源黑体，SIL OFL-1.1），须：附带字体自带的 OFL 文本及版权行，不单独出售字体，不使用保留字体名（Reserved Font Name）命名衍生字体。**现状：待核实（若有未发现的内嵌字体）**。
- `reportlab` 仅在测试语料生成中使用（dev 依赖），不随包分发。

## 6. 其他主要运行时依赖（均已由 `licenses.py` 解析为宽松许可证）

| 组件 | 许可证 | 备注 |
|---|---|---|
| Tauri 及其插件 | Apache-2.0 OR MIT | Rust 栈，见 `DEPENDENCIES.md` |
| React / react-dom / lucide-react | MIT / MIT / ISC | 前端，打包进 Web 资源 |
| FastAPI / Starlette / uvicorn / pydantic | MIT / BSD-3-Clause / BSD-3-Clause / MIT | sidecar |
| python-docx / openpyxl / lxml | MIT / MIT / BSD-3-Clause | 文档解析与改写 |
| numpy | BSD-3-Clause AND 0BSD AND MIT AND Zlib AND CC0-1.0 | onnxruntime 依赖 |
| tokenizers | Apache-2.0 | 以其 wheel 元数据为准（待核实具体字段） |

## 安装包内应附带的文件（发布清单，待办）

1. 本文件与 `LICENSE`、`NOTICE`；
2. pypdfium2 wheel 的 `licenses/`（含 `BUILD_LICENSES/`）、pikepdf wheel 的 `third-party-licenses/`、onnxruntime 的 `ThirdPartyNotices.txt`；
3. 前端依赖许可证汇总（npm，随 Vite 产物，待确认是否需 `licenses.txt` 插件生成）；
4. Rust 依赖许可证汇总（可用 `cargo about`/`cargo-bundle-licenses` 生成，待接入，见 `docs/release.md`）。

以上 2–4 目前**尚未自动打入安装包**，属发布前待办。
