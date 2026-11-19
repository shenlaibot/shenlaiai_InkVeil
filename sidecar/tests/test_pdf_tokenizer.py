"""回归：PDF 内容流 tokenizer 必须终止且不破坏十六进制串 / 字典 / 内联图片。

此前遇到 <hex>、<< >>、孤立的 ] ) { } 时 i 不前进，导致无限循环并无限占用内存。
"""

import signal

import pytest

from redact import serialize, tokenize


class _Timeout(Exception):
    pass


def _run_with_timeout(fn, seconds=3):
    def handler(signum, frame):
        raise _Timeout()

    old = signal.signal(signal.SIGALRM, handler)
    signal.alarm(seconds)
    try:
        return fn()
    finally:
        signal.alarm(0)
        signal.signal(signal.SIGALRM, old)


CASES = {
    "hex_string": b"BT /F1 12 Tf <00310032> Tj ET",
    "hex_in_tj_array": b"BT /F1 12 Tf [<0031> -10 <0032>] TJ ET",
    "dict_bdc": b"/P << /MCID 0 >> BDC BT (a) Tj ET EMC",
    "nested_dict_with_string": b"/Span << /A << /B (x >> y) >> >> BDC EMC",
    "stray_delims": b"1 0 0 1 0 0 cm ] ) { } > q Q",
    "inline_image": b"q BI /W 1 /H 1 /BPC 8 /CS /G ID \x00\x28\x29\x3c\x3e EI Q BT (z) Tj ET",
    "unterminated_hex": b"BT <0031",
    "unterminated_dict": b"/P << /MCID 0",
}


@pytest.mark.parametrize("name", list(CASES))
def test_tokenize_terminates(name):
    toks = _run_with_timeout(lambda: tokenize(CASES[name]))
    assert len(toks) < 200  # 不应出现无界增长
    _run_with_timeout(lambda: serialize(toks))


def test_hex_and_inline_image_preserved():
    out = serialize(tokenize(CASES["hex_string"]))
    assert b"<00310032>" in out
    out = serialize(tokenize(CASES["inline_image"]))
    assert b"ID \x00\x28\x29\x3c\x3e EI" in out  # 二进制数据原样保留
    out = serialize(tokenize(CASES["hex_in_tj_array"]))
    assert b"<0031>" in out and b"<0032>" in out
