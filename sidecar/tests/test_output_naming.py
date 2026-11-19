"""输出文件：不含原名、不覆盖已有文件、原子写入（失败不留半成品）。"""

import os
import tempfile

import pytest

from redact import redact_file


def _src(d, name="张三-劳动合同.txt", content="联系人张三，电话13800138000"):
    p = os.path.join(d, name)
    with open(p, "w", encoding="utf-8") as f:
        f.write(content)
    return p


def test_output_name_has_no_original_name_and_no_overwrite():
    with tempfile.TemporaryDirectory() as d:
        src = _src(d)
        out_dir = os.path.join(d, "out")
        r1 = redact_file(src, [{"start": 3, "end": 5, "replacement": "X"}], out_dir)
        r2 = redact_file(src, [{"start": 3, "end": 5, "replacement": "Y"}], out_dir)
        assert "张三" not in os.path.basename(r1["output_path"])
        assert r1["output_path"] != r2["output_path"]  # 第二次不覆盖第一次
        assert open(r1["output_path"], encoding="utf-8").read().startswith("联系人X")
        assert open(r2["output_path"], encoding="utf-8").read().startswith("联系人Y")


def test_keep_name_opt_in():
    with tempfile.TemporaryDirectory() as d:
        src = _src(d)
        r = redact_file(src, [], os.path.join(d, "out"), keep_name=True)
        assert os.path.basename(r["output_path"]) == "张三-劳动合同_脱敏.txt"


def test_failure_leaves_no_partial_output():
    with tempfile.TemporaryDirectory() as d:
        src = _src(d)
        out_dir = os.path.join(d, "out")
        with pytest.raises(ValueError):
            redact_file(
                src,
                [{"start": 0, "end": 5, "replacement": "a"}, {"start": 3, "end": 6, "replacement": "b"}],
                out_dir,
            )
        assert not os.path.isdir(out_dir) or os.listdir(out_dir) == []
