# Chromora 0.10.1

Windows x64 上的原生修图软件（Rust + GPUI），以及使用同一套引擎的本机命令 `photo_control`。`photo_control` 只读本地 JSON 文件，不监听网络，也不操作已经打开的桌面窗口。

产品版本和 Cargo 包版本都是 **0.10.1**。当前能力以本页和 [PARITY.md](PARITY.md) 顶部为准。0.x 发布说明已归档到 [docs/HISTORY.md](docs/HISTORY.md)，其中“RAW 尚未实现”“28 项测试”等句子描述的是当时的纵切片，不是现在的状态。

## 安装和运行

### 便携包

已发布的 Windows x64 便携包：

[Chromora-0.10.1-Windows-x64-Portable.zip](https://github.com/Winsiby/Chromora-Rust/releases/download/v0.10.1/Chromora-0.10.1-Windows-x64-Portable.zip)

SHA-256：`7345d547c025643007757dfe1c6e3af6b3c3f7eae9626a4d2f744f59c48f4857`

1. 下载 ZIP 并核对哈希。
2. 解压整个目录，保留 `chromora.exe` 与同目录 DLL。
3. 双击 `chromora.exe` 启动。

运行不需要 Python、Node 或浏览器。本仓库的 CI 不负责打这个便携包，也不做代码签名。

### 从源码构建

需要 Rust 1.89（见 `rust-toolchain.toml`）、MSVC 和 Windows SDK。在仓库根目录：

```powershell
cargo build --locked --release
```

生成 `target\release\chromora.exe` 和 `target\release\photo_control.exe`。`build.rs` 只在 Windows 上编译 libjpeg-turbo 与 LibRaw 桥；Linux 可以编译并测试 Rust 核心，但没有这条精确 JPEG / RAW 原生路径。

### photo_control

`photo_control` 是正式二进制，不是 example。

```powershell
cargo build --locked --release --bin photo_control
& .\target\release\photo_control.exe --help
& .\target\release\photo_control.exe .\photo-control-request.local.json
```

请求格式、预设和输出文件见 [docs/photo-control.md](docs/photo-control.md)。

## 数据目录

环境变量 `CHROMORA_DATA_ROOT` 若指向一个可以创建并写入数据子目录的路径，则优先使用该路径。空值视为未设置。路径尚不存在时，只要父目录可写，程序会创建所需子目录。

未设置，或所给路径不可写时，Windows 保持原有默认，不改已有用户的工程位置：

1. `D:\修图软件`
2. 上一项不可写时，`%LOCALAPPDATA%\Chromora`

非 Windows 且未设置该变量时，仍使用 `$HOME/Chromora`。左侧“项目”面板显示本次实际根目录。回退不会自动搬走已有文件。

该根目录下使用：

- `数据/工程`、`数据/原片`、`数据/预设`
- `导出`
- `缓存`

## 当前能力

- 导入 JPG、PNG、TIFF 和 RAW。RAW 扩展名与 LibRaw 0.22.3 解码入口一致（27 种）。带任意透明像素的 PNG 会拒绝；alpha 全不透明的 PNG 可以导入。
- 工作缓冲是 float32 sRGB。新导入使用 `float_srgb`。旧照片保留 `legacy_srgb`、`hq_srgb` 或 `accurate_srgb`，打开旧工程不会自动改颜色。
- 基础、曲线、HSL、分段分级、细节、人像、sRGB 3D LUT、构图，以及线性/径向局部蒙版。
- 撤销和重做随工程保存（每张照片合计最多 100 步）。新工程为 schema 7，渲染版本 `chromora-0.8-srgb-v4`。旧 schema 与 Python 工程可以打开，首次保存必须另存。
- 导出 JPG（质量 1–100，默认 96）、16 位 PNG、16 位 TIFF、双像素层 PSD，以及 Patchy 的 TIFF 交接。可以按目录串行导出工程内照片。导出拒绝覆盖原片和已有目标。
- 调色复制/粘贴只在当前会话有效。个人预设从实际数据目录的 `数据/预设` 读取。
- 风格面板可以直接选用随仓库提供的参数滤镜。名称里的富士、理光是参数近似，不是原厂滤镜，也不是逐像素复刻。
- 方向键切换照片，滚轮缩放画布，窄窗口可收起左侧工具、右侧面板和底部照片条。

滤镜入口的操作说明见 [WORKFLOW101.md](WORKFLOW101.md)。0.10 的输入、预设和批量导出细节见 [WORKFLOW10.md](WORKFLOW10.md)。

## 当前边界

这些还没有交付，文档里不再把已经有的 RAW 解码、TIFF、PSD、构图、蒙版或撤销写成“未实现”：

- 没有端到端广色域工作缓冲，不能按 P3、Adobe RGB 或 ProPhoto 导出，也没有逐显示器校色。
- RAW 能解码，但不是按品牌和机型验收的色彩还原。LibRaw 声明的 1,274 条支持项不是已验收机型数。厂商机内风格没有按机型复现。
- 白平衡吸管、高光/阴影剪切提示、三向色轮，以及原清单里的 P2 / P3 功能，本轮不做。
- 干净 Windows 启动、多显示器 DPI、长时间稳定性和 24MP 性能基线仍未作为通过项验收。

## 开发

Windows CI 先用 `scripts/verify-native-hashes.ps1` 对照 `native/checksums.sha256`，再运行：

```powershell
cargo test --locked --all-targets
```

Dependabot 每周查看 Cargo 和 GitHub Actions 依赖。CI 不打包便携版，也不签名。

许可见 [LICENSE](LICENSE)。Chromora 自己的代码是 MIT；捆绑的 LibRaw（LGPL 2.1 或 CDDL 1.0）和 libjpeg-turbo 使用 `native/` 里的上游许可。
