# 0.7 JPEG 独立参照

`icc-srgb-pillow-rgb.png` 是 Pillow 13.0.0 / libjpeg-turbo 3.2.0 对既有
`../icc_srgb_reference.jpg` 执行 `Image.open(...).convert("RGB")` 的冻结 RGB8
结果，不经过 Chromora 或其 ICC 转换。测试对中性 sRGB ICC 导入后的预览保留
原有 1 LSB 容差。源 JPEG 不变。

- JPEG SHA256：`39a95bb69262619ed4fd1f3d953fa429feaff6c7f269551eb306226479fef855`
- PNG SHA256：`d794b36bb183f488f965112deb3c2ff97c2db8c1c622c24a9a7077f9e0b07a9d`

此两像素回归测试只验证导入契约；完整色差结论以独立 25 项固定夹具验收为准。
