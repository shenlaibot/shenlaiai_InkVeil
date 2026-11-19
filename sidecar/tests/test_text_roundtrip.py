"""TXT 往返：保留原编码、BOM 与换行风格（含 Windows \\r\\n）。"""

import os
import tempfile

from parsers import read_text_file
from redact import redact_text


def _roundtrip(raw: bytes, needle: str, repl: str):
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.txt")
        dst = os.path.join(d, "b.txt")
        with open(src, "wb") as f:
            f.write(raw)
        text, _ = read_text_file(src)
        s = text.index(needle)
        redact_text(src, [(s, s + len(needle), repl)], dst)
        with open(dst, "rb") as f:
            return f.read()


def test_crlf_preserved():
    out = _roundtrip("第一行\r\n张三在此\r\n第三行\r\n".encode("utf-8"), "张三", "[PER_1]")
    assert out == "第一行\r\n[PER_1]在此\r\n第三行\r\n".encode("utf-8")


def test_utf8_bom_preserved():
    out = _roundtrip(b"\xef\xbb\xbf" + "张三".encode("utf-8"), "张三", "X")
    assert out == b"\xef\xbb\xbf" + b"X"


def test_gbk_preserved():
    out = _roundtrip("联系人张三".encode("gbk"), "张三", "李四")
    assert out == "联系人李四".encode("gbk")


def test_utf16_preserved():
    out = _roundtrip("张三".encode("utf-16"), "张三", "李四")
    assert out.startswith((b"\xff\xfe", b"\xfe\xff"))
    assert out.decode("utf-16") == "李四"
