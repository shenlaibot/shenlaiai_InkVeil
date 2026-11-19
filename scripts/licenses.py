#!/usr/bin/env python3
"""墨隐 InkVeil 依赖许可证合规检查 / 清单生成 / CycloneDX SBOM（离线，仅用标准库）。

覆盖三个生态：
  - cargo ：`cargo metadata --locked`（license 字段来自各 crate 的 Cargo.toml）
  - npm   ：package-lock.json 的 packages[*].license
  - python：以 sidecar/requirements.lock（与 requirements-build.lock）为准确定“包名+版本”，
            许可证从 venv 的 *.dist-info/METADATA 读取（License-Expression > License > Classifier）

许可证按 SPDX 表达式解析（OR / AND / WITH / 括号），语义：
  - A OR B  ：取等级最低（最宽松）的分支——使用者可任选其一；
  - A AND B ：取等级最高的分支——需同时遵守；
  - 等级：0 宽松 / 1 弱 copyleft（MPL 等，需在 THIRD-PARTY-NOTICES 中单独列出）/
          2 禁止（GPL、AGPL、LGPL、SSPL、NC 等）/ 3 无法解析（未声明或未知标识符）。

退出码 1 的条件（任一）：
  - 存在等级 2 或 3 的依赖（“未声明/无法解析”同样判失败）；
  - 存在等级 1（MPL）依赖但其名称未出现在 THIRD-PARTY-NOTICES.md；
  - 某生态的数据源不可用（例如 python venv 不存在）——不会静默跳过。

人工核实过的例外写在 scripts/license-overrides.json（必须带 reason），不在代码里硬编码。

用法：
  python3 scripts/licenses.py                       # 检查 + 重写 DEPENDENCIES.md
  python3 scripts/licenses.py --check-only          # 仅检查
  python3 scripts/licenses.py --sbom-dir dist/sbom  # 同时输出 CycloneDX SBOM（各生态 + 合并）
  python3 scripts/licenses.py --write-notices       # 更新 THIRD-PARTY-NOTICES.md 中的 MPL 自动清单块
  python3 scripts/licenses.py --venv sidecar/.venv --only python,npm
"""
from __future__ import annotations

import argparse
import base64
import datetime
import email
import json
import os
import pathlib
import re
import subprocess
import sys
import uuid

ROOT = pathlib.Path(__file__).resolve().parent.parent
OVERRIDES_PATH = ROOT / "scripts" / "license-overrides.json"
NOTICES_PATH = ROOT / "THIRD-PARTY-NOTICES.md"
NOTICE_BEGIN = "<!-- BEGIN AUTO-MPL-LIST (scripts/licenses.py --write-notices 生成，勿手改) -->"
NOTICE_END = "<!-- END AUTO-MPL-LIST -->"

# 本仓库自有 crate（cargo metadata 中 source 为空）的许可证：见仓库根 LICENSE（Apache-2.0）。
FIRST_PARTY_LICENSE = "Apache-2.0"

LEVEL_PERMISSIVE, LEVEL_WEAK, LEVEL_FORBIDDEN, LEVEL_UNKNOWN = 0, 1, 2, 3
LEVEL_NAME = {0: "宽松", 1: "弱copyleft(MPL等)", 2: "禁止", 3: "无法解析"}

