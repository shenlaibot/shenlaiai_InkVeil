"""环境相关测试的跳过规则：缺少可选依赖/模型时跳过，而不是失败。

- NER 评测/长文本用例依赖已下载的 ONNX 模型（sidecar/models/ner）；
- 部分 PDF 黄金用例用 reportlab 生成样本，而 reportlab 需要 Python ≥ 3.10。
这样 CI 缺少模型或 Python 较旧时测试结果是“跳过”而非“红”，真正的回归仍会失败。
"""

import importlib
import os

import pytest


def _has(mod: str) -> bool:
    try:
        importlib.import_module(mod)
        return True
    except Exception:
        return False


def pytest_collection_modifyitems(config, items):
    from ner import DEFAULT_MODEL_DIR

    model_ok = os.path.isfile(os.path.join(DEFAULT_MODEL_DIR, "model.onnx"))
    reportlab_ok = _has("reportlab")
    pdf_needs_reportlab = (
        "test_pdf_parse_and_redact",
        "test_concurrent_pdf_parse_and_redact",
        "test_pdf_full_flow_leaves_no_temp_artifacts",
        "test_golden_redact_leaves_no_residue[pdf]",
        "test_golden_parse_spans_consistent",
    )
    for item in items:
        if "test_ner.py" in item.nodeid and not model_ok:
            item.add_marker(pytest.mark.skip(reason="NER 模型未安装（sidecar/models/ner）"))
        if not reportlab_ok and any(k in item.nodeid for k in pdf_needs_reportlab):
            item.add_marker(pytest.mark.skip(reason="需要 reportlab（要求 Python ≥ 3.10）"))
