# s541控制按钮

系统美化 → s541控制按钮，默认关闭。开启后接管 launcher 的应用视图并隐藏原“布局切换”按钮；列表下方三个原版图形按钮分别选择纯图标、图标文字、列表。关闭后恢复原生网格和原布局按钮的隐藏状态。设置与所选视图保存至 `/data/chaos/s541-launcher.cfg`，与蜂窝视图互斥。

本实现保留系统应用枚举顺序、当前图标源、名称回调及原生启动回调。视图克隆显示控件，不修改系统应用注册表或固件代码。排序编辑暂沿用关闭此功能后的原生桌面。没有移植参考固件的地址或修改任何固件镜像。

原网格自身保持可绘制，仅隐藏其应用图标，确保Corona与旧版背景的DRAW订阅继续工作；关闭时还原原图标的隐藏状态，而非隐藏整棵网格导致背景模糊一起消失。

## 参考来源

用户提供目录：`/Users/ikun_cxkpro/Downloads/Vela_NativeApp-syspatch 4/`。

- `ELF/src/ui/launcher/o62_q63_launcher.c`：原版三个透明命中区、原生事件分发、三种视图及按钮素材选择。
- `ELF/src/ui/launcher/q63_selector.c`：视觉按钮0/1/2映射原偏好0/2/1。
- `ELF/src/ui/launcher/q63_selector_assets.inc`：三个原版按钮合成状态，RLE白色预乘alpha。
- `ELF/research/launcher/selector_assets_manifest.json`：原始按钮背景及选中遮罩的来源和摘要。
- `ELF/research/launcher/q63_selector_port_202611B4.md`：466×466参考平台的位置、材质和行为记录。

`tools/import_s541_selector.py` 将上述原始360×120素材缩放至288×96预乘BGRA，嵌入三份不可变图像，切换时不改写仍可能被绘制队列使用的像素。未使用替代字符图标。保留素材原有来源；本次移植不声称拥有原始素材版权或为其新增许可。

10 Pro采用原生纵向滚动容器，底部控制条随列表滚动。未移植参考机型466像素圆屏的分段位移缩放动画。

## 3.101.043固件核对

- launcher网格全局槽：`0x201102EC`，class `0x2CD2024C`；原生根：`0x201102F4`。
- 网格实例`+0x5C`保存原布局按钮，该按钮是网格的同级子对象；解引用前核对父对象子表。
- 原生图标SHORT_CLICKED：`0x0C517419`，事件描述符userdata为app_id。原回调在`0x0C517422`获取userdata，随后从事件对象读取中心坐标构造应用启动消息。新视图注册真实点击事件，不合成事件内存。
- 图片对象`+0x34`为当前图像源，`+0x44`为原图宽度；使用现有p67图像工厂、设源、枢轴和缩放接口。
- `app_lookup`：`0x0CA69935`，注册表节点`+0x10`为u16应用编号。
- `launcher_data_reload_app_name`：`0x0C514284`，`0x0C5142BE`读取注册节点`+0x1C`的显示名函数并调用。该回调也可能属于安装在RAM中的原生模块，不能仅允许flash指针。
- 用户配置及界面操作在Chaos现有UI维护回调执行；点击切换只登记重建请求，下一维护拍重建。
- 控件DELETE时立即作废自有视图句柄；系统重建时重新枚举当前图标。创建不完整时销毁自有视图，保留系统桌面。

## 验证

`rustc --edition=2021 --test supervisor/tests/s541_harness.rs -o /tmp/chaos-s541-tests && /tmp/chaos-s541-tests`

另运行 `python3 tools/test_honeycomb.py` 和 `python3 tools/build.py`。主机测试不证明设备上的布局、触摸和GPU效果，真机仍需验证三种视图、底部按钮、点击启动与关闭还原。
