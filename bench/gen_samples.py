#!/usr/bin/env python3
"""墨隐 InkVeil 性能基准样本生成器。

用法：
  python gen_samples.py text <path> <size_mb>    # 生成 <size_mb> MB 中文文本（含敏感串）
  python gen_samples.py pdfs <dir> <count> <pages>  # 生成 <count> 个 <pages> 页 PDF
"""

import os
import random
import string
import sys

# 敏感串素材：手机号/身份证（校验位合法）/银行卡（Luhn 合法）/邮箱/日期
MOBILE = "13800138000"
ID_CARD = "110101198403201234"
BANK = "6222020200088888884"
EMAIL = "contact@example.com"
DATE = "2024-06-30"

SENSITIVE = [MOBILE, ID_CARD, BANK, EMAIL, DATE]
TEXT_CHUNK = (
    "乙方同意于{date}前向甲方支付人民币¥1,234,567.89元，"
    "款项汇入乙方指定银行账户（卡号：{bank}），联系手机 {mobile}，"
    "法定代表人身份证号 {idc}，电子邮箱 {email}。"
    "本合同一式两份，双方各执一份，自签订之日起生效。"
    "各方应本着诚实信用原则履行本合同项下全部义务，不得擅自变更或解除。"
)


def gen_text(path: str, size_mb: float) -> None:
    target = int(size_mb * 1024 * 1024)
    rng = random.Random(20240811)
    parts: list[str] = []
    total = 0
    while total < target:
        s = TEXT_CHUNK.format(
            date=rng.choice(SENSITIVE) if rng.random() < 0.3 else DATE,
            bank=rng.choice(SENSITIVE) if rng.random() < 0.3 else BANK,
            mobile=rng.choice(SENSITIVE) if rng.random() < 0.3 else MOBILE,
            idc=rng.choice(SENSITIVE) if rng.random() < 0.3 else ID_CARD,
            email=rng.choice(SENSITIVE) if rng.random() < 0.3 else EMAIL,
        )
        # 掺入随机数字串/网址，逼近真实文档形态
        if rng.random() < 0.4:
            s += " " + "".join(rng.choices(string.digits, k=rng.randint(6, 11)))
        if rng.random() < 0.2:
            s += " http://example.com/page%d" % rng.randint(1, 9999)
        parts.append(s)
        total += len(s.encode("utf-8"))
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(parts))
    actual = os.path.getsize(path) / (1024 * 1024)
    print(f"文本样本: {path} ({actual:.2f}MB)")


def gen_pdfs(out_dir: str, count: int, pages: int) -> None:
    from reportlab.lib.pagesizes import A4
    from reportlab.pdfbase import pdfmetrics
    from reportlab.pdfbase.cidfonts import UnicodeCIDFont
    from reportlab.pdfgen import canvas

    pdfmetrics.registerFont(UnicodeCIDFont("STSong-Light"))
    os.makedirs(out_dir, exist_ok=True)
    rng = random.Random(7)
    for i in range(count):
        path = os.path.join(out_dir, f"bench-{i}.pdf")
        c = canvas.Canvas(path, pagesize=A4)
        for p in range(pages):
            c.setFont("STSong-Light", 11)
            y = 770
            for line in range(46):  # 每页约 46 行，模拟合同密度
                s = TEXT_CHUNK.format(
                    date=rng.choice(SENSITIVE),
                    bank=BANK,
                    mobile=rng.choice(SENSITIVE) if rng.random() < 0.5 else MOBILE,
                    idc=ID_CARD,
                    email=EMAIL,
                )
                c.drawString(50, y, f"{i}-{p}-{line} " + s[:80])
                y -= 16
            c.showPage()
        c.save()
    print(f"PDF 样本: {count} 个 × {pages} 页 → {out_dir}")


def main() -> None:
    cmd = sys.argv[1]
    if cmd == "text":
        gen_text(sys.argv[2], float(sys.argv[3]))
    elif cmd == "pdfs":
        gen_pdfs(sys.argv[2], int(sys.argv[3]), int(sys.argv[4]))
    else:
        sys.exit(f"未知子命令: {cmd}")


if __name__ == "__main__":
    main()
