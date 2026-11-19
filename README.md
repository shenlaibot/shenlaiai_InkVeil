<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/inkveil-logo-dark.svg">
    <img alt="墨隐 InkVeil" src="docs/brand/inkveil-logo.svg" width="560">
  </picture>
</p>

# 墨隐 InkVeil

**开源 AI 法律本地脱敏工具 · 安全可靠 · 数据不联网**

墨隐 InkVeil 面向律师、法务与企业：把含个人信息的文书交给 AI 或外部协作方之前，
在本机完成「拖入 → 扫描 → 确认 → 导出脱敏副本」，并可凭映射文件一键还原。
文档解析、检测、脱敏全部在本地运行；联网仅发生在下载模型与检查更新，且可关闭。

- **多格式**：PDF / DOCX / XLSX / TXT / MD，尽力保留版式，无法处理的部位导出时列出。
- **多通道检测**：规则 + 词典 + 上下文 + 可选本地 NER 模型（ONNX Runtime，INT8，CPU 推理）。
- **可逆脱敏**：映射文件可加密，协作完成后还原为原文。
- **审计留痕**：每次脱敏/还原写入本地 SQLite（不含原文）。

技术栈：Tauri 2 + React + TypeScript，Rust 处理管线，本地 Python sidecar。
品牌资源与使用规范见 [docs/brand](docs/brand/README.md)。

## 开发

```bash
npm install
npm run tauri dev
```

推荐 IDE：[VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)。

## 发布工程与供应链

- CI、依赖锁文件（带哈希）、许可证合规与 SBOM、sidecar `--onedir` 变体、签名/公证清单：见 [docs/release.md](docs/release.md)
- 第三方组件声明：[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)；依赖清单：[DEPENDENCIES.md](DEPENDENCIES.md)（`python3 scripts/licenses.py` 生成）
- 安全问题报告：[SECURITY.md](SECURITY.md)

## 📜 许可说明 / License

本项目采用 **自定义许可协议**（非 OSI 开源协议），完整条款见 [LICENSE](./LICENSE)。

### ✅ 个人学习 / 非商用：免费

- 个人学习、研究、教学、实验等非商业场景，可以免费使用、修改、查看源码
- 分发时请保留版权声明与许可协议，并标注你所做的修改

### ⚠️ 商业使用：必须获得商业授权

以下任何商用场景，**必须事先联系作者获取商业授权**：

- 付费产品、付费功能、付费服务
- SaaS / 云服务 / API 服务
- 企业、机构内部系统集成或内置使用
- 二次售卖、转授权、租赁、打包分发
- 为第三方提供有偿技术服务、咨询、外包或定制开发

未经授权的商业使用视为侵权，作者保留追究法律责任的权利。

### 📮 商业授权联系方式

- GitHub Issues：<https://github.com/shenlaibot/shenlai_AnonKit/issues>
- 邮箱：shenlaibot@gmail.com

---

> **License summary (EN):** Free for personal learning and non-commercial use (use, modify, view source). **Any commercial use** (paid products, SaaS, enterprise integration, resale, commercial services) **requires a commercial license from the author.** See [LICENSE](./LICENSE) for full terms.