# ---- SPDX 标识符分类 ------------------------------------------------------
PERMISSIVE_IDS = {
    "mit", "mit-0", "apache-2.0", "apache-1.1", "bsd-1-clause", "bsd-2-clause", "bsd-3-clause",
    "bsd-2-clause-patent", "bsd-3-clause-clear", "isc", "zlib", "libpng-2.0", "unlicense", "0bsd",
    "cc0-1.0", "bsl-1.0", "python-2.0", "psf-2.0", "unicode-3.0", "unicode-dfs-2016", "ofl-1.1",
    "cdla-permissive-2.0", "cdla-permissive-1.0", "blueoak-1.0.0", "hpnd", "ncsa", "wtfpl",
    "cc-by-4.0", "cc-by-3.0", "openssl", "curl", "x11", "mit-cmu", "mpl-2.0-no-copyleft-exception-x",
    "artistic-2.0", "postgresql", "apache-2.0-with-llvm-exception", "bzip2-1.0.6", "ijg", "ftl",
}
WEAK_IDS = {
    "mpl-2.0", "mpl-1.1", "mpl-1.0", "epl-1.0", "epl-2.0", "cddl-1.0", "cddl-1.1", "cpl-1.0",
    "ms-rl", "osl-3.0",
}
FORBIDDEN_RE = re.compile(r"(?:^|[-_.])(?:a?gpl|lgpl|sspl|busl|nc|commons-clause|cc-by-nc)(?:$|[-_.\d])", re.I)
# 明确允许的 WITH 例外（把本来的 GPL 变为可用于闭源/商用分发）
SAFE_EXCEPTIONS = {"bootloader-exception", "llvm-exception"}

# 旧式（非 SPDX）自由文本到 SPDX 的归一化表（全部小写匹配）
LEGACY_MAP = {
    "mit license": "MIT", "mit": "MIT", "the mit license": "MIT", "expat": "MIT",
    "bsd license": "BSD-3-Clause", "bsd": "BSD-3-Clause", "new bsd license": "BSD-3-Clause",
    "bsd 3-clause": "BSD-3-Clause", "3-clause bsd license": "BSD-3-Clause",
    "bsd 2-clause": "BSD-2-Clause", "simplified bsd": "BSD-2-Clause",
    "apache software license": "Apache-2.0", "apache 2.0": "Apache-2.0", "apache-2": "Apache-2.0",
    "apache license 2.0": "Apache-2.0", "apache license, version 2.0": "Apache-2.0",
    "apache 2": "Apache-2.0", "apache2": "Apache-2.0",
    "isc license": "ISC", "isc license (iscl)": "ISC",
    "python software foundation license": "PSF-2.0", "psf": "PSF-2.0",
    "mozilla public license 2.0 (mpl 2.0)": "MPL-2.0", "mpl 2.0": "MPL-2.0",
    "the unlicense (unlicense)": "Unlicense", "public domain": "CC0-1.0",
}
CLASSIFIER_MAP = {
    "mit license": "MIT", "bsd license": "BSD-3-Clause", "apache software license": "Apache-2.0",
    "isc license (iscl)": "ISC", "mozilla public license 2.0 (mpl 2.0)": "MPL-2.0",
    "python software foundation license": "PSF-2.0", "the unlicense (unlicense)": "Unlicense",
    "gnu lesser general public license v3 (lgplv3)": "LGPL-3.0-only",
    "gnu general public license v3 (gplv3)": "GPL-3.0-only",
    "gnu affero general public license v3": "AGPL-3.0-only",
    "public domain": "CC0-1.0",
}


class SpdxError(Exception):
    pass


def _tokenize(expr: str) -> list[str]:
    return re.findall(r"\(|\)|[^\s()]+", expr)


def _classify_id(lic_id: str, exception: str | None) -> tuple[int, str]:
    """单个标识符（可带 WITH 例外）→ (等级, 规范化文本)。"""
    low = lic_id.lower().rstrip("+")
    text = lic_id if not exception else f"{lic_id} WITH {exception}"
    if exception and exception.lower() in SAFE_EXCEPTIONS:
        return LEVEL_PERMISSIVE, text
    if low in PERMISSIVE_IDS:
        return LEVEL_PERMISSIVE, text
    if low in WEAK_IDS:
        return LEVEL_WEAK, text
    if FORBIDDEN_RE.search(lic_id):
        return LEVEL_FORBIDDEN, text
    return LEVEL_UNKNOWN, text


