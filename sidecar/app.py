"""墨隐 InkVeil Python sidecar — FastAPI 服务。

本地文档脱敏服务：/parse 解析文档（PDF/DOCX/TXT/MD → 全文 + span 流 + 结构），
/redact 按全文字符偏移执行脱敏写回。Rust 主进程通过本地 HTTP 调用，
只传本地文件路径，数据不出本机。
"""

from __future__ import annotations

import argparse
import hmac
import os
from typing import Optional

import uvicorn
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import JSONResponse
from pydantic import BaseModel, Field

from parsers import detect_format, parse_file
from redact import redact_file
from ner import ModelNotFoundError, get_engine

APP_VERSION = "0.2.0"

app = FastAPI(title="InkVeil Sidecar", version=APP_VERSION)

# 认证 token：由 Rust 主进程通过 `--token` 注入。非 None 时所有端点要求
# `Authorization: Bearer <token>`。用于防本机其它进程扫到端口后直接调用
# /parse（读任意文件）/redact（写任意目录）。
_TOKEN: Optional[str] = None


@app.middleware("http")
async def auth_middleware(request: Request, call_next):
    if _TOKEN:
        auth = request.headers.get("authorization", "")
        # 常量时间比较，避免逐字节计时泄露 token
        if not hmac.compare_digest(auth.encode("utf-8"), f"Bearer {_TOKEN}".encode("utf-8")):
            return JSONResponse(status_code=401, content={"detail": "unauthorized"})
    return await call_next(request)


# ---------------------------------------------------------------------------
# 请求/响应模型
# ---------------------------------------------------------------------------


class ParseRequest(BaseModel):
    doc_id: Optional[str] = None
    path: str = Field(..., description="本地文件绝对路径（PDF/DOCX/TXT/MD）")


class Replacement(BaseModel):
    start: int = Field(..., description="全文字符起始偏移（含）")
    end: int = Field(..., description="全文字符结束偏移（不含）")
    replacement: str = Field(..., description="替换文本（掩码）")


class RedactRequest(BaseModel):
    input_path: str = Field(..., description="输入文件绝对路径")
    replacements: list[Replacement] = Field(..., description="按全文偏移的替换列表（互不重叠）")
    output_dir: str = Field(..., description="输出目录（脱敏文件写入此处，原文件不改）")
    keep_name: bool = Field(False, description="沿用原文件名（默认 False：原名常含个人信息）")
    verify_needles: list[str] = Field(
        default_factory=list, description="导出后复检：这些已确认原文不得再出现在输出文件中"
    )


class NERRequest(BaseModel):
    text: str = Field(..., description="待识别文本（UTF-8 字符偏移）")
    threshold: float = Field(0.5, ge=0.0, le=1.0, description="实体置信度阈值")
    model_path: Optional[str] = Field(None, description="ONNX 模型目录（默认 sidecar/models/ner/）")


class RestoreRequest(BaseModel):
    input_path: str = Field(..., description="待还原的 docx/xlsx 绝对路径")
    mapping: dict[str, str] = Field(..., description="占位符 → 原文（仅在内存中使用，不落盘）")
    output_dir: str = Field(..., description="输出目录（另存新文件，输入文件不改）")


# ---------------------------------------------------------------------------
# 端点
# ---------------------------------------------------------------------------


@app.get("/health")
def health() -> dict:
    return {"status": "ok", "version": APP_VERSION}


def _friendly_error(path: str, e: Exception, action: str) -> str:
    """把解析器内部异常转成面向用户的说明；不回传异常 repr（可能带内部路径/内容片段）。"""
    import zipfile

    try:
        with open(path, "rb") as f:
            head = f.read(8)
    except Exception:
        head = b""
    if head.startswith(b"\xd0\xcf\x11\xe0"):
        return f"{action}失败：文件已加密，或是旧版 .doc/.xls 格式，请先另存为 .docx/.xlsx 后再处理"
    if isinstance(e, zipfile.BadZipFile) or head[:2] != b"PK" and path.lower().endswith((".docx", ".xlsx")):
        return f"{action}失败：文件已损坏，或不是有效的 Office 文档（也可能已加密）"
    if head.startswith(b"%PDF") or path.lower().endswith(".pdf"):
        return f"{action}失败：PDF 无法打开（可能已损坏或需要密码）"
    return f"{action}失败：文件无法读取（{type(e).__name__}）"


