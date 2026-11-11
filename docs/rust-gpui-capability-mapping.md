# Chromora 技术映射报告

> 截至本次研究，父任务检查时新项目目录尚不存在。因此本报告映射的是 Rust/GPUI/第三方生态与架构可行性，不是对 Chromora 已写代码的验收。**有 crate 不等于应用功能现成**：表中的库通常只提供基础 API、算法原语或数据格式能力，产品交互、工程流程和验收仍需实现。

覆盖原始功能清单 33 行：0.3 已交付 9 行、P1 8 行、P2 10 行、P3 6 行；另按项目经理要求新增“拍摄信息/EXIF 分析”一行，作为扩展项单独标识，不计入原清单统计。

## 逐项映射

表中“现成基础”只表示研究材料有证据支持的底层候选，不代表已选型、已集成或已在目标 Windows 机器通过测试。

| 清单项 | 现成基础（库/框架） | 必须自研/集成 | 风险 |
|---|---|---|---|
| **S1 照片导入**（单张、多张、目录、拖放；JPG/PNG/TIFF/RAW） | 当前纵切片限 JPG/PNG RGB8/sRGB；这不是 RAW 支持。后续 RAW 可比较 Rawler crate、另一个独立项目 `rawloader`、LibRaw FFI 与 RawSpeed 组件，均须用同一机型样片验收；`image` 不能代替 RAW 显影。[R1][R2][R3][R4][R78][R79][R81] | 文件/目录/拖放工作流、过滤校验、原片副本规则、错误处理、异步解码、RAW 预览/缓存；增加“机型×固件×RAW 子格式”兼容登记、解码器适配层、原始元数据与 profile 来源追踪。 | 当前 JPG/PNG 切片不表示 RAW 解码或相机色彩已实现。格式扩展名支持不代表压缩变体、黑白电平、WB、CFA/矩阵或渲染正确；必须逐机型和变体验收。GPUI 原生文件对话框/拖放是否开箱可用未证实。[R2][R3][R4][R78][R79][R81] |
| **M1 拍摄信息/EXIF 分析**（新增扩展项） | `kamadak-exif` 是纯 Rust EXIF 解析候选，支持 JPEG、PNG、TIFF、HEIF/HEIC/AVIF、WebP 及部分 TIFF-based RAW；标准 EXIF 能提供拍摄时间、Make/Model、镜头/焦距、35mm 等效焦距、光圈/快门/ISO、曝光补偿、方向等字段。文件名/大小/像素尺寸/位深应从文件与解码结果读取，ICC profile 另按各容器解析，不应当作 EXIF 字段。[R46][R47][R48] | 做统一 metadata model 与来源标记；按容器分别提取 EXIF、ICC/XMP 与图像属性；单位/时间时区与缺失状态处理；将 MakerNote 厂商风格标签按具体机型逐步补齐。UI 缺字段显示“—/未记录”，不拿文件修改时间冒充拍摄时间；GPS 默认隐藏，序列号不展示。 | JPEG 的 EXIF 通常在 APP1，ICC profile 分段放在 APP2；PNG 用 eXIf 块存 EXIF、iCCP 块存 ICC；TIFF/许多 RAW 使用 TIFF IFD/容器内不同目录；HEIF/AVIF 的 EXIF 与色彩信息又由其容器元数据项/属性承载。EXIF parser 可读一个容器，不代表 ICC 路径也已接通。MakerNote 是厂商私有且常按机型/固件变化，可能含偏移、未知或不可解码字段；相机“WB”标准标签通常只是自动/手动模式，并不保证有 Kelvin 值。首个纵切片的 JPG/PNG RGB8 范围不应误报为完整元数据兼容；按目标机型样片分期验收。[R46][R47][R48] |
| **S2 画布和选片**（胶卷、Fit、100%、分屏、空格暂看、快速预览） | GPUI Render/Element/事件机制可作自绘画布与交互底层；gpui-component `img()` 可显示图像；Iced Viewer 有独立缩放/平移能力但不是 GPUI 控件。[R5][R6][R7] | 胶卷与缩略图缓存、缩放锚点/平移、分屏比较、空格按下/松开边界、快速预览与高分辨率重算、任务取消和过期结果丢弃。 | 未发现可直接嵌入 GPUI 的完整照片编辑画布。GPUI 的 Windows 稳定性和大图性能须先实测；Iced 混用会增加集成成本。[R5][R7][R8] |
| **S3 基础调色**（曝光、对比、高光/阴影、白黑阶、色温/色调、饱和度、自然饱和、清晰度） | `image` 提供像素缓冲；Photon 有通用亮度/饱和度/卷积等候选算子；`wgpu` 可作自研 GPU 执行底座；LCMS 可供 ICC 转换。RGB8/sRGB 仅是当前 JPG/PNG 切片格式，不预设长期编辑 master；线性宽色域、高位深工作表示待基准选型。[R9][R10][R11][R12][R82][R83] | 非破坏参数模型、每项摄影语义和顺序、场景线性/高位深管线、输入/显示/导出颜色变换、预览/导出一致、CPU 回退及性能基准。 | 通用亮度/锐化不等同摄影控件；量化、剪裁、色彩空间和 GPU/CPU 差异可能改变结果。工作空间未定，不可把 RGB8/sRGB 写成最终架构；Photon 算子需逐项验证。[R9][R10][R11] |
| **S4 曲线与色彩**（RGB/单通道曲线、八色 HSL、分离色调/平衡） | `wgpu`/CPU 像素缓冲及 LCMS 提供实现底层；未核实完整摄影曲线/HSL/色轮控件库。[R9][R11][R12] | 曲线插值、端点/通道定义、八色分段与平滑、分离色调权重、参数序列化和色彩管理。 | 曲线、HSL、色轮的数学语义与顺序会改变外观；需制定规范及参考图测试。[R9][R12] |
| **S5 细节**（去雾、锐化、亮度降噪） | `image`/`imageproc` 提供基础滤波与卷积；可在 `wgpu` 或 CPU 上实现专用算子。[R9][R13] | 摄影级算法、参数范围、边缘保护、噪声/锐化顺序、全分辨率与快速预览策略。 | 通用卷积不等同高质量去雾或降噪；线性/sRGB域及大图性能需实测。[R13] |
| **S6 人像**（手动框选后的皮肤修饰） | OpenCV `inpaint`/`seamlessClone` 是修复原语；image/imageproc 可作像素和滤波底座。[R14][R15] | 手动选区、皮肤瑕疵弱化、保留纹理平滑、肤色均匀、纹理滑杆、局部预览及可撤销参数。 | OpenCV 修复并非人像美化；可能糊斑、失真或产生塑料感。OpenCV Rust API 自述不稳定且 Windows 原生依赖需部署。[R14][R15] |
| **S7 风格库**（sRGB 3D `.cube`、收藏、强度及处理顺序） | `lut-cube` 声称解析 1D/3D/shaper；`image` 提供图像像素基础。厂商相机风格另设可选外观层：Canon Picture Style、Nikon Picture Control、Sony Creative Look/Creative Style、Fujifilm Film Simulation、OM Picture Mode、Panasonic Photo Style/REAL TIME LUT；它们不是中性 RAW 基础显色。[R16][R9][R52][R57][R58][R62][R66][R67][R73][R75] | LUT 格式验证、采样/插值、强度混合、输入域/色彩空间、收藏和应用顺序、末端微调；风格按机型/代际映射并保存参数，允许无厂商风格的中性基础渲染。 | `lut-cube` 版本/维护有限且许可证标为 non-standard，必须先审查；解析成功不等于色彩结果正确。没有可合法使用的机型 profile 与参考渲染时，不承诺与厂商软件逐像素相同。[R16] |
| **S8 工程和预设**（命名工程、参数、照片、人像预设） | Serde 可序列化数据结构；SQLite/rusqlite 可作本地存储候选；数据库迁移工具只迁移数据库结构。[R17][R18] | 工程格式、资源/原片路径策略、预设、保存/打开、损坏恢复、schema 与渲染语义版本迁移。 | Serde 默认值不保证 0.3 中性；数据库迁移不保证旧工程渲染一致。锁定版本后核查许可及 Windows 构建。[R17][R18] |
| **S9 导出和精修**（JPG、16 位 TIFF、双像素层 PSD、Patchy） | `image` 支持 JPEG/PNG/TIFF；`tiff` 有明确的部分 16 位路径；`ag-psd` 是 PSD/PSB 写入候选；Patchy 声称可处理 16 位 TIFF。[R19][R20][R21][R22] | 输出选项、色彩/元数据策略、目标 RGB 编码与 ICC/sRGB 标签对应、双图层生成、PSD 命名和兼容测试；Chromora 参数仍保存在工程；TIFF 交接说明。 | TIFF 16 位格式/压缩支持有边界；ag-psd 高位深映射与 Photoshop 往返尚未验证；Patchy 不是可嵌入库。必须以导出样片核对实际像素编码与嵌入 ICC/色彩标签一致。[R20][R21][R22][R47][R82][R83] |
| **P1.1 线性渐变蒙版** | `wgpu` 可承载自写解析遮罩 shader；`imageproc` 有几何/采样原语，无现成照片蒙版交互。[R11][R13][R23] | 端点、过渡宽度、原图坐标参数、局部调色、叠加预览和保存/导出一致。 | 坐标变换、预览/全尺寸羽化和色彩域需统一；GPU集成到 GPUI 未证实。[R11][R23] |
| **P1.2 径向蒙版** | 同 P1.1，底层可用自研解析椭圆覆盖率与 GPU/CPU 像素处理。[R11][R23] | 椭圆位置/尺寸、羽化、反选、拖拽交互、局部参数和持久化。 | 边缘羽化尺度与裁剪/缩放坐标必须一致；未发现可直接复用的 GPUI 径向照片蒙版控件。[R23] |
| **P1.3 蒙版管理和局部滑杆** | GPUI 可提供控件/状态承载，`wgpu`/图像缓冲可执行蒙版；没有现成完整蒙版管理器。[R5][R11][R23] | 多蒙版 ID、命名、启停、删除、覆盖预览、局部曝光/高光/阴影/色温/饱和度、工程重开和导出一致。 | 蒙版排序、混合和版本化需自定义；CPU/GPU路径及工程兼容待验证。[R23] |
| **P1.4 裁剪、旋转和拉直** | `image` 有裁剪、重采样、直角旋转；`imageproc` 有旋转、平移、投影变换。[R13][R23] | 构图框、比例预设、网格、任意小角度拉直、非破坏变换、边界/自动裁切及导出同构图。 | 基础 API 不负责编辑器手柄/比例约束；插值、填边和原片坐标映射需自测。[R23] |
| **P1.5 白平衡吸管** | LCMS 可做 ICC 转换及温度/色度低层数学；不提供照片吸管或自动中性判断。[R12] | 点击取样、取样区策略、中性估计、色温/色调回写和滑杆细调。 | 单点无法保证中性，混合光/剪切/噪声会误判；RAW工作色彩契约待定。[R12] |
| **P1.6 高光/阴影剪切提示** | 像素比较、色彩转换/WCS gamut API可作低层；没有现成成片剪切覆层。[R12][R24] | 以明确的最终渲染/显示定义检测，画布覆色、开关、不污染导出。 | 需区分编辑态剪切、显示变换与导出剪切；Windows Advanced Color profile 行为有边界。[R24] |
| **P1.7 三向色轮** | GPUI 可绘控件；LCMS 提供色彩转换基础，不提供照片三向轮语义。[R5][R12] | 色轮交互、数字输入双向同步、阴影/中间调/高光权重与参数存储。 | 色彩空间、权重曲线和色域边界需规格化与参考样片验收。[R12] |
| **P1.8 撤销/重做** | Serde/SQLite 可保存状态数据，但无证据表明提供产品级编辑命令历史。[R17][R18] | 命令/快照模型、滑杆拖动合并、曲线/蒙版一步语义、重做栈、工程重开策略。 | 历史体积、版本迁移、GPU/CPU非确定性及跨版本渲染复现需设计。[R17] |
| **P2.1 黑白混色、颗粒、暗角** | `image`/`imageproc` 有灰度、模糊等基础；spektrafilm 可作光谱模拟研究参考但非 Rust 库。[R9][R13][R25] | 八色黑白混合、颗粒大小/强度/随机种子、暗角、独立参数和渲染管线。 | 普通灰度/LUT不等于胶片模拟；颗粒需可复现，spektrafilm许可/性能需评估。[R25] |
| **P2.2 扩展风格导入**（HaldCLUT、富士/理光配方） | `image` 可读 PNG；HaldCLUT语义可参考 ImageMagick；`.cube` 解析候选见 S7。[R9][R16][R26] | Hald结构识别与采样、配方参数模型、近似映射标注、收藏/校验。 | Hald排列/色深/alpha需校验；厂商配方不是等价 LUT，官方 LUT再分发权待核。[R26][R27] |
| **P2.3 Lightroom/Camera Raw XMP 预设** | Adobe CRS/XMP 文档可作字段和值域依据；本次未核实可直接推荐的 Rust XMP 解析器。[R28][R29] | RDF/XML解析、支持字段映射、版本兼容、未映射项清单及应用参数换算。 | 字段存在不代表 Chromora 渲染等价；未映射字段不能静默丢弃。[R28][R29] |
| **P2.4 点色** | LCMS可作颜色空间转换，GPUI承载采样/控件；未发现现成点色摄影功能。[R5][R12] | 画布取样、颜色范围/容差、平滑蒙版、HSL/明度调整和边缘稳定性。 | 感知空间与颜色范围定义、皮肤/高饱和边界需样片验证。[R12] |
| **P2.5 命名版本和历史记录** | Serde/SQLite可存数据；不提供照片版本切换或可重放语义。[R17][R18] | 版本快照/命名、切换、历史关联、渲染语义版本和迁移。 | 外部原片/LUT变化、算法升级影响重放；需定义兼容目标。[R17] |
| **P2.6 照片多选和筛选** | GPUI仅承载界面；notify可发文件变化通知，不能代替可靠索引。[R5][R30] | 照片身份、星级/标记、筛选、多选状态和持久化。 | 文件监视可能漏事件；需全量扫描对账、去重和错误恢复。[R30] |
| **P2.7 按组同步调整** | 工程参数模型可作为底层；没有研究证据显示有现成照片组同步 crate。[R17] | 基础调色/曲线/HSL/风格字段映射、选择性覆盖确认；构图、蒙版、人像参数隔离。 | 误同步局部参数或版本不兼容会破坏照片独有调整；制定字段白名单。[R17] |
| **P2.8 批量导出** | Rayon可并行CPU任务；SQLite持久队列候选 apalis-sqlite；GPUI提供状态界面基础。[R5][R31] | 有界队列、取消/重试、命名冲突、逐图进度/失败原因、输出预设、恢复。 | 并行不等于有背压/进度；取消贯穿渲染写盘且输出需幂等。队列Windows适配待测。[R31] |
| **P2.9 画布内污点修复** | OpenCV `inpaint`适合邻域修复；`seamlessClone`用于源区融合；不是可移动修复点工具。[R14][R15] | 修复点/来源/半径/羽化、移动/重选/删除、撤销、代理预览和全尺寸渲染。 | inpaint适合小区域；seamlessClone有8-bit三通道等限制；伪影需真实素材验收。[R14][R15] |
| **P2.10 抠图和透明导出** | `ort`可接ONNX推理；SAM2/RMBG为候选模型，RMBG权重非商业许可限制明显。[R32][R33][R34] | 主体选择、蒙版修边/反相/羽化、背景替换、透明PNG导出与非破坏工程。 | SAM2原生Windows/ONNX未证实；RMBG 2.0权重CC BY-NC 4.0，不可直接用于商业随包。[R32][R33][R34] |
| **P3.1 更多局部工具**（笔刷、橡皮、亮度/颜色范围、蒙版布尔、自动选区候选） | `wgpu`可跑自写蒙版；OpenCV/图像处理仅提供底层原语，未证实现成照片局部工具。[R11][R14][R23][R32] | 笔刷笔迹、橡皮、范围蒙版、相加/相减/交集、自动天空/主体/对象评测和手动修边。 | 自动选区模型和权重许可/质量各异；GPU/CPU、蒙版坐标和复杂边缘要测。[R32] |
| **P3.2 镜头与透视**（手动畸变、紫边、透视；自动逐项验证） | Lensfun及Rust移植候选提供畸变/TCA/暗角及部分几何校正；Lensfun有机身/镜头数据库。[R35][R36] | 元数据读取/匹配、手动强度、紫边定义、透视交互、边界裁切、配置缺失提示。 | Lensfun官方指出镜头MakerNote匹配困难；代码LGPL-3.0、数据CC BY-SA 3.0需分别审查；Rust移植Beta且Windows未核实。[R35][R36] |
| **P3.3 人像细化**（眼袋/黑眼圈、眼白、牙齿、手动液化） | OpenCV/imageproc可作基础变换与滤波；没有核实到现成Rust液化或语义人像工具。[R14][R15] | 区域选择、受限局部修饰、液化位移场/保护区、可撤销预览与全尺寸输出。 | 肤色/身份细节、五官形变、边缘露底及塑料感是主要质量风险；须按清单样片验收。[R14] |
| **P3.4 光谱胶片与相纸模拟** | spektrafilm可作独立研究参考；SpektraLab指南仅作工作流参照，不证明代码/配置可复用。[R25][R37] | Windows渲染原型、胶片/相纸参数与配置、授权台账、性能策略。 | spektrafilm代码GPLv3、配置/LUT另有许可且示例性能较慢；SpektraLab要求macOS，资源授权待核。[R25][R37] |
| **P3.5 多张合成**（HDR、全景） | OpenCV HDR/MergeMertens、stitching/detail模块；mtb-align是有限的整数平移对齐候选。[R38][R39] | 曝光分组/参考帧、对齐诊断、ghosting/视差修复、融合/色调映射、全景投影裁切与接缝编辑。 | OpenCV教程假设已对齐且无运动；MTB不支持旋转；Windows绑定/依赖和色彩链需验证。[R38][R39] |