class _Parser:
    def __init__(self, tokens: list[str]):
        self.t, self.i = tokens, 0

    def peek(self) -> str | None:
        return self.t[self.i] if self.i < len(self.t) else None

    def next(self) -> str:
        tok = self.peek()
        if tok is None:
            raise SpdxError("表达式意外结束")
        self.i += 1
        return tok

    def parse(self):
        node = self.expr()
        if self.peek() is not None:
            raise SpdxError(f"多余的记号: {self.peek()}")
        return node

    def expr(self):  # OR 优先级最低
        nodes = [self.term()]
        while (self.peek() or "").upper() == "OR":
            self.next()
            nodes.append(self.term())
        return nodes[0] if len(nodes) == 1 else ("OR", nodes)

    def term(self):
        nodes = [self.factor()]
        while (self.peek() or "").upper() == "AND":
            self.next()
            nodes.append(self.factor())
        return nodes[0] if len(nodes) == 1 else ("AND", nodes)

    def factor(self):
        tok = self.next()
        if tok == "(":
            node = self.expr()
            if self.next() != ")":
                raise SpdxError("括号不匹配")
            return node
        if tok in (")",) or tok.upper() in ("AND", "OR", "WITH"):
            raise SpdxError(f"意外记号: {tok}")
        exc = None
        if (self.peek() or "").upper() == "WITH":
            self.next()
            exc = self.next()
        return ("ID", tok, exc)


def _eval(node) -> tuple[int, list[str]]:
    kind = node[0]
    if kind == "ID":
        lvl, text = _classify_id(node[1], node[2])
        return lvl, [text]
    results = [_eval(n) for n in node[1]]
    if kind == "OR":  # 取最宽松
        return min(results, key=lambda r: r[0])
    lvl = max(r[0] for r in results)  # AND：取最严
    ids: list[str] = []
    for r in results:
        ids += r[1]
    return lvl, ids


def normalize_license(raw: str | None) -> str | None:
    """把各生态的原始写法归一成 SPDX 表达式文本；无法归一返回原文。"""
    if raw is None:
        return None
    s = " ".join(raw.split())
    if not s:
        return None
    if s.lower() in LEGACY_MAP:
        return LEGACY_MAP[s.lower()]
    # cargo 旧写法 "MIT/Apache-2.0" / "Apache-2.0 / MIT"（SPDX 无斜杠，按 OR 理解）
    if "/" in s:
        s = re.sub(r"\s*/\s*", " OR ", s)
    return s


def evaluate(raw: str | None) -> dict:
    """返回 {expr, level, ids}；level=3 表示未声明或无法解析。"""
    expr = normalize_license(raw)
    if not expr:
        return {"expr": "未声明", "level": LEVEL_UNKNOWN, "ids": [], "reason": "未声明"}
    try:
        lvl, ids = _eval(_Parser(_tokenize(expr)).parse())
    except SpdxError as exc:
        return {"expr": expr, "level": LEVEL_UNKNOWN, "ids": [], "reason": f"无法解析: {exc}"}
    reason = "" if lvl < LEVEL_UNKNOWN else "含未知 SPDX 标识符"
    return {"expr": expr, "level": lvl, "ids": ids, "reason": reason}


# ---- 覆盖表 ----------------------------------------------------------------
def load_overrides() -> dict:
    if not OVERRIDES_PATH.exists():
        return {}
    data = json.loads(OVERRIDES_PATH.read_text(encoding="utf-8"))
    out = {}
    for key, val in data.items():
        if key.startswith("_"):
            continue
        if not val.get("license") or not val.get("reason"):
            raise SystemExit(f"{OVERRIDES_PATH.name}: 条目 {key} 必须同时含 license 与 reason")
        out[key.lower()] = val
    return out


# ---- 数据采集 ---------------------------------------------------------------
def _pkg(eco, name, version, lic, scope, extra=None):
    d = {"ecosystem": eco, "name": name, "version": version, "license_raw": lic, "scope": scope}
    d.update(extra or {})
    return d