@app.post("/parse")
def parse(req: ParseRequest) -> dict:
    """解析文档：返回全文文本、span 流（基于全文偏移）与页面/块级结构。"""
    try:
        result = parse_file(req.path, doc_id=req.doc_id)
    except FileNotFoundError as e:
        raise HTTPException(status_code=404, detail=str(e))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:  # 解析器内部错误（损坏文件等）
        raise HTTPException(status_code=500, detail=_friendly_error(req.path, e, "解析"))
    return result.to_dict()


def _sha256_file(path: str) -> str:
    import hashlib

    h = hashlib.sha256()
    try:
        with open(path, "rb") as f:
            for chunk in iter(lambda: f.read(1 << 20), b""):
                h.update(chunk)
    except OSError:
        return ""
    return h.hexdigest()


@app.post("/redact")
def redact(req: RedactRequest) -> dict:
    """按全文字符偏移执行脱敏写回，输出到 output_dir，原文件绝不修改。"""
    try:
        result = redact_file(
            req.input_path,
            [r.model_dump() for r in req.replacements],
            req.output_dir,
            keep_name=req.keep_name,
        )
        # 覆盖范围与指纹：只含部位/类型/数量与哈希，绝不含原文
        try:
            from doc_coverage import uncovered_detail
            from parsers import detect_format as _df

            result["uncovered"] = uncovered_detail(req.input_path, _df(req.input_path))
        except Exception:
            result["uncovered"] = []
        result["input_sha256"] = _sha256_file(req.input_path)
        result["output_sha256"] = _sha256_file(result["output_path"])
        result["sidecar_version"] = APP_VERSION
        if req.verify_needles:
            from parsers import detect_format
            from verifier import find_residue

            out = result["output_path"]
            # 复检失败按“无法确认”处理：返回 verified=False，调用方不得当作通过
            try:
                result["residual"] = find_residue(out, detect_format(out), req.verify_needles)
                result["verified"] = True
            except Exception:
                result["residual"] = []
                result["verified"] = False
    except FileNotFoundError as e:
        raise HTTPException(status_code=404, detail=str(e))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail=_friendly_error(req.input_path, e, "脱敏"))
    return {"ok": True, "files": [result]}


@app.post("/restore")
def restore(req: RestoreRequest) -> dict:
    """文档还原：按映射表把 docx/xlsx 里的占位符（含改写形态）还原为原文，另存新文件。"""
    from restore_doc import restore_file

    try:
        result = restore_file(req.input_path, req.mapping, req.output_dir)
    except FileNotFoundError as e:
        raise HTTPException(status_code=404, detail=str(e))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail=_friendly_error(req.input_path, e, "还原"))
    return {"ok": True, **result}


@app.post("/ner")
def ner(req: NERRequest) -> dict:
    """中文 NER 检测层兜底：识别人名/机构/地址（ONNX 本地推理，零外传）。

    模型懒加载（首次请求时加载并全局缓存）；模型缺失时返回 HTTP 200 +
    {"ok": false, "error": "model not found", "model_loaded": false}。
    """
    try:
        if not req.text:  # 空文本无需加载模型
            return {"ok": True, "entities": [], "model_loaded": False, "error": None}
        engine = get_engine(req.model_path)
        entities = engine.predict(req.text, threshold=req.threshold)
    except ModelNotFoundError:
        return {"ok": False, "error": "model not found", "model_loaded": False, "entities": []}
    except Exception as e:
        return {"ok": False, "error": f"NER 推理失败（{type(e).__name__}）", "model_loaded": False, "entities": []}
    return {"ok": True, "entities": entities, "model_loaded": True, "error": None}


def main() -> None:
    global _TOKEN

    parser = argparse.ArgumentParser(description="墨隐 InkVeil sidecar server")
    parser.add_argument("--host", default="127.0.0.1")
    # 0 表示由 uvicorn 随机绑定可用端口，sidecar 管理器会从启动日志解析实际端口。
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument(
        "--token",
        default=None,
        help="Rust 主进程注入的认证 token（缺省则不要求认证，仅用于开发/调试）",
    )
    args = parser.parse_args()
    # 发布期必须带 token：否则本机任意进程都能调用 /parse（读任意文件）与 /redact（写任意目录）。
    # 仅开发/测试可显式设置 ANONKIT_ALLOW_NO_TOKEN=1 放行。
    if not args.token and os.environ.get("ANONKIT_ALLOW_NO_TOKEN") != "1":
        parser.error("缺少 --token：sidecar 默认拒绝无认证启动（开发调试请设置 ANONKIT_ALLOW_NO_TOKEN=1）")

    _TOKEN = args.token

    uvicorn.run(app, host=args.host, port=args.port, log_level="info")


if __name__ == "__main__":
    main()