## GPUI 与 Windows 可行性

GPUI 上游 `main` README 说明 Windows 使用 Win32 窗口与 DirectWrite，且无需额外平台 feature；但公开发布的 `gpui` 0.2.2 文档在入门要求中仍写 macOS 或 Linux。两份资料的版本/平台表述不一致，应把 Windows 可行性视为“有上游实现线索、目标发布版待实测”，而不是无条件支持。[R1][R5][R8] GPUI 仍处 pre-1.0、活跃开发且可能有破坏性变更。具体锁定版本的 Windows CI、最低系统版本、GPU/高 DPI/输入法/触控兼容、文件对话框和拖放 API 覆盖、与 `wgpu` 的纹理/计算共享路径，均需原型验证。建议先固定版本做 Windows 原生最小原型，实测窗口、DPI、多屏、输入、文件选择/拖放、图像纹理、设备丢失和安装包构建。

**阻断门槛：在确认重做架构可行、开始依赖 GPUI 扩展开发之前，必须选定并记录准确的 GPUI 上游 Git ref（tag 或 commit SHA）、对应 crate 版本和 `Cargo.lock`。不得依赖浮动的 `main` 作为可复现基线。随后必须在原生 Windows 环境用该锁定版本提供可复现的构建证据（Windows target、Rust/MSVC 工具链、构建命令与成功日志），并实际启动该应用，验证窗口、图像打开/显示、GPU/纹理路径、文件选择、保存和导出。将 ref、锁文件、构建日志及运行验收记录关联到同一提交；缺少其中任何一项都标记为未验收并阻塞 Windows 架构通过。Linux 构建/运行仅作开发检查，不能替代这些 Windows 证据。当前研究没有替项目选定一个 ref，因此此项仍是待定门槛，不应伪称已经通过。**