def cargo_packages() -> list[dict]:
    lock_text = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    checksums = {}
    for m in re.finditer(r'name = "([^"]+)"\nversion = "([^"]+)"\n(?:source = "[^"]*"\n)?(?:checksum = "([0-9a-f]+)")?', lock_text):
        if m.group(3):
            checksums[(m.group(1), m.group(2))] = m.group(3)
    last_err = None
    meta = None
    for extra in (["--locked"], ["--locked", "--offline"]):
        try:
            out = subprocess.run(
                ["cargo", "metadata", "--format-version", "1", *extra],
                cwd=ROOT, capture_output=True, text=True, timeout=600, check=True,
            )
            meta = json.loads(out.stdout)
            break
        except Exception as exc:  # noqa: BLE001
            last_err = getattr(exc, "stderr", None) or exc
    if meta is None:
        raise SystemExit(f"cargo metadata 失败（不会回退为“未声明”）：{last_err}")
    rows = []
    for p in meta["packages"]:
        first_party = p.get("source") is None
        lic = FIRST_PARTY_LICENSE if first_party else p.get("license")
        if not lic and p.get("license_file"):
            lic = None  # 仅有 license_file：视为无法机读，交给覆盖表
        rows.append(_pkg(
            "cargo", p["name"], p["version"], lic, "first-party" if first_party else "runtime",
            {"checksum": checksums.get((p["name"], p["version"])), "first_party": first_party},
        ))
    return rows


def npm_packages() -> list[dict]:
    lock = json.loads((ROOT / "package-lock.json").read_text(encoding="utf-8"))
    rows = []
    for path, info in lock.get("packages", {}).items():
        if not path:  # 根包
            continue
        name = info.get("name") or path.split("node_modules/")[-1]
        lic = info.get("license")
        if isinstance(lic, dict):
            lic = lic.get("type")
        elif isinstance(lic, list):
            lic = " OR ".join(x.get("type", "") if isinstance(x, dict) else str(x) for x in lic)
        rows.append(_pkg(
            "npm", name, info.get("version", "?"), lic,
            "dev" if info.get("dev") else "runtime",
            {"integrity": info.get("integrity")},
        ))
    return rows


