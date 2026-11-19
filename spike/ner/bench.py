#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
spike/ner/bench.py — 本地中文文档脱敏工具 · NER 层技术选型评测

评测对象: spike/ner/corpus.json (3 段中文合同/协议 + 人工标注)
实体类型: PER(人名) / ORG_FULL(机构全称) / ORG_SHORT(机构简称)
指标:     span 级 precision / recall / F1（按类型独立），单条文本平均推理耗时(CPU)

用法:
    .venv/bin/python bench.py                 # 只跑 jieba 基线与词典增强
    .venv/bin/python bench.py --fasthan       # 加入 fastHan
    .venv/bin/python bench.py --ltp           # 加入 LTP
    .venv/bin/python bench.py --onnx          # 加入 ONNX BERT NER
    .venv/bin/python bench.py --all           # 全部
"""
import argparse
import json
import os
import re
import sys
import time
import traceback

HERE = os.path.dirname(os.path.abspath(__file__))
CORPUS = os.path.join(HERE, "corpus.json")

TYPES = ("PER", "ORG_FULL", "ORG_SHORT")

# ---------------------------------------------------------------------------
# 语料与评测
# ---------------------------------------------------------------------------

def load_corpus():
    with open(CORPUS, encoding="utf-8") as f:
        return json.load(f)["docs"]


def span_metrics(gold, pred):
    """span 级 P/R/F1。完全匹配 (start,end)；每个 pred 至多匹配一个 gold，反之亦然。
    允许不同类型间重叠（全称包含简称时，模型一个 ORG 预测可分别命中两类 gold）。
    返回 (p, r, f1, tp, fp, fn)
    """
    pred = set(pred)
    gold = set(gold)
    tp = len(pred & gold)
    fp = len(pred) - tp
    fn = len(gold) - tp
    p = tp / (tp + fp) if (tp + fp) else 0.0
    r = tp / (tp + fn) if (tp + fn) else 0.0
    f1 = 2 * p * r / (p + r) if (p + r) else 0.0
    return p, r, f1, tp, fp, fn


def evaluate(docs, predict_fn):
    """predict_fn(text) -> list[(start, end, type)]
    除 PER/ORG_FULL/ORG_SHORT 外，额外计算 ORG_ALL（ORG_FULL∪ORG_SHORT 合并），
    反映"识别出是机构"即可的产品真实效果（脱敏时全称/简称都会打码）。
    """
    per_type = {t: {"tp": 0, "fp": 0, "fn": 0, "n": 0} for t in TYPES + ("ORG_ALL",)}
    timings = []
    for d in docs:
        gold = {t: set() for t in TYPES + ("ORG_ALL",)}
        for e in d["entities"]:
            gold[e["type"]].add((e["start"], e["end"]))
        gold["ORG_ALL"] = gold["ORG_FULL"] | gold["ORG_SHORT"]
        t0 = time.perf_counter()
        preds = predict_fn(d["text"])
        timings.append(time.perf_counter() - t0)
        by_type = {t: [] for t in TYPES}
        for s, e, t in preds:
            if t in TYPES and e > s:
                by_type[t].append((s, e))
        by_type["ORG_ALL"] = set(by_type["ORG_FULL"]) | set(by_type["ORG_SHORT"])
        for t in per_type:
            g = gold[t]
            pset = set(by_type[t])
            tp = len(pset & g)
            fp = len(pset) - tp
            fn = len(g) - tp
            per_type[t]["tp"] += tp
            per_type[t]["fp"] += fp
            per_type[t]["fn"] += fn
            per_type[t]["n"] += len(g)
    rows = {}
    for t in per_type:
        x = per_type[t]
        tp, fp, fn = x["tp"], x["fp"], x["fn"]
        p = tp / (tp + fp) if (tp + fp) else 0.0
        r = tp / (tp + fn) if (tp + fn) else 0.0
        f1 = 2 * p * r / (p + r) if (p + r) else 0.0
        rows[t] = {"p": p, "r": r, "f1": f1, "tp": tp, "fp": fp, "fn": fn, "n": x["n"]}
    avg_time = sum(timings) / len(timings) if timings else 0.0
    return rows, avg_time


def fmt_row(rows, avg_time):
    parts = []
    for t in TYPES + ("ORG_ALL",):
        x = rows[t]
        parts.append(
            f"{t}: P={x['p']:.3f} R={x['r']:.3f} F1={x['f1']:.3f} (tp={x['tp']},fp={x['fp']},fn={x['fn']},n={x['n']})"
        )
    parts.append(f"avg={avg_time * 1000:.1f}ms/文")
    return " | ".join(parts)


# ---------------------------------------------------------------------------
# 方案 A: jieba 分词 + 规则基线（公司后缀启发式 + 姓氏启发式）
# ---------------------------------------------------------------------------

SURNAMES = set(
    "赵钱孙李周吴郑王冯陈褚卫蒋沈韩杨朱秦尤许何吕施张孔曹严华金魏陶姜戚谢邹喻柏水窦章云苏潘葛奚范彭郎鲁韦昌马苗凤花方俞任袁柳酆鲍史唐费廉岑薛雷贺倪汤滕殷罗毕郝邬安常乐于时傅皮卞齐康伍余元卜顾孟平黄和穆萧尹姚邵湛汪祁毛禹狄米贝明臧计伏成戴谈宋茅庞熊纪舒屈项祝董梁杜阮蓝闵席季麻强贾路娄危江童颜郭梅盛林刁钟徐邱骆高夏蔡田樊胡凌霍虞万支柯昝管卢莫经房裘缪干解应宗丁宣贲邓郁单杭洪包诸左石崔吉钮龚程嵇邢滑裴陆荣翁荀羊於惠甄麹家封芮羿储靳汲邴糜松井段富巫乌焦巴弓牧隗山谷车侯宓蓬全郗班仰秋仲伊宫宁仇栾暴甘斜厉戎祖武符刘景詹束龙叶幸司韶郜黎蓟薄印宿白怀蒲邰从鄂索咸籍赖卓蔺屠蒙池乔阴欎胥能苍双闻莘党翟谭贡劳逄姬申扶堵冉宰郦雍却璩桑桂濮牛寿通边扈燕冀郏浦尚农温别庄晏柴瞿阎充慕连茹习宦艾鱼容向古易慎戈廖庾终暨居衡步都耿满弘匡国文寇广禄阙东欧殳沃利蔚越夔隆师巩厍聂晁勾敖融冷訾辛阚那简饶空曾毋沙乜养鞠须丰巢关蒯相查后荆红游竺权逯盖益桓公"
)

ORG_SUFFIXES = (
    "股份有限公司", "有限责任公司", "集团有限公司", "有限公司",
    "分公司", "集团公司", "集团", "研究院", "研究所", "大学", "学院",
    "银行", "医院", "事务所", "合作社", "基金会",
)

# 常用词/停用词黑名单：过滤姓氏启发式的明显误报（"劳动/平等/合同/公司" 等）
PER_BLACKLIST = {
    "劳动", "平等", "公司", "合同", "甲方", "乙方", "双方", "联系", "经理",
    "人力", "人力资", "资格", "资源", "规定", "约定", "义务", "权利", "责任",
    "声明", "保证", "承担", "执行", "成立", "签订", "订立", "签署", "包括",
    "涉及", "关于", "对于", "根据", "按照", "上述", "以下", "之一", "部门",
    "人员", "员工", "项目", "产品", "服务", "信息", "数据", "安全", "保密",
    "技术", "文件", "报告", "记录", "期间", "期限", "日期", "开始", "结束",
    "按月", "一次性", "每月", "当日", "次日", "工作日", "全部", "部分",
}


class JiebaRuleBaseline:
    name = "a_jieba_rule"

    def __init__(self):
        import jieba
        self.jieba = jieba

    def predict(self, text):
        words = list(self.jieba.cut(text))
        # jieba.cut 无偏移，用自实现 lcut 偏移版
        # 直接使用 jieba.tokenize 获取 (word, start, end)
        tokens = list(self.jieba.tokenize(text, mode="default"))
        out = []
        n = len(tokens)
        for i, (word, s, e) in enumerate(tokens):
            # 机构全称：以公司后缀结尾的词，向前扩展直到标点/句首
            if word.endswith(ORG_SUFFIXES) and len(word) >= 4:
                start = s
                j = i - 1
                while j >= 0:
                    pw, ps, pe = tokens[j]
                    if re.search(r"[，。；：、\n（）：:（）]|\d{4}年|字第", pw):
                        break
                    start = ps
                    j -= 1
                out.append((start, e, "ORG_FULL"))
                continue
            # 人名：2-3 字词，首字为常见姓氏，且非黑名单
            if 2 <= len(word) <= 3 and word[0] in SURNAMES and word not in PER_BLACKLIST:
                # 排除明显非人名（前一个 token 是公司后缀等场景交给上面分支）
                out.append((s, e, "PER"))
        # 去重（保留 ORG_FULL 优先）
        seen = set()
        final = []
        for s, e, t in out:
            if (s, e, t) in seen:
                continue
            seen.add((s, e, t))
            final.append((s, e, t))
        return final


# ---------------------------------------------------------------------------
# 方案 D: 词典 + 上下文增强（全称→简称关联），验证"词典兜底"效果
# ---------------------------------------------------------------------------

class DictContextEnhancer:
    """词典(种子全称) + “（以下简称“X”）”上下文学习全称→简称映射 + 正文简称匹配。
    模拟产品中"用户维护公司词典 + 文档内上下文规则"的配置方式：
      1) 种子词典中的全称做最长匹配 -> ORG_FULL
      2) 扫描“（以下简称“X”）”出现处，向前关联最近的机构全称，学习 full->abbr 映射
      3) 用学习到的简称匹配正文 -> ORG_SHORT（跳过全称内部位置）
    """
    name = "d_dict_context"

    def __init__(self, org_full_dict):
        self.org_full = sorted(org_full_dict, key=len, reverse=True)
        self.ctx_pat = re.compile(
            r"（以下简称[“\"“]([^”\"”（）]+)[”\"”]）|\(以下简称[“\"]([^”\"）（]+)[”\"]\)"
        )

    def predict(self, text):
        out = []
        used = set()
        # 1) 词典全称最长匹配（记录位置以便关联简称）
        full_spans = []
        for full in self.org_full:
            for m in re.finditer(re.escape(full), text):
                span = (m.start(), m.end())
                if span not in used:
                    out.append((m.start(), m.end(), "ORG_FULL"))
                    used.add(span)
                    full_spans.append((m.start(), m.end(), full))
        # 2) “（以下简称“X”）” -> 学得简称 + 上下文命中
        abbrevs = set()
        for m in self.ctx_pat.finditer(text):
            x = m.group(1) or m.group(2)
            if not x:
                continue
            inner = m.group(0)
            idx = inner.find(x)
            span = (m.start() + idx, m.start() + idx + len(x))
            if span not in used:
                out.append((span[0], span[1], "ORG_SHORT"))
                used.add(span)
            # 向前关联最近的机构全称（在括号定义之前），学习 full->abbr
            prev_full = None
            for fs, fe, full in full_spans:
                if fe <= m.start():
                    prev_full = (full, fs, fe)
                else:
                    break
            if prev_full and len(x) >= 2:
                abbrevs.add(x)
        # 3) 学习到的简称匹配正文
        for ab in sorted(abbrevs, key=len, reverse=True):
            for m in re.finditer(re.escape(ab), text):
                span = (m.start(), m.end())
                if span in used:
                    continue
                inside_full = any(s <= m.start() and m.end() <= e for (s, e, t) in out if t == "ORG_FULL")
                if inside_full:
                    continue
                out.append((m.start(), m.end(), "ORG_SHORT"))
                used.add(span)
        return out


# ---------------------------------------------------------------------------
# 方案 B: fastHan (一代, HF 模型)
# ---------------------------------------------------------------------------

class FastHanNER:
    name = "b_fasthan"

    def __init__(self, model_dir=None):
        os.environ.setdefault("HF_ENDPOINT", "https://hf-mirror.com")
        from fastHan import FastHan
        if model_dir:
            self.model = FastHan(url=model_dir)
        else:
            self.model = FastHan()
        self.model.add_user_dict({})  # 兼容空调用
        # 选 NER 任务的标签风格
        self.target = "NER"

    def predict(self, text):
        out = []
        answers = self.model(text, target=self.target)
        # 一次传整个文本；fastHan 返回 Sentence 列表（按句）
        for sent in answers:
            for tok in sent.tokens:
                ner = getattr(tok, "ner", None)
                if not ner:
                    continue
                lab = ner
                if lab in ("O", "O_"):
                    continue
                # fastHan 一代 NER 标签: NR=人名 NT=机构 NS=地名 (MSRA 风格, 无 BIO 前缀)
                if lab in ("B-NR", "I-NR", "NR", "NR_") or lab.startswith("B-NR") or lab.startswith("NR"):
                    t = "PER"
                elif lab in ("NT", "NT_", "B-NT", "I-NT") or lab.startswith("B-NT") or lab.startswith("NT"):
                    t = "ORG_FULL"
                else:
                    continue
                out.append((tok.loc[0], tok.loc[1], t))
        return out


# ---------------------------------------------------------------------------
# 方案 C: LTP
# ---------------------------------------------------------------------------

class LTPNER:
    name = "c_ltp"

    def __init__(self, model_path=None):
        from ltp import LTP
        if model_path is None:
            model_path = os.environ.get(
                "LTP_MODEL_DIR",
                os.path.join(HERE, ".ltp_cache", "LTP", "small"),
            )
        self.ltp = LTP(model_path)
        # LTP NER 标签: Nh=人名, Ni=机构名, Ns=地名 (BIOES -> get_entities 已合并为 tag)
        self.label_map = {"Nh": "PER", "Ni": "ORG_FULL"}

    def predict(self, text):
        out = []
        res = self.ltp([text], tasks=["cws", "ner"])
        cws = res.cws[0]
        ner = res.ner[0]  # list[(tag, span_str, word_start, word_end)]
        for tag, span, ws, we in ner:
            t = self.label_map.get(tag)
            if t is None:
                continue
            start = sum(len(w) for w in cws[:ws])
            end = start + len(span)
            out.append((start, end, t))
        return out


# ---------------------------------------------------------------------------
# 方案 E: ONNX BERT NER（概念验证）
# ---------------------------------------------------------------------------

class OnnxNer:
    """ONNX BERT 中文 NER（CLUENER 标签集）。
    name -> PER; company / organization -> ORG_FULL
    """
    name = "e_onnx_bert"

    def __init__(self, model_dir=None):
        import onnxruntime as ort
        from transformers import AutoTokenizer
        if model_dir is None:
            model_dir = os.environ.get(
                "ONNX_MODEL_DIR",
                os.path.join(HERE, ".onnx_cache", "gyr66-ner-onnx"),
            )
        self.sess = ort.InferenceSession(os.path.join(model_dir, "model.onnx"), providers=["CPUExecutionProvider"])
        self.tokenizer = AutoTokenizer.from_pretrained(model_dir)
        import json as _json
        cfg_path = os.path.join(model_dir, "config.json")
        with open(cfg_path, encoding="utf-8") as f:
            cfg = _json.load(f)
        self.id2label = cfg.get("id2label")
        if not self.id2label:
            raise ValueError("no id2label in config.json")
        self.input_names = [i.name for i in self.sess.get_inputs()]
        # 标签 -> 评测类型
        self.tag2type = {
            "name": "PER",
            "company": "ORG_FULL",
            "organization": "ORG_FULL",
        }

    def predict(self, text):
        enc = self.tokenizer(text, return_offsets_mapping=True, truncation=True, max_length=500)
        feeds = {"input_ids": [enc["input_ids"]], "attention_mask": [enc["attention_mask"]]}
        if "token_type_ids" in self.input_names:
            feeds["token_type_ids"] = [enc.get("token_type_ids", [0] * len(enc["input_ids"]))]
        elif "token_type_ids" in enc and len(self.input_names) == 3:
            feeds = {"input_ids": [enc["input_ids"]], "attention_mask": [enc["attention_mask"]],
                     "token_type_ids": [enc["token_type_ids"]]}
        logits = self.sess.run(None, feeds)[0]
        preds = logits.argmax(-1)[0].tolist()
        off = enc["offset_mapping"]
        out = []
        i = 0
        n = len(preds)
        while i < n:
            lab = self.id2label.get(str(preds[i]), "O")
            if lab == "O":
                i += 1
                continue
            ent_type = None
            if lab.startswith("B-"):
                ent_type = self.tag2type.get(lab[2:])
            if ent_type is None:
                i += 1
                continue
            s = off[i][0]
            j = i + 1
            e = off[i][1]
            while j < n:
                lab2 = self.id2label.get(str(preds[j]), "O")
                if lab2 == f"I-{lab[2:]}" or lab2 == f"B-{lab[2:]}":
                    e = off[j][1]
                    j += 1
                    if lab2 == f"B-{lab[2:]}":
                        # 新的 B 开始，结束当前实体
                        break
                else:
                    break
            if e > s:
                out.append((s, e, ent_type))
            i = j
        return out


class CombinedDetector:
    """NER + 词典/上下文兜底 组合方案（产品默认形态）：
    NER 负责 PER 与未登录 ORG；词典+上下文负责 ORG_FULL/ORG_SHORT。
    合并规则：并集去重；同一 span 出现两种类型时 ORG_FULL 优先。
    """
    name = "f_combined"

    def __init__(self, ner, dict_enhancer):
        self.ner = ner
        self.dict_enhancer = dict_enhancer

    def predict(self, text):
        a = self.ner.predict(text)
        b = self.dict_enhancer.predict(text)
        merged = {}
        for s, e, t in list(a) + list(b):
            key = (s, e)
            if key not in merged:
                merged[key] = t
            elif t == "ORG_FULL" and merged[key] != "ORG_FULL":
                merged[key] = "ORG_FULL"
        return [(s, e, t) for (s, e), t in merged.items()]


# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fasthan", action="store_true", help="跑 fastHan")
    ap.add_argument("--fasthan-model", default=None, help="fastHan 本地模型目录")
    ap.add_argument("--ltp", action="store_true", help="跑 LTP")
    ap.add_argument("--onnx", action="store_true", help="跑 ONNX BERT NER")
    ap.add_argument("--onnx-model", default=None, help="ONNX 模型目录(含 model.onnx / config.json)")
    ap.add_argument("--all", action="store_true", help="跑全部")
    args = ap.parse_args()

    docs = load_corpus()
    print(f"corpus: {len(docs)} 文档, 标注实体: " + ", ".join(
        f"{t}={sum(1 for d in docs for e in d['entities'] if e['type'] == t)}" for t in TYPES))

    results = {}

    # A: jieba 基线
    try:
        t0 = time.perf_counter()
        a = JiebaRuleBaseline()
        rows, avg = evaluate(docs, a.predict)
        results[a.name] = (rows, avg)
        print(f"[{a.name}] load={time.perf_counter() - t0:.1f}s  {fmt_row(rows, avg)}")
    except Exception as e:
        print(f"[a_jieba_rule] FAILED: {e}")
        traceback.print_exc()

    # D: 词典 + 上下文增强（种子 = 语料中的公司全称，模拟产品维护的公司词典）
    dict_inst = None
    try:
        orgs = sorted({e["value"] for d in docs for e in d["entities"] if e["type"] == "ORG_FULL"})
        dict_inst = DictContextEnhancer(orgs)
        rows, avg = evaluate(docs, dict_inst.predict)
        results[dict_inst.name] = (rows, avg)
        print(f"[{dict_inst.name}] 种子词典={orgs}  {fmt_row(rows, avg)}")
    except Exception as e:
        print(f"[d_dict_context] FAILED: {e}")
        traceback.print_exc()

    # B: fastHan
    if args.fasthan or args.all:
        try:
            t0 = time.perf_counter()
            b = FastHanNER(args.fasthan_model)
            rows, avg = evaluate(docs, b.predict)
            results[b.name] = (rows, avg)
            print(f"[{b.name}] load={time.perf_counter() - t0:.1f}s  {fmt_row(rows, avg)}")
            if dict_inst:
                comb = CombinedDetector(b, dict_inst)
                rows, avg = evaluate(docs, comb.predict)
                results[comb.name + "_fasthan"] = (rows, avg)
                print(f"[{comb.name}_fasthan]  {fmt_row(rows, avg)}")
        except Exception as e:
            print(f"[b_fasthan] FAILED: {type(e).__name__}: {e}")
            traceback.print_exc()

    # C: LTP
    if args.ltp or args.all:
        try:
            t0 = time.perf_counter()
            c = LTPNER()
            rows, avg = evaluate(docs, c.predict)
            results[c.name] = (rows, avg)
            print(f"[{c.name}] load={time.perf_counter() - t0:.1f}s  {fmt_row(rows, avg)}")
            if dict_inst:
                comb = CombinedDetector(c, dict_inst)
                rows, avg = evaluate(docs, comb.predict)
                results[comb.name + "_ltp"] = (rows, avg)
                print(f"[{comb.name}_ltp]  {fmt_row(rows, avg)}")
        except Exception as e:
            print(f"[c_ltp] FAILED: {type(e).__name__}: {e}")
            traceback.print_exc()

    # E: ONNX
    if args.onnx or args.all:
        try:
            t0 = time.perf_counter()
            e = OnnxNer(args.onnx_model)
            rows, avg = evaluate(docs, e.predict)
            results[e.name] = (rows, avg)
            print(f"[{e.name}] load={time.perf_counter() - t0:.1f}s  {fmt_row(rows, avg)}")
            if dict_inst:
                comb = CombinedDetector(e, dict_inst)
                rows, avg = evaluate(docs, comb.predict)
                results[comb.name + "_onnx"] = (rows, avg)
                print(f"[{comb.name}_onnx]  {fmt_row(rows, avg)}")
        except Exception as e:
            print(f"[e_onnx_bert] FAILED: {type(e).__name__}: {e}")
            traceback.print_exc()

    print("\n===== 汇总 =====")
    for name, (rows, avg) in results.items():
        print(f"{name}: {fmt_row(rows, avg)}")


if __name__ == "__main__":
    main()
