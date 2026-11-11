# Bundled LibRaw 0.22.3: JPEG and X3F enabled

2026-11-06: Chromora 0.5 uses a local MSVC x64 build from the same official
0.22.3 source archive, enabling `USE_JPEG` and `USE_X3FTOOLS`. No LibRaw source
files are patched. The existing source/header version is unchanged.

Current `bin/libraw.dll`: 1,496,064 bytes, SHA-256
`cbd9f18eb43d23d8214fcfaafb77feb3343af227b71e502e033146bdd4e88261`.

The unmodified upstream DLL is retained as `bin/libraw-upstream-0.22.3.dll`,
SHA-256 `2c4df36d9011267fd3ffd5c23e80d69d5f42c3d503f03ac02448f5c8a7fe0fa2`.
It is rollback evidence and is not loaded automatically.

JPEG is statically linked from the official libjpeg-turbo 3.1.2 source release:
https://github.com/libjpeg-turbo/libjpeg-turbo/releases/tag/3.1.2

Source asset: https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/3.1.2/libjpeg-turbo-3.1.2.tar.gz

Archive SHA-256: `8f0012234b464ce50890c490f18194f913a7b1f4e6a03d6644179fa0f867d0cf`.
Build flags: shared OFF, static ON, CRT DLL ON, SIMD OFF, TurboJPEG OFF.
JPEG is covered by the IJG and BSD-style license files retained under
`jpeg-licenses`. This software is based in part on the work of the Independent JPEG Group.
X3F tools are included in the LibRaw source and use the BSD-style license
retained in `LICENSE.X3FTOOLS`.

Both upstream source archives are bundled in `sources`. To rebuild the DLL,
run `build-enhanced-windows.ps1` with MSVC x64, Windows SDK and the Visual Studio
CMake component installed. Use a new ASCII `BuildRoot` path, because current
CMake/NMake may misencode absolute PDB/include paths containing Chinese.
The script leaves its build directory intact and never replaces the bundled DLL.
Build provenance and smoke results are in the delivery validation records.

The bridge also copies actual decoder metadata into a Chromora-owned POD,
filling only absent EXIF fields. It preserves unknown timezone, sensor bit
depth, white-balance mode and proprietary look fields rather than inferring them.

The following describes the upstream archive used for both builds:

---

Official Windows x64 package: https://www.libraw.org/data/LibRaw-0.22.3-Win64.zip

Version: 0.22.3, built upstream with MSVC 2022.

Archive SHA-256: `4e2213a0b10e32199a30b9114a61376cdb40f94df861262b4a6dca659701276b`

The unmodified upstream DLL, public headers, COPYRIGHT and both upstream
license texts are retained. LibRaw is available under LGPL 2.1 or CDDL 1.0;
these files retain their upstream license independently of Chromora's MIT code.
The original archive is retained only as download evidence and is not required
to run or build Chromora.

`native/raw_bridge.cpp` loads the DLL using an explicit absolute path, checks
its exact version before accessing structures, and applies the previous rawpy
settings (camera WB, no auto brightening, 16-bit sRGB output and sRGB gamma).
No custom color/look profile or proprietary camera simulation is bundled.