def _norm_py(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def parse_lock(path: pathlib.Path) -> dict[str, str]:
    """requirements*.lock → {规范化包名: 版本}（同名多版本取首个，如 numpy 的 py 版本分支）。"""
    out: dict[str, str] = {}
    if not path.exists():
        return out
    for line in path.read_text(encoding="utf-8").splitlines():
        m = re.match(r"^([A-Za-z0-9][A-Za-z0-9._-]*)==([^\s;\\]+)(?:\s*;\s*(.*?))?\s*\\?$", line)
        if not m:
            continue
        marker = m.group(3) or ""
        # 目标为 Python 3.11 的桌面三平台：仅 emscripten(WASM) 或仅 py>=3.12 的条件包永远不会被安装，排除
        if "== 'emscripten'" in marker:
            continue
        if re.match(r"^python_full_version >= '3\.(1[2-9])'", marker):
            continue
        out.setdefault(_norm_py(m.group(1)), m.group(2))
    return out


def _site_packages(venv: pathlib.Path) -> list[pathlib.Path]:
    cands = list(venv.glob("lib/python3*/site-packages")) + [venv / "Lib" / "site-packages"]
    return [c for c in cands if c.is_dir()]


def _read_py_license(msg) -> str | None:
    expr = (msg.get("License-Expression") or "").strip()
    if expr:
        return expr
    lic = (msg.get("License") or "").strip()
    # 部分包把整份许可证正文塞进 License 字段：过长/多行视为无效，回退 Classifier
    if lic and len(lic) <= 100 and "\n" not in lic and lic.upper() != "UNKNOWN":
        return lic
    found = []
    for c in msg.get_all("Classifier", []) or []:
        if c.startswith("License ::"):
            tail = c.split("::")[-1].strip().lower()
            if tail.startswith("osi approved"):
                continue
            found.append(CLASSIFIER_MAP.get(tail, c.split("::")[-1].strip()))
    return " AND ".join(found) if found else None


def python_packages(venv: pathlib.Path) -> list[dict]:
    runtime = parse_lock(ROOT / "sidecar" / "requirements.lock")
    build = parse_lock(ROOT / "sidecar" / "requirements-build.lock")
    if not runtime:
        raise SystemExit("缺少 sidecar/requirements.lock，无法确定 Python 依赖集合")
    if not venv.is_dir():
        raise SystemExit(f"python venv 不存在：{venv}（需先按锁文件安装依赖，或用 --only 排除 python）")
    installed = {}
    for sp in _site_packages(venv):
        for meta in sp.glob("*.dist-info/METADATA"):
            with meta.open("r", encoding="utf-8", errors="replace") as f:
                msg = email.message_from_file(f)
            nm = _norm_py(msg.get("Name") or meta.parent.name.split("-")[0])
            installed[nm] = (msg.get("Version", "?"), _read_py_license(msg))
    rows = []
    for nm, ver in sorted({**build, **runtime}.items()):
        scope = "runtime" if nm in runtime else "build"
        inst = installed.get(nm)
        extra = {}
        if inst is None:
            lic = None
            extra["note"] = "venv 中未安装（平台条件包？），许可证需覆盖表核实"
        else:
            lic = inst[1]
            if inst[0] != ver:
                extra["note"] = f"venv 版本 {inst[0]} ≠ 锁文件 {ver}（许可证取自 venv）"
        rows.append(_pkg("python", nm, ver, lic, scope, extra))
    return rows


def apply_overrides(rows: list[dict], overrides: dict) -> None:
    for r in rows:
        key = f"{r['ecosystem']}:{r['name']}".lower()
        ov = overrides.get(f"{key}@{r['version']}".lower()) or overrides.get(key)
        if ov:
            r["license_raw"] = ov["license"]
            r["override_reason"] = ov["reason"]
            r["exempt_build_tool"] = bool(ov.get("exempt_build_tool"))


# ---- 评估与输出 ---------------------------------------------------------------
def evaluate_all(rows: list[dict]) -> None:
    for r in rows:
        r.update(evaluate(r["license_raw"]))


def mpl_names_in_notices() -> str:
    return NOTICES_PATH.read_text(encoding="utf-8").lower() if NOTICES_PATH.exists() else ""


def render_mpl_block(weak: list[dict]) -> str:
    seen = {}
    for r in weak:
        seen.setdefault((r["ecosystem"], r["name"]), set()).add(r["version"])
    lines = [NOTICE_BEGIN, "", "| 生态 | 包 | 版本 | 许可证 |", "|---|---|---|---|"]
    for (eco, name), vers in sorted(seen.items(), key=lambda kv: (kv[0][0], kv[0][1].lower())):
        lic = next(r["expr"] for r in weak if r["ecosystem"] == eco and r["name"] == name)
        lines.append(f"| {eco} | {name} | {', '.join(sorted(vers))} | {lic} |")
    lines += ["", NOTICE_END]
    return "\n".join(lines)


def write_notices_block(weak: list[dict]) -> None:
    text = NOTICES_PATH.read_text(encoding="utf-8")
    block = render_mpl_block(weak)
    if NOTICE_BEGIN in text and NOTICE_END in text:
        pre = text.split(NOTICE_BEGIN)[0]
        post = text.split(NOTICE_END, 1)[1]
        text = pre + block + post
    else:
        text = text.rstrip("\n") + "\n\n" + block + "\n"
    NOTICES_PATH.write_text(text, encoding="utf-8")


def render_markdown(rows: list[dict], failures: list[dict], weak: list[dict]) -> str:
    L = [
        "# 墨隐 InkVeil 第三方依赖与许可证清单",
        "",
        "> 由 `scripts/licenses.py` 自动生成（cargo / npm / python 三个生态，按 SPDX 表达式解析）。",
        "> 策略：GPL / AGPL / LGPL / SSPL / NC 判失败；“未声明/无法解析”判失败；"
        "MPL 等弱 copyleft 单独列出，且必须出现在 `THIRD-PARTY-NOTICES.md`。",
        "> Python 部分以 `sidecar/requirements*.lock` 为准；表中“范围”：runtime 随应用分发 / build 与 dev 仅构建期使用 / first-party 本仓库自有。",
        "",
        f"## 摘要\n\n- 依赖总数：{len(rows)}（"
        + "，".join(f"{e} {sum(1 for r in rows if r['ecosystem'] == e)}" for e in ("cargo", "npm", "python"))
        + f"）\n- 不合规：{len(failures)}\n- 弱 copyleft（MPL 等）：{len(weak)}\n",
    ]
    if failures:
        L += ["## 不合规项", "", "| 生态 | 包 | 版本 | 许可证 | 原因 |", "|---|---|---|---|---|"]
        for r in failures:
            L.append(f"| {r['ecosystem']} | {r['name']} | {r['version']} | {r['expr']} | {LEVEL_NAME[r['level']]} {r.get('reason','')} |")
        L.append("")
    for eco in ("cargo", "npm", "python"):
        sub = [r for r in rows if r["ecosystem"] == eco]
        if not sub:
            continue
        L += [f"## {eco} 依赖（{len(sub)}）", "", "| 包 | 版本 | 许可证 | 范围 |", "|---|---|---|---|"]
        for r in sorted(sub, key=lambda x: (x["name"].lower(), x["version"])):
            mark = "（人工核实）" if r.get("override_reason") else ""
            L.append(f"| {r['name']} | {r['version']} | {r['expr']}{mark} | {r['scope']} |")
        L.append("")
    return "\n".join(L)


# ---- CycloneDX ----------------------------------------------------------------
def purl(r: dict) -> str:
    name, ver = r["name"], r["version"]
    if r["ecosystem"] == "npm":
        name = name.replace("@", "%40", 1) if name.startswith("@") else name
        return f"pkg:npm/{name}@{ver}"
    if r["ecosystem"] == "cargo":
        return f"pkg:cargo/{name}@{ver}"
    return f"pkg:pypi/{name}@{ver}"


def component(r: dict) -> dict:
    c = {
        "type": "library",
        "bom-ref": purl(r),
        "name": r["name"],
        "version": r["version"],
        "purl": purl(r),
        "scope": "required" if r["scope"] in ("runtime", "first-party") else "optional",
        "properties": [{"name": "anonkit:scope", "value": r["scope"]}],
    }
    if r["level"] < LEVEL_UNKNOWN:
        c["licenses"] = [{"expression": r["expr"]}]
    else:
        c["licenses"] = [{"license": {"name": r["expr"]}}]
    if r.get("checksum"):
        c["hashes"] = [{"alg": "SHA-256", "content": r["checksum"]}]
    elif r.get("integrity") and r["integrity"].startswith("sha512-"):
        try:
            c["hashes"] = [{"alg": "SHA-512", "content": base64.b64decode(r["integrity"][7:]).hex()}]
        except Exception:  # noqa: BLE001
            pass
    return c


def build_bom(name: str, rows: list[dict], app_version: str) -> dict:
    ts = os.environ.get("SOURCE_DATE_EPOCH")
    now = (datetime.datetime.utcfromtimestamp(int(ts)) if ts else datetime.datetime.utcnow()).strftime("%Y-%m-%dT%H:%M:%SZ")
    comps = [component(r) for r in rows]
    serial = uuid.uuid5(uuid.NAMESPACE_URL, name + "|" + "|".join(sorted(c["purl"] for c in comps)))
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{serial}",
        "version": 1,
        "metadata": {
            "timestamp": now,
            "tools": {"components": [{"type": "application", "name": "anonkit-licenses.py"}]},
            "component": {"type": "application", "name": f"anonkit-{name}", "version": app_version},
        },
        "components": comps,
    }


