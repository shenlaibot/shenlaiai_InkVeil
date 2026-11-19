# 发布工程与供应链（release-eng）

> 本文记录 CI、依赖锁定、许可证/SBOM、sidecar 打包变体（onedir）和签名/公证的现状与待办。
> 验证环境：macOS（arm64）+ Python 3.11（uv 创建）。**Windows / Linux 路径未能在本机验证**，处用“未验证”标出。

## 1. CI 结构（`.github/workflows/ci.yml`）

| Job | 内容 | 阻断 |
|---|---|---|
| `rust-test` | `cargo test --workspace --locked` | 是 |
| `rust-lint` | `cargo fmt --check`、`cargo clippy -D warnings` | **否**（`continue-on-error` + `::warning::` + Step Summary 上报） |
| `typescript` | `npm ci` + `npx tsc --noEmit` | 是 |
| `sidecar-pytest` | Python 3.11，`pip install --require-hashes -r requirements-dev.lock`，pytest + 代理封死的隐私冒烟 | 是 |
| `corpus-e2e` | 建 `sidecar/.venv`（锁文件）后 `cargo test -p anonymizer-pipeline --test corpus_e2e`；测试本身缺 venv 时仍会自动跳过（逻辑保留），CI 显式建 venv 保证真跑 | 是 |
| `supply-chain` | `check-network-policy.sh` + `licenses.py --check-only --sbom-dir dist/sbom`，上传 SBOM artifact | 是 |

要点：
- 去掉了所有 `|| echo` 吞错；原 `build-macos/build-windows` 里“失败也算过”的 `tauri build --no-bundle || echo` 已删除。
  安装包构建只在 `build-installers.yml`（tag / 手动）中做，且不再静默忽略失败。
- 所有 Action 以 commit SHA 固定（行尾注释为版本）。已核实的 SHA：checkout v4.2.2、setup-node v4.4.0、setup-python v5.6.0、
  cache v4.2.3、upload-artifact v4.6.2。`dtolnay/rust-toolchain` 没有版本 tag（只有 `stable` 分支），
  固定到 master 当前 commit `683b588c…`，工具链由 `with: toolchain: stable` 指定——**待办**：改为固定具体 Rust 版本（`rust-toolchain.toml`）以获得可复现构建。
- CI 在构建前删除 `.cargo/config.toml`（它是本地用的 rsproxy 国内镜像），直连官方 crates.io；依赖完整性由 `Cargo.lock` checksum 保证。
- `workflow` 校验：`actionlint`（actionlint-py 1.7.12）与 PyYAML 解析均通过。

## 2. Python 依赖锁定

| 文件 | 说明 |
|---|---|
| `sidecar/requirements.txt` | 运行时直接依赖（宽松，作为 *输入*） |
| `sidecar/requirements-dev.txt` | 测试/开发（pytest、httpx、reportlab、huggingface_hub） |
| `sidecar/requirements-build.txt` | 构建期（`pyinstaller`） |
| `sidecar/requirements.lock` | 运行时锁文件（版本 + sha256 哈希，universal，Python ≥3.11 标记） |
| `sidecar/requirements-dev.lock` | 运行时 + dev |
| `sidecar/requirements-build.lock` | 运行时 + PyInstaller（`build-sidecar.sh` 与发布 workflow 使用，PyInstaller 固定为 6.22.3） |

