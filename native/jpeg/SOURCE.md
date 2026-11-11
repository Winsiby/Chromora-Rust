# JPEG pixel decoding for Chromora 0.7 (Windows x64)

The unmodified libjpeg-turbo 3.1.2 static library is reused from the local
LibRaw-enhanced build. `jpeg-static.lib` SHA256:
`304077f3cf159373c55b4f3b4c933b08b279061bebd7fbafba4707b0ff2bb776`.
It uses the v6b API (62), MSVC x64, CRT DLL ON, SIMD OFF. Runtime does not
need another DLL or Python. Public `jpeglib.h`, `jmorecfg.h` and the generated
`jconfig.h` are retained here. Upstream source is unchanged.

Official release: https://github.com/libjpeg-turbo/libjpeg-turbo/releases/tag/3.1.2

The official source archive is retained at
`../libraw/sources/libjpeg-turbo-3.1.2.tar.gz`, SHA256
`8f0012234b464ce50890c490f18194f913a7b1f4e6a03d6644179fa0f867d0cf`.
Licenses are retained at `../libraw/jpeg-licenses`. This software is based in
part on the work of the Independent JPEG Group.

`build-windows.ps1` rebuilds from that archive using the same configuration
and reports the resulting library; use a new ASCII BuildRoot. It never
replaces the bundled library. Object paths and compiler metadata may affect
the rebuilt binary hash; the included static library has the hash above.

`../jpeg_bridge.c` explicitly selects `JDCT_ISLOW`, fancy upsampling and RGB8
output. Grayscale is expanded to RGB; Adobe CMYK/YCCK output uses inverted
CMYK, other CMYK uses normal polarity. Non-RGB embedded ICC remains an
explicit unsupported-profile error in the Rust color layer. Only 8-bit JPEG
is supported. Decoder warnings, including synthetic EOI for truncated input,
are errors. EXIF orientation and ICC processing run once in the Rust layer.

Only `AccurateSrgb` uses this decoder. `HqSrgb` and `LegacySrgb` retain the
previous Zune pixel decoder so reopening saved projects does not change
their rendering. New processing does not expand the bounded sRGB working gamut.