许可证也须做版本级审计：crates.io 发布版 `gpui 0.2.2` 的 Cargo 元数据标注 Apache-2.0；但 Zed 仓库整体许可证不能代表每个组件及其完整依赖图。上游 issue #55470 曾记录默认依赖图可能带入 GPL-3.0 依赖的质疑；该议题目前关闭，但页面没有形成可替代法律审核的结论。发行前应针对锁定版本和实际 feature graph 检查 Cargo.lock、传递依赖与打包内容，必要时请专业人员审阅。[R44][R45]

## 相机 RAW 与色彩：架构和功能映射

### 当前阶段边界与两层色彩模型

**当前 JPG/PNG 纵切片继续推进，但明确仅覆盖 JPG/PNG 的 RGB8/sRGB 路径；RAW 解码与相机机型级色彩尚未实现。** 该切片是阶段性互操作范围，不是长期内部图像表示选型。不要把普通 sRGB 转换或 EXIF 相机型号字段当成“已支持该 RAW 机型色彩”；也不要在工程格式、渲染 API 或缓存中把 8-bit gamma 编码 sRGB 固定为不可扩展的 master。

| 层 | 责任与边界 | 主要验收对象 |
|---|---|---|
| **RAW 基础色彩** | 对具体机型与 RAW 变体解码；核对 CFA/active area、黑电平/白电平、必要线性化、白平衡、去马赛克，以及相机矩阵或适用的机型输入 profile；转换到统一的高位深、宽色域工作空间，再分别进入显示预览或导出变换。通用 sRGB 只可作为明确的显示/交付目标，不能冒充 RAW 机型显色。 | 每个机型×固件×RAW 子类型的实图；黑白电平、WB 与矩阵/profile 来源及适用性；色卡/中性轴/高光暗部；预览、导出和色彩标签。 |
| **厂商风格外观** | 与基础显色分开、由用户选择启用。首批覆盖 Canon Picture Style、Nikon Picture Control、Sony Creative Look（较早代为 Creative Style）、Fujifilm Film Simulation、OM System Picture Mode、Panasonic Photo Style/REAL TIME LUT。外观名/参数按机型与固件登记；不可假设每款机型都有同一套风格。 | 原始 RAW 的中性/基础渲染与各外观分别对照同机 JPEG或官方软件参考；将锐化、降噪、色调曲线、镜头校正等区别于矩阵/profile。无合法适用 profile 和可追溯参考时，不承诺与厂商软件逐像素相同。 |

