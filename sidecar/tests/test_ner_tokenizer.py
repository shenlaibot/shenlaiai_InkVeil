"""回归：tokenizer 不得截断长文本（上游 tokenizer.json 自带 max_length=512 截断）。

不依赖 ONNX 模型：用临时构造的、带截断设置的小 tokenizer 验证 load_tokenizer。
"""

import os
import tempfile

from tokenizers import Tokenizer, models, pre_tokenizers

from ner import load_tokenizer


def _make_truncating_tokenizer(path: str) -> None:
    vocab = {"[UNK]": 0, "a": 1}
    tok = Tokenizer(models.WordLevel(vocab, unk_token="[UNK]"))
    tok.pre_tokenizer = pre_tokenizers.Split("", "isolated")
    tok.enable_truncation(max_length=512)
    tok.save(path)


def test_load_tokenizer_disables_truncation():
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, "tokenizer.json")
        _make_truncating_tokenizer(path)
        # 前提：不经处理时确实被截断（防止测试本身失效）
        raw = Tokenizer.from_file(path)
        assert len(raw.encode("a" * 5000, add_special_tokens=False).ids) == 512

        tok = load_tokenizer(path)
        enc = tok.encode("a" * 5000, add_special_tokens=False)
        assert len(enc.ids) == 5000
        assert enc.offsets[-1][1] == 5000  # 覆盖到文本末尾