def write_sboms(rows: list[dict], out_dir: pathlib.Path) -> list[pathlib.Path]:
    app_version = json.loads((ROOT / "package.json").read_text(encoding="utf-8")).get("version", "0")
    out_dir.mkdir(parents=True, exist_ok=True)
    paths = []
    groups = {e: [r for r in rows if r["ecosystem"] == e] for e in ("cargo", "npm", "python")}
    groups = {k: v for k, v in groups.items() if v}
    for eco, sub in groups.items():
        p = out_dir / f"anonkit-{eco}.cdx.json"
        p.write_text(json.dumps(build_bom(eco, sub, app_version), indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        paths.append(p)
    p = out_dir / "anonkit-all.cdx.json"
    p.write_text(json.dumps(build_bom("all", rows, app_version), indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    paths.append(p)
    return paths


# ---- main ---------------------------------------------------------------------
def main() -> int:
    ap = argparse.ArgumentParser(description="墨隐 InkVeil 许可证合规检查 / 清单 / SBOM")
    ap.add_argument("--check-only", action="store_true", help="仅检查，不写 DEPENDENCIES.md")
    ap.add_argument("--venv", type=pathlib.Path, default=ROOT / "sidecar" / ".venv", help="已按锁文件安装依赖的 venv")
    ap.add_argument("--only", default="cargo,npm,python", help="逗号分隔的生态子集")
    ap.add_argument("--sbom-dir", type=pathlib.Path, help="输出 CycloneDX SBOM 的目录")
    ap.add_argument("--write-notices", action="store_true", help="更新 THIRD-PARTY-NOTICES.md 的 MPL 自动清单块")
    args = ap.parse_args()

    eco = {e.strip() for e in args.only.split(",") if e.strip()}
    rows: list[dict] = []
    if "cargo" in eco:
        rows += cargo_packages()
    if "npm" in eco:
        rows += npm_packages()
    if "python" in eco:
        rows += python_packages(args.venv if args.venv.is_absolute() else ROOT / args.venv)
    apply_overrides(rows, load_overrides())
    evaluate_all(rows)

    # 构建期工具（scope=build）经覆盖表显式豁免：不进入分发产物，但仍在输出中列出供人工复核
    exempt = [r for r in rows if r["level"] >= LEVEL_FORBIDDEN and r["scope"] == "build" and r.get("exempt_build_tool")]
    failures = [r for r in rows if r["level"] >= LEVEL_FORBIDDEN and r not in exempt]
    weak = [r for r in rows if r["level"] == LEVEL_WEAK]

    counts = {e: sum(1 for r in rows if r["ecosystem"] == e) for e in ("cargo", "npm", "python")}
    print("共收集 %d 个依赖（%s）" % (len(rows), " / ".join(f"{k} {v}" for k, v in counts.items())))

    if args.write_notices and weak:
        write_notices_block(weak)
        print(f"已更新 {NOTICES_PATH.name} 的 MPL 自动清单块（{len(weak)} 项）")

    notices = mpl_names_in_notices()
    missing_notice = sorted({(r["ecosystem"], r["name"]) for r in weak if r["name"].lower() not in notices})

    ok = True
    if failures:
        ok = False
        print(f"\n✗ 不合规依赖 {len(failures)} 项（GPL/AGPL/LGPL/SSPL/NC 或 未声明/无法解析）：")
        for r in failures:
            print(f"  ✗ [{r['ecosystem']}] {r['name']} {r['version']}: {r['expr']}  ← {LEVEL_NAME[r['level']]}{(' ' + r['reason']) if r.get('reason') else ''}")
    if exempt:
        print(f"\n○ 构建期工具豁免 {len(exempt)} 项（不随应用分发，需法务/负责人确认，理由见 license-overrides.json）：")
        for r in exempt:
            print(f"  ○ [{r['ecosystem']}] {r['name']} {r['version']}: {r['expr']}")
    if weak:
        names = sorted({(r["ecosystem"], r["name"]) for r in weak})
        print(f"\n△ 弱 copyleft（MPL 等）{len(names)} 项（须出现在 THIRD-PARTY-NOTICES.md）：")
        for e, n in names:
            print(f"  △ [{e}] {n}")
    if missing_notice:
        ok = False
        print(f"\n✗ 以下 MPL 依赖未出现在 {NOTICES_PATH.name}（可运行 --write-notices）：")
        for e, n in missing_notice:
            print(f"  ✗ [{e}] {n}")

    if args.sbom_dir:
        for p in write_sboms(rows, args.sbom_dir if args.sbom_dir.is_absolute() else ROOT / args.sbom_dir):
            print(f"SBOM: {p.relative_to(ROOT) if str(p).startswith(str(ROOT)) else p}")

    if not args.check_only:
        (ROOT / "DEPENDENCIES.md").write_text(render_markdown(rows, failures, weak), encoding="utf-8")
        print(f"已生成 {ROOT / 'DEPENDENCIES.md'}")

    print("\n合规检查" + ("通过" if ok else "失败"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