### 解码与色彩配置的全栈候选

建议处理边界为：文件/机型/变体识别 → RAW 解码输出（CFA、尺寸/裁切、黑白电平、WB 与元数据）→ 黑电平/坏点/白电平处理及必要线性化 → 去马赛克 → 白平衡与机型矩阵或明确适用的相机输入 profile → 场景线性、高精度、宽色域工作表示 → 非破坏性编辑 → 显示预览变换或文件导出变换。相机输入 profile、显示器 profile、导出 profile 和创意外观是不同角色，不能互相替代。

- **Rust/Native 解码候选：** 现有报告中的 Rawler crate、LibRaw FFI 可继续评估；另一个 `rawloader` Rust 项目是不同项目，不应与 Rawler 混为一谈。LibRaw 有较广机型清单及 C/C++ API，但版本/编译选项仍需实图验证，Rust 边界宜用窄 C ABI 隔离并审查 LGPL/CDDL 许可。RawSpeed 是 RAW 首阶段解码组件，不负责去马赛克、WB、色彩校正或完整 metadata；不能当作完整显影引擎。先用同一语料比较候选的变体覆盖、解码正确性、稳定性、Windows 构建、性能和许可证，再锁定后端。[R3][R4][R78][R79][R80][R81]
- **ICC 与 DCP：** LCMS2 可承担 ICC 输入/输出转换；应对有 ICC、明确 sRGB、无 profile、无效 profile 的输入规定解释和可见回退。DCP 是相机 profile 机制，可含相机校准矩阵、Hue/Sat/Look 表与 tone curve；不等于厂商专有风格，也不等同任意 ICC profile。矩阵型基础显色与 DCP 内的 look/tone 组件应能分开处理。profile 必须记录来源、版本、机型适用性和再分发许可；缺 profile 时只能如实标注采用的 fallback 与验证级别。[R12][R47][R82][R83][R84][R85]
- **长期内部表示候选：** 评估场景线性 float32 RGB 与线性宽色域候选（如 linear Rec.2020、ACEScg/AP1），同时考虑瓦片/金字塔与大图内存；XYZ 可用于连接空间/交换，但暂不建议未经编辑算子验证就作为默认编辑 RGB。上述均为候选而非已选型。当前 RGB8/sRGB 可继续用于第一阶段输入/预览/交付范围，但应在内部接口保留后续高精度转换空间。[R11][R43][R82][R84]
- **预览/导出与 Windows 显示：** 编辑状态与颜色变换定义应共享，显示端和文件端只在目标变换处区分；导出像素值必须与嵌入 ICC 或 sRGB/色彩空间标签相符。GPUI 窗口/纹理不自动证明显示器 ICC、HDR/SDR 或多显示器色彩路径正确，Windows WCS/Advanced Color、GPU surface 与实际导出仍需目标 Windows 机器实测。[R24][R47][R82][R83][R86]

### 首批相机机型覆盖矩阵（候选，不代表已通过）

首批以六个主流系统覆盖常见机型、画幅与代际。矩阵中的“预期/待验证”是待采实图核对项；库的机型名单只作为解码线索，不等于 Chromora 验收通过。

