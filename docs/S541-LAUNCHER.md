# s541控制按钮

系统美化 → s541控制按钮，默认关闭。仅替换官方“布局切换”按钮；底部三个图形按钮分别选择官方纯图标、图标文字、列表布局。开关保存至 `/data/chaos/s541-launcher.cfg`，布局偏好由官方接口保存和读取。独立启用蜂窝时暂停显示控制条，不修改蜂窝或其他美化设置。

三种布局均由固件创建和维护。Chaos不接管网格class、layout回调、输入或惯性，不创建应用图标或名称标签，不改图标坐标、大小、名称字体、排序和间距。

附加层不可点击、不可滚动，只容纳控制条。位置取自官方布局按钮的实际矩形中心，不推算应用行列。原按钮仅设为透明并取消点击，保留官方HIDDEN标记、坐标与布局占位；关闭后恢复原透明度和点击标记。

保留原生网格、图标和背景绘制链，包含Corona与旧版模糊的DRAW订阅。

## 参考来源

用户提供目录：`/Users/ikun_cxkpro/Downloads/Vela_NativeApp-syspatch 4/`。

- `ELF/src/ui/launcher/o62_q63_launcher.c`：原版三个透明命中区、原生事件分发、三种视图及按钮素材选择。
- `ELF/src/ui/launcher/q63_selector.c`：视觉按钮0/1/2映射原偏好0/2/1。
- `ELF/src/ui/launcher/q63_selector_assets.inc`：三个原版按钮合成状态，RLE白色预乘alpha。
- `ELF/research/launcher/selector_assets_manifest.json`：原始按钮背景及选中遮罩的来源和摘要。
- `ELF/research/launcher/q63_selector_port_202611B4.md`：466×466参考平台的位置、材质和行为记录。

当前控制条使用用户补充的RW7资源：`固件修改/rw7/4.100.080/out-8ba7ea907e46be7e1ff9a2ccb3192909_upd_miwear.watch.q65/resource_png/00_vela_resource/resource/app/launcher/1`。原始PNG保存在`supervisor/assets/s541/source/`，包括btn_bg及三类selected/deselected图标。背景alpha范围0～138。

`tools/import_s541_selector.py supervisor/assets/s541/source` 合成三个状态，使用Lanczos缩放与预乘BGRA，避免旧版最近邻缩放产生的毛边。原始背景透明遮罩另外保存在background.alpha，供玻璃层裁切。

控制条完全可见且滚动停止后，先隐藏控制条等待两次UI更新，再经GPU空闲及平衡电源锁的fence检查，从`/dev/fb0`局部采样72×24 BGR背景。三轮可分离模糊后双线性放大至288×96，并应用原PNG轮廓与最高160的alpha。使用独立双缓冲、复用前清理旧解码缓存及cache clean，不借用Corona图像描述符，也不强制开启系统模糊。采样失败三次则保留透明PNG控制条并记录日志，不阻断布局切换。此为静态局部玻璃，非每帧实时模糊。

滚动和布局切换后的定位完全由官方launcher处理，控制条跟随原按钮位置。未移植参考机型466像素圆屏的分段位移缩放动画。

## 3.101.043固件核对

- launcher网格全局槽：`0x201102EC`，class `0x2CD2024C`；原生根：`0x201102F4`。
- 网格实例`+0x5C`保存原布局按钮，该按钮是网格的同级子对象；解引用前核对父对象子表。
- 布局偏好读取：`0x0C514361`；切换：`0x0C514301(preference, 1)`。视觉按钮0/1/2对应偏好0/2/1。切换接口调用固件KV保存，成功后发送`(0x10000, 1, 1)`刷新通知；不合成事件。
- 图片对象`+0x34`为当前图像源，`+0x44`为原图宽度；使用现有p67图像工厂、设源、枢轴和缩放接口。
- `app_lookup`：`0x0CA69935`，注册表节点`+0x10`为u16应用编号。
- `launcher_data_reload_app_name`：`0x0C514284`，`0x0C5142BE`读取注册节点`+0x1C`的显示名函数并调用。该回调也可能属于安装在RAM中的原生模块，不能仅允许flash指针。
- 名称回调签名必须保留应用节点参数：`name(node)`。原生`BLX`时R0仍为lookup返回的节点；不能因为某些回调不读取R0，就将所有名称回调声明为无参数函数。
- 用户配置及界面维护在Chaos现有UI回调执行；点击调用官方切换接口，下一维护拍同步控制条高亮。
- 控件DELETE时立即作废自有视图句柄；系统重建时重新查找官方按钮。创建不完整时销毁自有视图，保留系统桌面。

## 验证

`rustc --edition=2021 --test supervisor/tests/s541_harness.rs -o /tmp/chaos-s541-tests && /tmp/chaos-s541-tests`

另运行 `python3 tools/test_honeycomb.py` 和 `python3 tools/build.py`。主机测试不证明设备上的布局、触摸和GPU效果，真机仍需验证三种视图、底部按钮、点击启动与关闭还原。

`tools/probe_s541_preference_abi.py`使用Unicorn执行3.101.043原始布局getter/setter指令，仅替代KV和通知接口，验证三种偏好的保存、读取、通知及重复选择不写入。需要安装Unicorn。

## 2026-11-14 崩溃修复

设备日志 `dfx/note/20261114184215_2755_114_10328.ev.gz` 的BusFault回溯包含`0x0C549636`，其后为Chaos模块地址。该固件名称回调入口`0x0C549634`将R0复制到R3，`0x0C549638`读取`[R3+0x30]`，继续沿`+8`、`+4`解析名称上下文。无参数调用丢失了这个必需的应用节点。

恢复为带节点参数的调用。`tools/probe_s541_name_abi.py`在Unicorn执行该固件原始指令：R0=0重现未映射读取失败；传入模拟的合法应用节点则完成上下文解析、返回正确名称指针。资源字符串解析函数是显式测试替身，此测试验证实际名称回调的参数ABI，不代表真机验收。

当前按钮独立实现已移除名称查询和附加名称标签，不再进入上述名称调用链。
