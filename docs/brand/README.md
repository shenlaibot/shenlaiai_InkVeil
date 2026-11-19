# 墨隐 InkVeil · 品牌规范

> **墨隐 InkVeil** | 开源 AI 法律本地脱敏工具 · 安全可靠 · 数据不联网

## 名称

| 场景 | 写法 |
| --- | --- |
| 完整品牌名 | 墨隐 InkVeil |
| 仅中文 | 墨隐 |
| 仅英文 / 产品名（安装包、`productName`） | InkVeil |
| 一句话定位 | 开源 AI 法律本地脱敏工具 |
| 承诺标语 | 安全可靠 · 数据不联网 |

「墨」取自墨笔涂去，「隐」为隐去敏感信息；英文 **Ink**（墨）+ **Veil**（面纱/遮蔽）与之对应。
中英文之间保留一个半角空格，不写作「墨隐InkVeil」或「InkVeil 墨隐」。

## 标识

| 文件 | 用途 |
| --- | --- |
| [`inkveil-mark.svg`](inkveil-mark.svg) | 应用图标母版（1024×1024），`npx tauri icon docs/brand/inkveil-mark.svg` 可重新生成 `src-tauri/icons/` |
| [`inkveil-logo.svg`](inkveil-logo.svg) | 水平组合标，浅色背景 |
| [`inkveil-logo-dark.svg`](inkveil-logo-dark.svg) | 水平组合标，深色背景 |
| `src/components/InkVeilMark.tsx` | 应用内使用的同源 React 组件（标题栏、关于页） |
| `public/inkveil.svg` | 网页 favicon |

### 设计语义

- **宣纸底**：法律文书本身，也是「本地」——一切都在纸面上、在你的机器上完成。
- **一笔墨横**：用一笔浓墨遮去文书中的一行，即「脱敏」。笔画左起顿、右收笔，带一道飞白，是手写的墨，不是机器打的黑条。
- **朱砂小印**：盖章即生效、可追溯——对应审计留痕与可逆还原。
- **文书行**：四行灰线只在中大尺寸出现，小于 40px 时省略，保证托盘与标签页图标清晰。

### 色板

| 名称 | 色值 | 用途 |
| --- | --- | --- |
| 墨 Ink | `#1a1d2b → #0b0d14` | 主笔画、深色文字 |
| 宣纸 Paper | `#fbf7ef → #ece5d6` | 图标底、浅色文字 |
| 朱砂 Cinnabar | `#c4392c` | 印章，仅作点缀，不用于大面积填色 |
| 灰线 | `#bfb6a8` | 文书行 |
| 副文案 | `#6b6559`（浅底）/ `#b8b0a2`（深底） | 标语 |

### 字体

- 「墨隐」：宋体系（Songti SC / Noto Serif CJK SC / Source Han Serif），最粗字重。
- 「InkVeil」：系统无衬线（-apple-system / Inter / Helvetica Neue），Bold，字距 -2。
- 标语：PingFang SC / Noto Sans CJK SC，Medium，字距 +2。

### 禁忌

- 不拉伸、不旋转、不换色（深色背景请用 `inkveil-logo-dark.svg`，而不是反相）。
- 不在墨横上叠加文字或其他图形。
- 不把朱砂印放大成主体，也不去掉它。
- 图标四周至少留出 1/8 边长的安全距离。

## 代码层面的命名

品牌名只影响用户可见的文案与安装包名。以下技术标识**保持不变**，以兼容已发布版本与用户既有文件：

- Cargo / npm 包名 `anonkit`、sidecar 可执行文件 `anonkit-sidecar`
- 应用标识 `com.anonkit.desktop`、更新服务器 `updates.anonkit.dev`
- 文件格式与魔数：`.akcase`、`ANONKIT-VAULT`、`anonkit.anonymap.{json,enc}`、`anonkit.db`
- 环境变量 `ANONKIT_*`、配置键 `anonkit.config.v1`、PDF 占位字体 `AnonKitGlyphless`
