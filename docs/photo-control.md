# Chromora 本地调色接口

`photo_control` 通过 JSON 请求调用 Chromora 的修图引擎。Agent 需要能运行本机命令、读取用户指定的照片、写入输出目录并查看预览。接口按照片路径运行，与桌面窗口状态独立。

在 Windows x64 的 MSVC 开发环境中安装 Rust 1.89 或更新版本及 Windows SDK，然后在仓库根目录构建命令行示例。

```powershell
cargo build --locked --release --example photo_control
```

把下面的请求保存为仓库根目录的 `photo-control-request.local.json`。示例使用仓库中的合成测试图，处理自己的照片时将 `source` 换成原片路径。

```json
{
  "source": "fixtures/exposure_reference_input.png",
  "output_dir": "photo-control-results/run-001",
  "preview_long_edge": 1600,
  "variants": [
    { "name": "baseline" },
    {
      "name": "natural",
      "adjustments": { "exposure": 0.30, "highlights": -20 },
      "full_export": false
    }
  ]
}
```

先创建输出目录的父目录，再执行请求。每次使用新的 `output_dir`。请求中使用的相对路径以请求文件所在目录为基准。

```powershell
New-Item -ItemType Directory -Path .\photo-control-results -Force | Out-Null
& .\target\release\examples\photo_control.exe .\photo-control-request.local.json
```

- 一个请求可包含 1–16 个方案。方案名以 ASCII 字母或数字开头，可继续使用 ASCII 字母、数字、`-` 和 `_`，最多 64 字符，名称按大小写不敏感规则去重。
- `preset` 可指向 `landscape` 参数预设，随后应用 `adjustments`。曝光步进为 0.05 EV。其余参数采用 Chromora 自身的字段名与范围。[调整参数](../src/core/settings.rs) · [局部蒙版](../src/core/masks.rs)
- 每个方案生成 `preview.png` 和 `edit.chromora`。查看候选预览后，可从 `response.json` 取该方案的完整 `settings` 用于下一轮 `adjustments`。设 `full_export` 为 `true` 可同时输出完整分辨率的 RGB16 PNG 和质量参数 96 的 JPG。
- 成功响应会写到标准输出及输出目录的 `response.json`。核对退出码为 `0`、`success` 和 `source_unchanged` 为 `true`，以及输出文件存在且可读。工程引用原片，可在桌面版打开继续调整。

仓库附带 Chromora 自行编写的富士、理光风格参数。CLI 可将参数文件填入 `preset`。桌面版可在“风格”中直接打开单个 JSON，或将风格包复制到“项目”面板显示的数据目录下的 `数据/预设` 并刷新预设列表。[参数文件与来源清单](../recipes/chromora-film-styles-v1/)
