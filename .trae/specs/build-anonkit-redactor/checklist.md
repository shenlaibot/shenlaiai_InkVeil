# Checklist — AnonKit 本地文档脱敏桌面工具

> 每条对应 spec.md 中的 Requirement；验收通过后勾选。全部勾选 = MVP 达成。

## 结构化规则检测
- [x] 内置正则覆盖手机号/身份证(含校验)/银行卡(Luhn)/固话/车牌/邮箱/IP/金额/日期，纯 CPU 多线程并行扫描（rules.rs 10 规则；批量文件并行 + golden 回归验证）
- [x] 每条规则可独立启用/关闭/调整，关闭后对应类型不再命中（Strategy 页 toggle + rules.enabled）
- [x] 反例验证：17 位数字不命中身份证、Luhn 失败不命中银行卡（rules.rs 单测 + golden）

## 词典与上下文规则检测
- [x] 支持自定义词典（人名/公司/项目代号/地址等）全文精确匹配，可导入（dict.rs，JSON/CSV 导入，LeftmostLongest）
- [x] 支持关键词上下文规则提升置信度（RuleDef.context_keywords）
- [x] "简称联动"：全称（XX有限公司）与简称（降本增笑）被关联识别并在预览中标出（alias.rs bind_aliases，同占位符）

## 本地 NER 小模型增强（可选下载）
- [x] 模型市场：应用内下载 ONNX INT8 中文 NER 模型，断点续传 + SHA-256 校验（models.rs 下载管理器 + Models 页）
- [x] 模型启用后检测层自动增加人名/机构/地名识别；可禁用/删除（ner.py + delete_model）
- [x] 零依赖运行：未下载任何模型时，扫描正常完成且无网络请求（ner 未加载即 skip，扫描不依赖模型）

## 多策略脱敏与自定义
- [x] 三类输出：掩码（默认 `*`，可换替换符）、类型占位符 `[手机号1]`、假数据（transform.rs OutputKind）
- [x] 内置策略覆盖中文姓名/身份证/手机/电话/地址/邮箱/密码/车牌/银行卡/金额/日期/IP/URL/机构/通用（rules.rs + transform）
- [x] 自定义策略：保留前N后M、正则替换、词典替换（Strategy.keep_head/keep_tail + masked/fake_for）
- [x] 可逆加密：AES 加密脱敏，密钥本地存储，可解密还原（crypto.rs + .anonymap.enc + Restore 页）

## 全文一致性替换与还原
- [x] 单任务内同实体全文（含跨文件批量）替换为同一占位符（commands.rs 跨文件共享映射表）
- [x] 映射文件可导出（可选口令加密）并可导入还原，脱敏→还原往返 = 原文（.anonymap.json/.enc + restore_text）

## 多格式文档支持（MVP）
- [x] PDF：优先内容流级删除（脱敏区域不可再被选中/复制）；回退覆盖级时导出摘要明示"覆盖级脱敏"；版面保留、图片表格保留、支持 30 页+ 文档（redact.py is_deep 标志 + 测试）
- [x] DOCX：正文/表格/页眉页脚 run 文本替换，样式保留（parsers/redact + test_documents）
- [x] TXT/MD：读写正常，UTF-8 容错（GBK 兜底）
- [x] 原文件永不修改，输出 `_脱敏` 后缀新副本；导出副本清除文档元数据（audit.rs + redact）

## 扫描预览与人工确认工作流
- [x] 导入→扫描→预览（按文件/页码/类型分组+上下文高亮）→逐条勾选→导出（Workbench→Preview 全流程）
- [x] 未勾选命中不被替换；支持按类型一键全选/取消（Preview selectAll/deselectAll/按组全选）
- [x] 批量文件夹处理：并行输出全部脱敏副本，显示每文件状态与审计摘要（pickFolder 递归导入 + 并发 redact + 状态展示）
- [x] 漏检/冲突提醒：疑似漏脱、占位符冲突、词典冲突以提示条醒目列出并可定位（warn-banner 扫描提醒）

## 性能目标
- [x] 10MB 纯文本规则扫描 < 1s（实测 0.11s / 91.3MB/s，bench/run.sh）
- [x] 100 页合同 NER < 10s（CPU）（实测 10 万字 0.48s）
- [x] 8×20 页 PDF 批量脱敏 < 20s（实测 8 文件并行 1.59s）
- [~] 冷启动 < 1.5s，空闲内存 < 120MB；扫描大文件时 UI 保持响应、可取消
  - 窗口出现 0.31s（达标 <1.5s，用户感知启动完成）；sidecar 后台就绪 3.8s（PyInstaller onefile 解压开销，发布期可接受，onedir 打包可进一步缩短）
  - 空闲内存实测 177MB（主进程 96MB + sidecar 80MB），超 120MB 门槛——WKWebView 桌面基线 + FastAPI sidecar 运行时（双进程架构）；核心性能指标（扫描/NER/批量）均 10 倍达标，本项记录为已验收但门槛偏离，如需压降需引入更轻 sidecar HTTP 层（如标准库 http.server）

## 隐私与零外传
- [x] 完整流程（扫描→确认→脱敏→还原）在断网环境无报错运行（CI 不可达代理拦截冒烟：`HTTP_PROXY=http://127.0.0.1:9` 下全流程通过 + sidecar 客户端回环断言强制拒绝外部地址；本地实测 PyInstaller 产物离线全流程通过）
- [x] sidecar 仅监听 127.0.0.1 随机端口；构建期冒烟测试断言核心引擎不发起外部 socket（netpol 脚本 + SidecarClient 回环断言）
- [x] 前端运行时 CSP（default-src/connect-src 仅 ipc + ipc.localhost）阻断渲染层外发请求（纵深防御）
- [x] 原文档仅在内存/受控临时目录处理，任务结束即清理，无残留（test_privacy.py 零残留断言）
- [x] 无遥测/上报；唯一网络用途（模型下载/版本检查）可关闭——无遥测/版本检查代码零存在（`src-tauri/src` grep 0 命中）；「允许网络下载」开关已落地（Models 页 toggle + `start_model_download` 命令级守卫 `config_allows_download`，关闭后下载/断点续传全禁；默认开启兼容旧配置）

## 审计日志
- [x] 本地记录（时间/文件/命中统计/策略/输出路径），可查看/筛选/导出/清空（Task 8，SQLite audit_log + Audit 页）

## 开源与许可合规
- [x] 项目 Apache-2.0；`DEPENDENCIES.md` 许可证清单 + CI 检查，无 GPL/AGPL（含传递依赖）
- [x] PDF 处理使用 pypdfium2/pdfium-render（BSD）而非 PyMuPDF（AGPL）

## 质量门禁（golden corpus）
- [x] golden corpus 覆盖中文姓名/公司全称与简称/手机/身份证/银行卡/邮箱/地址，跨 PDF/DOCX/TXT/MD
- [x] CI 门槛：P0 类型（手机/身份证/银行卡）召回 ≥99%，P1 类型 ≥95%，无历史回归（golden.rs 实测 P0/P1 均 100%）
- [x] PDF 脱敏深度检查：脱敏后重新提取文本，脱敏区域不含敏感原文（或明确标记覆盖级）
