"""墨隐 InkVeil 中文 NER 推理引擎（ONNX BERT，CLUENER 标签集）。

检测层兜底：识别 人名 / 机构 / 地址 三类实体，返回全文字符偏移（可直接用于脱敏）。
模型: protectai/gyr66-bert-base-chinese-finetuned-ner-onnx（Apache-2.0，本地加载）。
运行期零外传：模型加载完成后推理不发起任何网络请求。
"""

from __future__ import annotations

import json
import logging
import os
import threading
import time
from typing import Optional

# 注意：numpy / onnxruntime / tokenizers 均为延迟导入（首次 NER 推理时加载），
# 使 sidecar 空闲内存保持低位（spec：空闲内存 <120MB 目标）。

logger = logging.getLogger(__name__)

# 默认模型目录：<sidecar>/models/ner/（model.onnx + tokenizer.json + config.json）
DEFAULT_MODEL_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "models", "ner")

# BERT 上下文上限（含 CLS/SEP）
MAX_LEN = 512
# 相邻窗口重叠 token 数：保证跨块实体在下一窗口完整重现，且右缘截断不丢实体
OVERLAP = 128
# 单窗口真实 token 上限
_WINDOW = MAX_LEN - 2

# CLUENER 标签 → 中文统一标签。
# 只保留 人名/机构/地址 三类；government 视为机构；其余（position/movie/book/
# scene/mobile/email/game/QQ/vx 等）不输出（可选增强留给正则层）。
CLUENER_TO_UNIFIED = {
    "name": "人名",
    "company": "机构",
    "organization": "机构",
    "enterprise": "机构",
    "government": "机构",  # 政府机构并入机构
    "address": "地址",
}


class ModelNotFoundError(FileNotFoundError):
    """模型目录缺失或文件不完整。"""


def _softmax(x, axis: int = -1):
    """numpy softmax（lazy：numpy 延迟到推理路径才加载，减小空闲内存）。"""
    import numpy as np

    x = x - np.max(x, axis=axis, keepdims=True)
    e = np.exp(x)
    return e / np.sum(e, axis=axis, keepdims=True)


def _split_tag(label: str) -> tuple[Optional[str], str]:
    """'B-name' -> ('B', 'name')；'O' -> (None, 'O')。"""
    if label == "O" or "-" not in label:
        return (None, label)
    tag, base = label.split("-", 1)
    return (tag, base)


def load_tokenizer(tok_path: str):
    """加载 tokenizer.json 并关闭其自带的截断与填充。

    上游 tokenizer.json 带 truncation(max_length=512)：不关掉的话 encode() 只返回
    前 512 个 token，后面的滑窗逻辑形同虚设，文档 99% 以上的正文从未进入模型。
    """
    from tokenizers import Tokenizer

    tok = Tokenizer.from_file(tok_path)
    tok.no_truncation()
    tok.no_padding()
    return tok