重新生成（需 [uv](https://github.com/astral-sh/uv)，等价于 `pip-compile --generate-hashes`）：

```bash
cd sidecar
uv pip compile requirements.txt                          --universal --python-version 3.11 --generate-hashes -o requirements.lock
uv pip compile requirements.txt requirements-build.txt   --universal --python-version 3.11 --generate-hashes -o requirements-build.lock
uv pip compile requirements.txt requirements-dev.txt     --universal --python-version 3.11 --generate-hashes -o requirements-dev.lock
```

- 使用 `--universal`，同一份锁覆盖 macOS / Windows / Linux（含平台条件包 colorama、pefile、macholib 等），目标 Python 3.11。
  本机默认 venv 是 Python 3.9，**不能**直接安装这些锁（numpy 2.4 等要求 ≥3.11）；锁生成不依赖本机 Python 版本。
- 已验证：用 Python 3.11 新建 venv，`--require-hashes` 装 `requirements-dev.lock` 与 `requirements-build.lock`，
  `pytest sidecar/tests` 结果 **68 passed, 7 skipped**。注意锁定后的版本比旧本机 venv 新得多（如 onnxruntime 1.31.1 对 1.19.2、pikepdf 10.17.0 对 9.11.0、
  fastapi 0.162 对 0.128）；测试通过，但**建议再跑一遍 corpus_e2e 并人工抽查 PDF 输出**后再冻结发布。
- `build-sidecar.sh` 从 `requirements-build.lock` 安装；若 venv 不是 3.11 会拒绝（`SKIP_LOCK_INSTALL=1` / `ALLOW_NONLOCK_PYTHON=1` 仅限本地调试）。
  可用 `SIDECAR_PY=/path/to/python` 指定解释器。

> 修复（顺带发现）：`build-sidecar.sh` 的健康自检在“sidecar 默认必须带 token”的安全加固后一直无法通过（sidecar 直接拒绝启动）。
> 现已改为用一次性随机 token 启动并带 `Authorization: Bearer` 请求。

## 3. 许可证合规与 SBOM（`scripts/licenses.py`）

功能：
- 三个生态：cargo（`cargo metadata --locked`）、npm（`package-lock.json`）、Python（以锁文件定包集合，从 venv 的 dist-info 读许可证）。
- **SPDX 表达式解析**（OR / AND / WITH / 括号；旧写法如 `MIT/Apache-2.0`、`Apache-2.0 / MIT`、自由文本分类器会先归一化）。
  `OR` 取最宽松分支，`AND` 取最严格分支。
- 失败条件：GPL/AGPL/LGPL/SSPL/NC/BUSL 等；**“未声明/无法解析”也判失败**；MPL 等弱 copyleft 单独列出，且包名必须出现在 `THIRD-PARTY-NOTICES.md`；数据源缺失（如 venv 不存在）直接失败而非静默跳过。
- 例外集中在 `scripts/license-overrides.json`（每条必须有 `reason`），不在代码里硬编码。
- 输出：`DEPENDENCIES.md`（默认）、CycloneDX 1.5 SBOM：`dist/sbom/anonkit-{cargo,npm,python,all}.cdx.json`（`--sbom-dir`）、`--write-notices` 更新 MPL 自动清单块。

本地运行（Python 3.11 venv 装好 `requirements-build.lock`）：

```bash
python3 scripts/licenses.py --venv <venv> --write-notices --sbom-dir dist/sbom
```

### 本次实际运行结果（2026-11-08，macOS）

```
共收集 753 个依赖（cargo 573 / npm 136 / python 44）
○ 构建期工具豁免 1 项：pyinstaller-hooks-contrib 2026.9: Apache-2.0 AND GPL-2.0-or-later
△ 弱 copyleft（MPL 等）7 项：cargo cssparser / cssparser-macros / dtoa-short / option-ext / selectors；python pikepdf / tqdm
SBOM: dist/sbom/anonkit-{cargo,npm,python,all}.cdx.json   （组件数 573 / 136 / 44 / 753，specVersion 1.5）
合规检查通过
```

**当前不合规项清单**

- 阻断项：**无**（脚本退出码 0）。
- 首次运行时曾被判失败、现已处理的项（供追溯）：
  - `fnv`（`Apache-2.0 / MIT` 非标准写法）→ 解析器归一化；
  - `pypdfium2`（自由文本 `BSD-3-Clause, Apache-2.0, dependency licenses`）→ 覆盖表；
  - `pyinstaller`（元数据仅自由文本 GPLv2 + 例外）→ 覆盖表为 `GPL-2.0-or-later WITH Bootloader-exception`；
  - `colorama` / `pefile` / `pywin32-ctypes`（仅 Windows 平台依赖，本机无元数据，按上游许可证覆盖，**待 Windows CI 首次运行复核**）；
  - `httpx2-jsfetch`（仅 emscripten/py≥3.12 条件包，桌面三平台不会安装）→ 解析锁文件时排除。
- 需要人工拍板的豁免：`pyinstaller-hooks-contrib`（标准 hooks 为 GPL-2.0-or-later，runtime hooks 为 Apache-2.0；仅构建机运行，不随包分发）。
- 其余待核实（详见 `THIRD-PARTY-NOTICES.md`）：pikepdf Linux wheel 内捆绑的 GCC 运行时（GPL-3.0 + GCC Runtime Exception）对 AppImage/deb 的影响；
  NER 模型自身许可证；tqdm 是否进入最终包；npm / cargo 许可证汇总文件尚未自动打进安装包。
- 数据局限：Python 许可证取自**本机 macOS 的 3.11 venv**；Linux/Windows 的 wheel 可能捆绑不同的原生库，需在对应 CI 上各跑一次 `licenses.py`（`supply-chain` job 目前只在 ubuntu 上跑）。

## 4. 供应链其他事项

- `SECURITY.md`：漏洞报告渠道（**占位，需填真实邮箱/PGP**）、响应时限、协调披露。
- `THIRD-PARTY-NOTICES.md`：PDFium 及捆绑库、onnxruntime、pikepdf/qpdf（MPL 源码获取方式）、字体 OFL 等；无法核实处标“待核实”。
- 待办：`cargo deny`（advisories/bans/sources）与 `npm audit --omit=dev`、`pip-audit -r sidecar/requirements.lock` 接入 CI 作漏洞扫描；
  对 SBOM 做签名/随 release 一并发布；`Cargo.lock` / `package-lock.json` 用 `--locked` / `npm ci`（已改）。

## 5. sidecar：`--onefile` → `--onedir`

### 为什么
`--onefile` 每次启动要把整个包解压到临时目录（本项目含 onnxruntime / pdfium / pikepdf，冷启动慢），且自解压行为更易被杀软误报，也妨碍对内部文件逐个签名。
`--onedir` 启动快、可对内部每个二进制单独签名/公证。

### 构建（已实现并本机验证）

```bash
SIDECAR_PY=<py311-venv>/bin/python bash scripts/build-sidecar.sh --onedir
# 产物：src-tauri/binaries/anonkit-sidecar-<triple>-onedir/{anonkit-sidecar[.exe], _internal/...}
```

本机 macOS arm64 实测：`/health` 与真实 docx `/parse` 自检均通过，目录体积 158 MB。`--onefile`（默认）行为不变。

### 需要的 Tauri / Rust 改动（本任务**未改动**，待办步骤）

1. **tauri.conf.json**：删除 `bundle.externalBin: ["binaries/anonkit-sidecar"]`，改用资源目录（Tauri 2 的 map 形式会保留目录结构）：
   ```json
   "bundle": { "resources": { "binaries/anonkit-sidecar-<triple>-onedir/": "sidecar/" } }
   ```
   `<triple>` 不能写变量：为各平台准备 `tauri.macos.conf.json` / `tauri.windows.conf.json` / `tauri.linux.conf.json`，
   或在构建脚本里把目录固定复制为 `src-tauri/binaries/sidecar-onedir/`（更简单，推荐——`build-sidecar.sh --onedir` 增加一个固定名拷贝即可，**待做**）。
2. **src-tauri/src/sidecar.rs `release_command`**：现在用 `current_exe().parent()/anonkit-sidecar[.exe]`（externalBin 与主程序同目录）。
   改为通过 `app.path().resource_dir()`（或 `BaseDirectory::Resource`）解析 `sidecar/anonkit-sidecar[.exe]`；
   `SidecarManager::start()` 需要拿到 `AppHandle`（调用方在 `lib.rs`/`main.rs` 里调整）；`current_dir` 设为该目录，保证 `_internal/` 可被找到。
   macOS 下资源位于 `InkVeil.app/Contents/Resources/sidecar/`。
3. **capabilities / shell 权限**：若走 `tauri-plugin-shell` 的 sidecar API 需要改成普通 `std::process::Command`/`tokio::process`（目前代码已是后者，无需 shell 的 `sidecar` 权限）。
4. **CI**：`build-installers.yml` 里 `bash scripts/build-sidecar.sh` → `bash scripts/build-sidecar.sh --onedir`；
   `ci.yml` 不依赖该产物，无需改。
5. **签名**（见下）：onedir 内部大量 `.so/.dylib/.pyd/.dll` 与主可执行文件都需要单独签名，不能依赖 `codesign --deep`。
6. **`sidecar/pyinstaller.spec` 已过期**（仍排除 onnxruntime/numpy，且与脚本的 `--collect-all` 参数不一致，实际构建不使用它）；
   二选一：删除，或改成与脚本等价的 spec 后让脚本改用 spec。**待办。**
7. 回归：`cargo test --workspace`、`corpus_e2e`、首次启动到 `/health` ready 的耗时对比（onefile vs onedir）、卸载是否清理干净（onedir 在安装目录，不写临时目录）。

## 6. 签名 / 公证清单（占位，**不含任何密钥**）

所有密钥/证书只放 GitHub Actions Secrets（或云端 HSM/签名服务），**绝不入库**。变量名为占位，按实际命名。

### 6.1 macOS 签名 + 公证（未实现，未在本机验证）

前置：Apple Developer Program 账号；Developer ID Application 证书（.p12）。
- [ ] Secrets：`APPLE_CERTIFICATE`（p12 的 base64）、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`（"Developer ID Application: …"）、
      `APPLE_ID`、`APPLE_PASSWORD`（app 专用密码）、`APPLE_TEAM_ID`（Tauri 读取这些环境变量完成签名+公证；也可改用 App Store Connect API key）。
- [ ] CI 里建临时 keychain 导入证书（job 结束删除）。
- [ ] 先对 sidecar 内部所有 Mach-O（`_internal/**/*.so|*.dylib` 与 `anonkit-sidecar`）逐个 `codesign --force --timestamp --options runtime --entitlements <sidecar.entitlements> -s "$APPLE_SIGNING_IDENTITY"`，再让 Tauri 签 `.app`。
- [ ] 准备 entitlements（`com.apple.security.cs.allow-unsigned-executable-memory` 等视 onnxruntime/PyInstaller 需要，**待实测**；原则上最小化）。
- [ ] 公证：`xcrun notarytool submit … --wait`，成功后 `xcrun stapler staple` dmg/app；失败时拉取 `notarytool log`。
- [ ] 验收：`codesign --verify --deep --strict`、`spctl -a -vv`、全新机器首次打开无 Gatekeeper 警告。
- [ ] 与 updater 签名（minisign，`TAURI_SIGNING_PRIVATE_KEY`）是**两套独立密钥**，不要混淆。

### 6.2 Windows 签名（未实现，**无法在 macOS 验证**）

- [ ] 选型：OV/EV 代码签名证书（硬件令牌限制 CI）或 Azure Trusted Signing / 云 HSM（推荐，可在 CI 使用）。
- [ ] Secrets（占位）：`WINDOWS_CERTIFICATE`/`WINDOWS_CERTIFICATE_PASSWORD`（或 Azure 的 tenant/client/endpoint 等）。
- [ ] `tauri.conf.json` 的 `bundle.windows.signCommand`（或 `certificateThumbprint`）配置，对主 exe、NSIS 安装包、**sidecar 的 exe 与 `_internal` 内 `.dll/.pyd`** 全部签名并加时间戳（`/tr`、`/td sha256`）。
- [ ] 验收：`signtool verify /pa /v`；SmartScreen 信誉需要时间积累（EV 可立即获得）。
- [ ] 杀软误报排查：onedir 比 onefile 好，但仍需向主流厂商提交白名单申请（待办）。

### 6.3 Linux
- [ ] AppImage/deb 的 GPG 分离签名与校验和（`SHA256SUMS` + 签名）发布到 Release；公钥写在 `SECURITY.md`。

### 6.4 发布前检查清单
- [ ] `licenses.py` 在 mac / win / linux 三平台 CI 均通过，SBOM 随 Release 发布
- [ ] `THIRD-PARTY-NOTICES.md` 全部“待核实”关闭，且 wheel 内的许可证文本已打进安装包
- [ ] `SECURITY.md` 占位符（邮箱 / PGP）已替换
- [ ] 签名与公证验收通过；updater 私钥只在 Secrets 中
