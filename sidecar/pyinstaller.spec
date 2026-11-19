# -*- mode: python ; coding: utf-8 -*-
# 注意：本 spec 已过期且当前构建不使用（scripts/build-sidecar.sh 直接用 PyInstaller 命令行参数）；
# 见 docs/release.md §5 的处理待办。

"""PyInstaller 配置：将墨隐 InkVeil sidecar 打包为单文件、无控制台的可执行文件。

产物名与 tauri.conf.json 中 bundle.externalBin 声明的 "binaries/anonkit-sidecar"
保持一致（Windows 上为 anonkit-sidecar.exe）。
"""

a = Analysis(
    ["app.py"],
    pathex=[],
    binaries=[],
    datas=[],
    hiddenimports=[],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    # 排除重 ML 依赖（后续接入 NER 时按需放宽），当前阶段保持精简。
    excludes=["torch", "transformers", "numpy", "pandas", "onnxruntime", "scipy"],
    noarchive=False,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name="anonkit-sidecar",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,  # 无控制台窗口
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