class NEREngine:
    """ONNX 中文 NER 推理引擎（懒加载，线程安全，进程内复用）。"""

    def __init__(self, model_dir: str = DEFAULT_MODEL_DIR):
        self.model_dir = model_dir
        self._lock = threading.Lock()
        self._sess = None
        self._tok = None
        self._id2label: dict[int, str] = {}
        self._input_names: list[str] = []
        self._cls_id: int = 101
        self._sep_id: int = 102

    # ------------------------------------------------------------------
    # 加载
    # ------------------------------------------------------------------

    @property
    def is_loaded(self) -> bool:
        return self._sess is not None

    def _ensure_loaded(self) -> None:
        """首次调用时加载模型（幂等；失败可重试，不污染缓存）。"""
        if self._sess is not None:
            return
        onnx_path = os.path.join(self.model_dir, "model.onnx")
        tok_path = os.path.join(self.model_dir, "tokenizer.json")
        cfg_path = os.path.join(self.model_dir, "config.json")
        missing = [p for p in (onnx_path, tok_path, cfg_path) if not os.path.isfile(p)]
        if missing:
            raise ModelNotFoundError(
                f"model not found: {self.model_dir}（缺少 {', '.join(os.path.basename(m) for m in missing)}）"
            )
        with self._lock:
            if self._sess is not None:
                return
            t0 = time.perf_counter()
            import onnxruntime as ort
            sess = ort.InferenceSession(onnx_path, providers=["CPUExecutionProvider"])
            tok = load_tokenizer(tok_path)
            with open(cfg_path, encoding="utf-8") as f:
                cfg = json.load(f)
            id2label = {int(k): v for k, v in (cfg.get("id2label") or {}).items()}
            if not id2label:
                raise ValueError(f"config.json 缺少 id2label: {cfg_path}")
            self._sess = sess
            self._tok = tok
            self._id2label = id2label
            self._input_names = [i.name for i in sess.get_inputs()]
            self._cls_id = tok.token_to_id("[CLS]") or 101
            self._sep_id = tok.token_to_id("[SEP]") or 102
            logger.info("NER 模型加载完成: %s (%.1fs)", self.model_dir, time.perf_counter() - t0)

    # ------------------------------------------------------------------
    # 推理
    # ------------------------------------------------------------------

    def predict(self, text: str, threshold: float = 0.5) -> list[dict]:
        """识别实体。

        返回 list[dict]：
          {"label": 统一标签, "model_label": 模型原始标签(去 B/I 前缀),
           "start": 字符起始(含), "end": 字符结束(不含), "text": 原文片段,
           "confidence": 实体置信度(各 token 概率均值)}
        长文本按 512 token 滑窗 + 128 重叠分块；跨块重复实体自动去重。
        """
        if not text:
            return []
        self._ensure_loaded()
        import numpy as np  # noqa: F401  lazy：仅在推理路径加载

        enc = self._tok.encode(text, add_special_tokens=False)
        ids = list(enc.ids)
        offsets = list(enc.offsets)
        n = len(ids)
        if n == 0:
            return []

        stride = _WINDOW - OVERLAP  # 每窗前移 382 token，末窗覆盖到文本结尾
        starts = list(range(0, n, stride))
        raw: list[tuple[int, int, str, float]] = []  # (start, end, model_label, conf)
        for idx, s in enumerate(starts):
            wids = ids[s : s + _WINDOW]
            woffs = offsets[s : s + _WINDOW]
            is_last = idx == len(starts) - 1
            raw.extend(self._predict_window(wids, woffs, is_last))

        # 跨块/跨窗去重：同一 (start, end, model_label) 保留置信度最高者
        best: dict[tuple[int, int, str], float] = {}
        for s, e, lab, conf in raw:
            key = (s, e, lab)
            if e > s and (key not in best or conf > best[key]):
                best[key] = conf

        out = []
        for (s, e, lab), conf in best.items():
            unified = CLUENER_TO_UNIFIED.get(lab)
            if unified is None or conf < threshold:
                continue
            out.append(
                {
                    "label": unified,
                    "model_label": lab,
                    "start": s,
                    "end": e,
                    "text": text[s:e],
                    "confidence": round(float(conf), 4),
                }
            )
        out.sort(key=lambda x: (x["start"], x["end"]))
        return out

    def _predict_window(self, wids: list[int], woffs: list[tuple[int, int]], is_last: bool) -> list[tuple[int, int, str, float]]:
        """推理单个窗口，返回窗口内的 (start_char, end_char, model_label, confidence)。

        非末尾窗口丢弃「触及右缘」的实体：该实体必然在下一窗口的 overlap 区域完整重现，
        避免因截断产生残缺实体。
        """
        import numpy as np  # noqa: F401  lazy：仅在推理路径加载

        input_ids = [self._cls_id] + wids + [self._sep_id]
        length = len(input_ids)
        feeds = {
            "input_ids": np.asarray([input_ids], dtype=np.int64),
            "attention_mask": np.asarray([[1] * length], dtype=np.int64),
        }
        if "token_type_ids" in self._input_names:
            feeds["token_type_ids"] = np.asarray([np.zeros(length, dtype=np.int64)])
        logits = self._sess.run(None, feeds)[0]  # (1, length, num_labels)
        probs = _softmax(logits[0], axis=-1)
        preds = probs.argmax(axis=-1)

        ents: list[tuple[int, int, str, int, list[float]]] = []  # (start, end, label, last_j, confs)
        cur: Optional[dict] = None

        def flush() -> None:
            nonlocal cur
            if cur is not None and cur["end"] > cur["start"]:
                confs = cur["confs"]
                ents.append((cur["start"], cur["end"], cur["label"], cur["last_j"], confs))
            cur = None

        n_real = len(woffs)
        for j in range(n_real):
            cs, ce = woffs[j]
            if ce <= cs:  # 空 token（padding 等），跳过
                flush()
                continue
            i = j + 1  # 输入位置 = CLS + j
            lab = self._id2label.get(int(preds[i]), "O")
            conf = float(probs[i, int(preds[i])])
            tag, base = _split_tag(lab)
            if tag is None:  # O 或未知标签
                flush()
            elif tag == "B":
                flush()
                cur = {"start": cs, "end": ce, "label": base, "last_j": j, "confs": [conf]}
            elif tag == "I":
                if cur is not None and cur["label"] == base:
                    cur["end"] = ce
                    cur["last_j"] = j
                    cur["confs"].append(conf)
                else:  # 容错：孤立 I 视为单 token 实体
                    flush()
                    cur = {"start": cs, "end": ce, "label": base, "last_j": j, "confs": [conf]}
            elif tag == "S":
                flush()
                ents.append((cs, ce, base, j, [conf]))
            elif tag == "E":
                if cur is not None and cur["label"] == base:
                    cur["end"] = ce
                    cur["last_j"] = j
                    cur["confs"].append(conf)
                    flush()
                else:
                    flush()
                    ents.append((cs, ce, base, j, [conf]))
            else:
                flush()
        flush()

        if not is_last:
            last_real = n_real - 1
            ents = [e for e in ents if e[3] < last_real]
        return [(s, e, lab, float(np.mean(confs))) for s, e, lab, _j, confs in ents]


# ---------------------------------------------------------------------------
# 进程内全局缓存（懒加载：首次请求时才真正加载模型）
# ---------------------------------------------------------------------------

_ENGINE_CACHE: dict[str, NEREngine] = {}
_ENGINE_LOCK = threading.Lock()


def get_engine(model_dir: Optional[str] = None) -> NEREngine:
    """按模型目录获取全局缓存的引擎（目录不同则分别缓存）。"""
    model_dir = os.path.abspath(model_dir or DEFAULT_MODEL_DIR)
    with _ENGINE_LOCK:
        eng = _ENGINE_CACHE.get(model_dir)
        if eng is None:
            eng = NEREngine(model_dir)
            _ENGINE_CACHE[model_dir] = eng
        return eng


def infer(text: str, threshold: float = 0.5, model_dir: Optional[str] = None) -> list[dict]:
    """便捷入口：等价于 get_engine(model_dir).predict(text, threshold)。"""
    return get_engine(model_dir).predict(text, threshold=threshold)