| 品牌/建议机型 | RAW 格式与首批变体 | 样片与参考渲染 |
|---|---|---|
| **Canon：EOS R5 Mark II、EOS R7、EOS 5D Mark IV** | R5 Mark II 官方确认静态 RAW/C-RAW 为 `.CR3`；R7 确认 RAW/C-RAW，扩展名以实机验证；5D IV 为 `.CR2`，含 RAW/M-RAW/S-RAW。按实际机型分测支持的压缩/尺寸变体。[R49–R52] | 每机同光源 RAW+机内 JPEG；R5 II/R7 分别覆盖可用 C-RAW。记录 Picture Style 全参数、WB、曝光、固件；用同源 Canon DPP 输出作参考，区分外观、降噪/锐化与基础显色。 |
| **Nikon：Z8、Z6 II、D850、D7500** | Z8 为 `.NEF`、14-bit，可选无损压缩/HE/HE*；LibRaw 0.22 清单指出 Z8 HE/HE* 尚不支持。D850 为 `.NEF` 12/14-bit 多种压缩，D7500 为 `.NEF` 12/14-bit 压缩/无损压缩。Z6 II 变体需官方规格和实机补证。[R53–R57] | 每机 RAW+机内 JPEG，分别记录 Picture Control、Active D-Lighting、WB 与曝光；色卡/肤色/灰卡、低/高 ISO。以同机 JPEG 与同源 NX Studio 输出作参考；不要拿 Z8 HE/HE* 未支持状态推断其他解码器。 |
| **Sony：ILCE-7M5（α7 V）、ILCE-7M4（α7 IV）、ILCE-6400、ILCE-7M3（α7 III）** | A7 V 官方列 ARW 6.0、Compressed (HQ)/Lossless Compressed；A7 IV 为 ARW 4.0，压缩/无损/未压缩；A6400 与 A7 III 为 ARW 2.3。扩展名/实际变体和固件行为以文件样本核验。[R59–R62] | 同机 RAW+JPEG；A7 V/A7 IV 分开采集各 RAW 记录模式；记录 Creative Look（较旧机型 Creative Style）、WB、DRO/降噪与输出标签。用机内 JPEG及 Imaging Edge Desktop（官方入口，具体机型兼容与输出设置逐项核对）作有版本记录的参考。[R63] |
| **Fujifilm：X-T5、X-T3、GFX100 II、X100V** | X-T5 `.RAF`、14-bit，Compressed/Lossless Compressed/Uncompressed；X-T3 为 14-bit RAF；GFX100 II 为 14/16-bit RAF，含多类压缩；X100V 扩展名/变体需实机复核。[R64–R68] | 每机、每种 RAW 变体采集 RAF+JPEG；记录 Film Simulation、WB、DR、色调与风格参数。以同机 JPEG 及 X RAW Studio 同源处理作参考；不能把新机型的 Film Simulation 或参数集外推至旧机。 |
| **OM System/Olympus：OM-1 Mark II、E-M1 Mark III、E-M5 Mark II** | OM-1 II 常规 RAW 为 `.ORF`；High Res 有 12/14-bit ORF 与 `.ORI` 先拍帧，须分型测试。E-M5 II 手册确认 `.ORF`；E-M1 III 的扩展名/位深仍需实机核验。[R69–R73] | 每机 RAW+JPEG；OM-1 II 另测常规 RAW、High Res ORF/ORI。记录 Picture Mode/Art Filter、WB 与参数；以同机 JPEG 和 OM Workspace 同源输出作参考。 |
| **Panasonic Lumix：DC-S5II、DC-GH7、DC-G9II、DMC-GH4** | `.RW2` 为预期格式，但已查官方资料未逐机确认后缀/压缩变体，全部待实机核验。S5II 手册确认常规 14-bit、连拍 12-bit；GH4 官方规格列 RAW 与 RAW+JPEG。[R74–R77] | 每机 RAW+JPEG；色卡/肤色/红蓝/灰卡、WB与低/高 ISO。记录 Photo Style/REAL TIME LUT 与固件；可用同机 JPEG及 S5II 对应的 SILKYPIX 处理作参考，不把 LUT 与基础色彩混同。 |

### 调研、全栈与测试交付门槛

| 角色/工作流 | 必须交付与通过条件 |
|---|---|
| **调研员：机型覆盖** | 将上表落实为样片清单：具体机型、固件、RAW 子格式/位深/压缩、扩展名、CFA/尺寸、官方参考渲染器版本及来源；每项明确“已官方核实/仅预期/待实机”。取得合法原片与许可记录；附同机 JPEG、色卡/肤色/红蓝/灰卡样片及 EXIF/MakerNote。 |
| **全栈：解码与色彩配置** | 比较并锁定解码器版本/构建/许可证；建立解码器接口与统一高精度内部帧契约。明确黑/白电平、WB、矩阵/DCP/ICC profile 的来源、适用性与 fallback；区分 RAW 基础显色、厂商外观、显示变换、导出编码/标签。当前 RGB8/sRGB 切片继续，但不固化为长期 master。 |
| **测试员：机型级验收** | 逐机型×固件×变体验收真实图像：RAW 解码/尺寸/CFA/黑白电平；WB（自动、预设、固定色温/自定义）；矩阵或 profile 来源及中性轴/色卡；预览与导出一致；目标色彩空间、实际像素编码与 ICC/标签一致。记录可复现样片、工具版本、误差容差与失败回归。 |

**最小测试门槛：**（1）同一型号的每个 RAW 变体须有原始文件、哈希与来源；（2）曝光阶梯、灰卡/色卡、肤色、高饱和红蓝、低/高 ISO，另有自动/预设/自定义 WB；（3）黑/白电平、矩阵/profile、色调曲线/外观分层可追溯且异常不静默套通用 sRGB；（4）以同机 JPEG/官方 RAW 软件为参考并记录机身/固件/软件/输出配置，不要求逐像素相同；（5）同一工程状态的预览与导出通过数值/色差容差，重开导出文件验证；（6）检查 sRGB、宽色域 ICC、无 profile、错误 profile 的输入，以及导出 ICC/色彩标记与实际 RGB 数值的对应；（7）损坏/恶意文件、内存/性能、Windows 构建、线程安全与依赖许可证均需独立验收。

## 四项高风险

- **RAW**：Rawler、独立的 `rawloader` Rust 项目、LibRaw FFI 与 RawSpeed 是不同候选/组件，均不能只凭扩展名清单视为完整显影引擎。逐机型/固件/压缩变体验证解码、黑白电平、WB/矩阵、metadata 与高位深输出；许可和 Windows 部署须版本级审查。[R3][R4][R78][R79][R80][R81]
- **PSD**：`ag-psd` 声明支持RGB 8/16/32位写入，但像素API和高位深映射存在限制；双像素层PSD不是通用Photoshop文档支持。先验证目标像素缓冲到PSD的位深/alpha/ICC，以及 Photoshop/Patchy 往返。[R21][R22]
- **色彩管理**：LCMS可提供ICC转换，但不自动定义 RAW 显影、相机 profile、厂商 look 或内部工作空间；DCP/ICC 也不能不加区分地互换。Windows Advanced Color/WCS 行为有条件限制；预览、导出与 ICC/色彩标签须用色彩样片逐项测试。当前 RGB8/sRGB 仅为早期 JPG/PNG 切片。[R12][R24][R47][R82][R83][R85][R86]
- **性能**：`image`高层解码可能整图入内存；浮点缓冲、金字塔、蒙版、双缓冲与导出会放大内存压力。Rayon并行不提供背压，GPU UI加速不等于图像处理已GPU化。应先测目标RAW/大图的峰值内存、缩放/拖动响应、并发导出及GPU失效回退。[R9][R11][R31][R43]

## 建议先后顺序

1. **先过GPUI/Windows阻断门槛**：选定并记录准确 GPUI Git ref、crate 版本和 `Cargo.lock`；在原生 Windows 上用该锁定版本产出可复现的构建日志和真实启动/打开图像/纹理显示/文件选择/保存导出证据。Linux 构建不能代替。再选定图像I/O与存储格式，并完成该锁定依赖图的许可证审计。
2. **当前 JPG/PNG 纵切片继续，另行证伪 RAW/色彩高风险**：现有切片作为 RGB8/sRGB 的阶段性 JPG/PNG 路径推进，不宣称 RAW 已支持，也不定为长期内部 master。安排独立 RAW 样片原型，比较解码后端、线性/高位深与 profile 配置、预览→导出；并行做 PSD 双层往返、GPUI+wGPU 纹理路径及超大图内存/响应基准。未过实图门槛不得宣传机型色彩支持。
3. **再做基础产品链**：工程/版本化参数、撤销重做、单张调色、裁剪/蒙版、JPG/TIFF 输出和画布工作流。内部数据模型保留切换高位深工作空间的能力，预览与导出共用处理语义。
4. **再扩展批量与风格**：照片筛选/同步/批量任务、XMP/Hald/LUT；所有导入器报告未映射内容。
5. **最后接入高复杂度能力**：抠图、AI、镜头自动匹配、光谱胶片、HDR/全景及修复工具；按实际素材与授权决定是否发布。

## 证据不足与待验证

- GPUI最新Windows支持级别、锁定版本稳定性、安装发布链和独立crate许可边界未由现有证据完全确认。
- 多个研究明确未验证Windows/MSVC完整构建、目标机驱动/色彩表现和真实素材性能；候选不代表选型结论。
- `image`、Rawler、LibRaw、OpenCV等的具体格式/机型支持仍需按Chromora目标样片逐项验证；不承诺“能读该格式名”即能处理全部相机文件。
- 本报告给出的六品牌机型是首批候选矩阵，不代表已经下载/验收真实样片；未实测的后缀、位深、压缩变体、profile 与官方软件输出均按矩阵标注为待验证。
- 厂商风格名与可调项随机型/代际/固件变化；没有合法可用 profile 与对照样片时，只能把自研映射称为近似外观，不能承诺与厂商软件逐像素相同。
- XMP字段映射、Hald布局、LUT采样与外部配方结果需要代表性文件测试；厂商公开下载不等于允许随软件再分发。
- 模型代码许可与模型权重/训练数据许可必须分开核实；BRIA RMBG权重非商业限制、LaMa权重/许可未核，不得默认为可商用打包。
- 原始清单有六条 P3 待办，本报告按原文顺序映射。

## 来源表

| 引用 | 标题 | 完整 URL | 支持的判断 |
|---|---|---|---|
| R1 | Welcome to GPUI — zed-industries/zed crates/gpui README | https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md | GPUI定位、Windows Win32/DirectWrite、pre-1.0状态；没有文件选择器或照片编辑承诺。 |
| R2 | Rust for Windows, and the windows crate — Microsoft Learn | https://learn.microsoft.com/en-us/windows/dev-environment/rust/rust-for-windows | Rust可调用Win32/Direct3D/传统窗口等API；绑定不是产品对话框。 |
| R3 | rawler v0.8.0 — crates.io | https://crates.io/crates/rawler | Rawler定位、版本、LGPL-2.1及Rust版本要求。 |
| R4 | LibRaw — GitHub README | https://github.com/LibRaw/LibRaw | RAW数据/元数据/预览能力及非完整生产显影边界；许可信息。 |
| R5 | Crate gpui — Rust | https://docs.rs/gpui/latest/gpui/ | Render/Elements、Actions、测试和pre-1.0说明。 |
| R6 | Interactivity in gpui — Rust | https://docs.rs/gpui/latest/gpui/struct.Interactivity.html | 鼠标按下/移动/抬起等交互事件。 |
| R7 | Image — GPUI Component | https://gpui-kit.com/component/image | 图像显示、加载/失败占位和图库样例，不是照片编辑画布。 |
| R8 | Module viewer — iced::widget::image::viewer | https://docs.rs/iced/latest/iced/widget/image/viewer/ | Iced图像Viewer的缩放/平移；非GPUI插件。 |
| R9 | Crate image — Rust | https://docs.rs/image/latest/image/ | Rust图像编解码、像素缓冲及基础操作定位。 |
| R10 | silvia-odwyer/photon | https://github.com/silvia-odwyer/photon | 通用像素算子候选及其边界。 |
| R11 | gfx-rs/wgpu | https://github.com/gfx-rs/wgpu | GPU API、Windows DX12/Vulkan支持；不含摄影算法。 |
| R12 | lcms2 v6.2.0 — crates.io | https://crates.io/crates/lcms2 | ICC profile转换、线程上下文、版本/许可证及Rust门槛。 |
| R13 | image-rs/imageproc | https://github.com/image-rs/imageproc | 通用处理、线性RGB假设、许可证等。 |
| R14 | opencv Rust bindings — crates.io | https://crates.io/crates/opencv | Rust绑定版本、MIT、Windows支持和API稳定性自述。 |
| R15 | Image Inpainting / Seamless Cloning — OpenCV 4.13 docs | https://docs.opencv.org/4.13.0/df/d3d/tutorial_py_inpainting.html ; https://docs.opencv.org/4.13.0/df/da0/group__photo__clone.html | 邻域修复与Poisson融合的算法边界和输入限制。 |
| R16 | lut-cube — crates.io | https://crates.io/crates/lut-cube | `.cube`解析范围、版本与non-standard许可标记。 |
| R17 | Serde Overview / Field attributes | https://serde.rs/ ; https://serde.rs/field-attrs.html | 序列化框架及字段默认/别名能力；不提供完整迁移。 |
| R18 | rusqlite / rusqlite_migration | https://crates.io/crates/rusqlite ; https://github.com/cljoly/rusqlite_migration | SQLite访问、bundled构建及数据库schema迁移职责。 |
| R19 | Module codecs — image 0.25.10 | https://docs.rs/image/latest/image/codecs/index.html | JPEG、PNG、TIFF编码/解码支持。 |
| R20 | tiff v0.11.3 — crates.io | https://crates.io/crates/tiff | 部分16位TIFF样本类型、压缩能力和限制。 |
| R21 | ag-psd v0.3.0 — crates.io | https://crates.io/crates/ag-psd | PSD/PSB读写、图层与RGB位深限制、许可证。 |
| R22 | Features and compatibility — Patchy | https://github.com/SethRobinson/Patchy/blob/main/docs/features.md | Patchy自述16位TIFF和PSD/PSB能力；不证明Chromora互操作。 |
| R23 | imageproc geometric_transformations | https://docs.rs/imageproc/latest/imageproc/geometric_transformations/index.html | 旋转、平移、投影warp与插值基础API。 |
| R24 | ICC profile behavior with Advanced Color — Microsoft Learn | https://learn.microsoft.com/en-us/windows/win32/wcs/advanced-color-icc-profiles | Windows Advanced Color下ICC行为、helper限制。 |
| R25 | spektrafilm README | https://github.com/andreavolpato/spektrafilm | 光谱胶片/相纸模拟参考、性能自述及代码/配置许可边界。 |
| R26 | ImageMagick `-hald-clut` | https://imagemagick.org/command-line-options/ | Hald是3D RGB立方的2D表示及查表语义。 |
| R27 | FUJIFILM 3D-LUT downloads | https://www.fujifilm-x.com/en-us/support/download/lut/ | 官方提供LUT下载/导入说明；未给出再分发许可。 |
| R28 | Camera Raw namespace — Adobe | https://developer.adobe.com/xmp/docs/xmp-namespaces/crs/ | CRS字段名、类型和值域。 |
| R29 | Overview of XMP technology / XMP Specification — Adobe | https://developer.adobe.com/xmp/docs/xmp-specifications/ | XMP数据模型和序列化规范，不保证渲染等价。 |
| R30 | notify crate documentation | https://docs.rs/notify/ | 文件监视及漏事件/保存语义限制。 |
| R31 | Rayon / apalis-sqlite | https://docs.rs/rayon/latest/rayon/ ; https://github.com/apalis-dev/apalis-sqlite | 并行迭代与持久任务队列候选；需自行做产品任务状态。 |
| R32 | ort — GitHub README | https://github.com/pykeio/ort | Rust ONNX Runtime接入层定位。 |
| R33 | ONNX Runtime DirectML Execution Provider | https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html | DirectML支持边界、限制与并发条件。 |
| R34 | BRIA RMBG 2.0 model card | https://huggingface.co/briaai/RMBG-2.0 | alpha输出、模型用途及CC BY-NC 4.0限制。 |
| R35 | Lensfun official usage / database docs | https://lensfun.github.io/usage/ ; https://lensfun.github.io/manual/v0.3.2/database.html | Lensfun是应用调用的校正库/数据库；镜头识别困难且由应用负责。 |
| R36 | Lensfun / lensfun Rust crate | https://github.com/lensfun/lensfun ; https://crates.io/crates/lensfun | 校正种类、LGPL与数据库CC BY-SA许可；Rust移植Beta/Windows待核。 |
| R37 | SpektraLab 中文使用指南 | https://spektralab.app/user_guide/zh/ | 公开指南所述工作流和macOS平台边界；不证明资源授权。 |
| R38 | High Dynamic Range Imaging — OpenCV 4.13 | https://docs.opencv.org/4.13.0/d3/db7/tutorial_hdr_imaging.html | Debevec/Mertens流程及已对齐、无运动假设。 |
| R39 | opencv::stitching — Rust docs / mtb-align | https://docs.rs/opencv/latest/opencv/stitching/index.html ; https://crates.io/crates/mtb-align | 全景模块原语与MTB整数平移对齐限制。 |
| R40 | printpdf — crates.io | https://crates.io/crates/printpdf | PDF页面、图像、字体等低层生成能力。 |
| R41 | Real-ESRGAN / GFPGAN | https://github.com/xinntao/Real-ESRGAN ; https://github.com/TencentARC/GFPGAN | 超分/人脸修复候选、分块和身份变化风险。 |
| R42 | LaMa — GitHub README | https://github.com/advimman/lama | 遮罩重绘候选；许可与权重来源未核实。 |
| R43 | image_pyramid — docs.rs | https://docs.rs/image-pyramid | 从内存图像生成金字塔API；非按需瓦片缓存。 |
| R44 | gpui 0.2.2 — published Cargo.toml on docs.rs | https://docs.rs/crate/gpui/latest/source/Cargo.toml | 发布版号、Apache-2.0 crate 元数据与 feature 配置；应与当前上游 main 区分。 |
| R45 | GPUI dependency-license concern — Zed issue #55470 | https://github.com/zed-industries/zed/issues/55470 | 记录对 GPL 传递依赖的报告与讨论；是待审计风险线索，不是法律结论。 |
| R46 | kamadak/exif-rs — README | https://github.com/kamadak/exif-rs | 纯 Rust EXIF parser 支持的容器范围；不等于完整 MakerNote/ICC 产品支持。 |
| R47 | ICC: Embedding ICC profiles in image file formats | https://www.color.org/profile_embedding/ | JPEG/TIFF 等 ICC 嵌入需按 ICC Annex B 处理；ICC 与 EXIF 是不同元数据路径。 |
| R48 | ExifTool EXIF tags and MakerNotes | https://exiftool.org/TagNames/EXIF.html ; https://exiftool.org/ | 标准 EXIF 标签、方向等字段；ExifTool 覆盖多厂商 MakerNotes，同时承认仍有未解码字段。 |
| R49 | Canon EOS R5 Mark II 官方手册 | https://cam.start.canon/en/C017/manual/html/UG-10_Reference_0110.html | 全画幅、静态 RAW/C-RAW 的 `.CR3`、图像尺寸/变体信息。 |
| R50 | Canon EOS R7 官方规格 | https://www.canon-europe.com/cameras/eos-r7/specifications/ | APS-C、32.5MP、14-bit RAW/C-RAW、Picture Style；页面未直接确认扩展名。 |
| R51 | Canon EOS 5D Mark IV 官方手册 | https://gdlp01.c-wss.com/gds/0/0300024970/01/eos5d-mk4-im-en.pdf | `.CR2`、RAW/M-RAW/S-RAW 与相机规格。 |
| R52 | Canon DPP 与 Picture Style 官方说明 | https://www.canon-europe.com/get-inspired/tips-and-techniques/editing-raw-images-in-dpp/ ; https://www.canon-europe.com/software/digital-photo-professional/ | Picture Style 可作用于 RAW 并在 DPP 中切换；DPP 是 EOS RAW 处理软件。 |
| R53 | Nikon Z8 官方规格 | https://onlinemanual.nikonimglib.com/z8/en/specifications_378.html | 14-bit NEF、无损压缩及 HE/HE* 记录选项。 |
| R54 | Nikon D850 官方规格 | https://imaging.nikon.com/imaging/lineup/dslr/d850/ | NEF 12/14-bit、多种压缩与 RAW 尺寸选项。 |
| R55 | Nikon D7500 官方规格 | https://imaging.nikon.com/imaging/lineup/dslr/d7500/ | DX 机型、NEF 12/14-bit 与压缩选项。 |
| R56 | LibRaw 支持相机清单（0.22） | https://www.libraw.org/supported-cameras | 相机型号级解码线索；清单指出 Z8 HE/HE* 尚未支持，不代表所有格式变体通过。 |
| R57 | Nikon NX Studio 官方说明 | https://www.nikonusa.com/content/nx-studio | NEF/NRW 处理及 Picture Control、WB、曝光等调节，可作为厂商参考渲染入口。 |
| R58 | Nikon Picture Control 官方指南 | https://imaging.nikon.com/imaging/support/digitutor/d7500/functions/picturecontrol.html | Picture Control 预设及参数说明。 |
| R59 | Sony ILCE-7M5（α7 V）官方规格 | https://www.sony.com/electronics/support/e-mount-body-ilce-7-series/ilce-7m5/specifications | ARW 6.0、RAW Compressed (HQ)/Lossless Compressed 记录模式。 |
| R60 | Sony ILCE-7M4（α7 IV）官方规格 | https://www.sony.com/electronics/support/e-mount-body-ilce-7-series/ilce-7m4/specifications | ARW 4.0、压缩/无损/未压缩 RAW 与 Creative Look 名称。 |
| R61 | Sony ILCE-6400 与 ILCE-7M3 官方规格 | https://www.sony.com/electronics/support/e-mount-body-ilce-6000-series/ilce-6400/specifications ; https://www.sony.com/electronics/support/e-mount-body-ilce-7-series/ilce-7m3/specifications | ARW 2.3 代际与各自静态 RAW/Creative Style 规格。 |
| R62 | Sony Creative Look 官方说明 | https://www.sony.com/electronics/support/articles/00273574 | Creative Look 的预设、可调项与 Picture Profile 的用途区别。 |
| R63 | Sony Imaging Edge Desktop 官方软件入口 | https://www.sony.com/electronics/support/e-mount-body-ilce-7-series/ilce-7sm3/software/00290423 | Sony 官方软件页重定向至 Imaging Edge Desktop 下载/说明入口；具体兼容机型和渲染一致性须另行核实。 |
| R64 | Fujifilm X-T5 官方规格 | https://www.fujifilm-x.com/en-us/products/cameras/x-t5/specifications/ | 14-bit RAF、Compressed/Lossless Compressed/Uncompressed RAW 等记录选项。 |
| R65 | Fujifilm X-T3 与 GFX100 II 官方规格 | https://www.fujifilm-x.com/en-us/products/cameras/x-t3/ ; https://www.fujifilm-x.com/global/products/cameras/gfx100-ii/specifications/ | X-T3 的 14-bit RAF；GFX100 II 的 14/16-bit RAF 与 RAW 变体。 |
| R66 | Fujifilm X RAW Studio 官方软件说明 | https://www.fujifilm-x.com/en-us/support/download/software/x-raw-studio/ | 使用相机处理器转换 RAF、保留相机处理特性；机型及版本支持按官方列表核查。 |
| R67 | Fujifilm Film Simulation 官方介绍 | https://www.fujifilm-x.com/en-us/products/film-simulation/ | Film Simulation 官方外观体系及名称。 |
| R68 | LibRaw 支持相机清单（Fujifilm 条目） | https://www.libraw.org/supported-cameras | X-T3、X-T5、X100V、GFX100 II 等型号的解码清单线索，不证明完整变体/风格渲染。 |
| R69 | OM SYSTEM OM-1 Mark II 官方产品页 | https://explore.omsystem.com/c/en/om-1-mark-ii | 机身产品定位与 High Res Shot 功能介绍；详细 RAW 位深/ORF/ORI 以 R70 规格和 Q&A 为准。 |
| R70 | OM-1 Mark II 官方规格与 RAW 格式 Q&A | https://my.omsystem.com/consumer/product_specs/SPECIFICATIONS_om-1-mark-ii_EN.pdf ; https://support.jp.omsystem.com/en/support/imsg/digicamera/qa/products/om1m2/ | 常规 ORF、高分辨率 ORF/ORI、12/14-bit 及相关模式差异。 |
| R71 | Olympus E-M5 Mark II 官方手册 | https://download.omsystem.com/pages/inst/em5m2/manual_em5m2_ENU.pdf | `.ORF` 文件扩展名与 Picture Mode 说明。 |
| R72 | OM Workspace 官方软件说明 | https://software.omsystem.com/omworkspace/ | RAW 处理定位、处理限制及机型支持线索。 |
| R73 | OM-1 Mark II 官方 Picture Mode / High Res 手册 | https://learning.omsystem.com/OM-1MarkII/zz_html_manual/en/picture_mode_131.html ; https://learning.omsystem.com/OM-1MarkII/zz_html_manual/en/high_res_shot_141.html | Picture Mode 外观层及 High Res Shot 的 ORI 先拍帧说明。 |
| R74 | Panasonic LUMIX S5II 官方手册 | https://help.na.panasonic.com/wp-content/uploads/2023/02/DCS5M2_DVQP2839ZA_ENG.pdf | RAW 位深（一般 14-bit、连拍 12-bit）、尺寸/比例、sRGB/Adobe RGB、Photo Style 与 SILKYPIX。 |
| R75 | Panasonic LUMIX Lab 官方说明 | https://www.panasonic.com/global/consumer/lumix/lumix-lab.html | 兼容机型、REAL TIME LUT 与创作者 LUT 生态。 |
| R76 | Panasonic LUMIX Lab RAW/照片传输格式说明 | https://av.jpn.support.panasonic.com/support/global/cs/soft/lumix_lab/en/cts/b1.html | 传输格式包括 RW2/JPEG/HEIF；不等于逐机型第三方解码器兼容证明。 |
| R77 | Panasonic LUMIX GH4 官方规格 | https://help.na.panasonic.com/answers/features-and-specifications-lumix-g-series-dmc-gh4/ | RAW、RAW+Fine/Standard 记录选项与机身规格。 |
| R78 | RawLoader Rust 项目 README（与 Rawler 不同项目） | https://github.com/pedrocr/rawloader | RAW/CFA、黑白电平、WB、相机矩阵及项目许可/状态；并非完整显影链。 |
| R79 | LibRaw 官方文档与 API notes | https://www.libraw.org/docs ; https://www.libraw.org/docs/API-notes.html | C/C++ 读取/解包接口、mosaic 数据和输出说明。 |
| R80 | LibRaw 支持相机清单与许可资料 | https://www.libraw.org/supported-cameras ; https://www.libraw.org/docs | 按版本/编译配置核对相机支持；LGPL-2.1/CDDL-1.0 许可路线需审阅。 |
| R81 | RawSpeed 官方文档与仓库 | https://darktable-org.github.io/rawspeed/ ; https://github.com/darktable-org/rawspeed | 首阶段 RAW 解码职责边界、不含去马赛克/WB/色彩校正/完整 metadata；构建与 LGPL 线索。 |
| R82 | ICC 官方规范入口 | https://www.color.org/icc_specs2/ | ICC profile 机制与 ICC.1:2022 v4.4 规范信息。 |
| R83 | Little CMS 官方项目 | https://github.com/mm2/Little-CMS | LCMS2 ICC 色彩管理引擎及其 profile/许可说明。 |
| R84 | RawTherapee 色彩管理文档 | https://rawpedia.rawtherapee.com/Color_Management | 相机输入 profile、矩阵/DCP 双光源、tone curve 与 Base/Look Table 的区分。 |
| R85 | Adobe Digital Negative 官方说明 | https://helpx.adobe.com/camera-raw/desktop/dng-and-file-formats/digital-negative.html | DNG 规范/SDK 与专利许可、notice 条件；具体 DCP tag 仍须按规范核对。 |
| R86 | Microsoft Windows Color System | https://learn.microsoft.com/en-us/windows/win32/wcs/windows-color-system | Windows WCS/ICM API 背景；不证明 GPUI/GPU 窗口表面已自动色彩管理。 |
