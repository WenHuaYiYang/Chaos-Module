# 控制中心按钮模糊 V2（p67 / 3.101.043）

2026-11-15。目标 AP SHA-256：
`519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`。
本轮验证包括固件静态核对、原指令探针、主机生命周期/像素/配置回归和完整编译。
未安装到 Mirage 或真机；视觉效果、GPU/cache 接受性、实际帧耗时仍待现场验收。

## 官方按钮与 GPU ABI

固件控制中心 `page_main.c` 在 `0x0c504c20` 创建页面，根槽 `0x2010fe50`、
根类 `0x2cdba8b4`。`0x0c50393c` 枚举 9 项、每项 64 字节的记录；
图标为独立 image 子对象，资源位于 `/resource/app/control_center/icon/`。

`0x0c5038c4` 调用 `0x0ca48474` 建立原生 LVX 按钮类 `0x2cdb9d6c`：
宽 156、高 128，圆角样式属性 12 = 36，默认底色属性 28 = `0x222222`。
`0x0c504464` 更新开关状态、蓝色 `0x277af7` 和 checked 状态。
底部编辑按钮为同类、160×66，由 `0x0ca48510` 设置不同状态的底色。
BG_OPA getter `0x0c1068a9(obj,0,29)`，setter `0x0c5883f9(obj,opacity,0)`。

图片描述符初始化 `0x0c5882b9`、大小 112 字节；绘制 `0x0c5882a1`。
源槽 index 7，scale_x/y index 12/13，pivot index 16/17，opacity byte 76。
原生 VG-Lite 图片核心 `0x0c153c94` 会将缩放矩阵与透明混合参数提交到
`0x002b467c` blit_rect 或 `0x002b6720` draw_pattern。
`0x0c15b7e4` 只构建坐标/scale/pivot 矩阵，没有独立 crop 偏移；
`0x0c15c0b8` 是颜色 key，不能当作圆角 bitmap mask。

原文与脚本位于固件资料的 `Ghidra/chaos-cc-*.txt`、`ChaosCcButtons.java`。
`chaos-cc-live-abi.txt` 记录本轮矩阵/颜色 key 核对。
`tools/probe_cc_buttons_abi.py` 执行原固件工厂、尺寸/圆角设置、
112 字节描述符初始化指令；对象分配、通用样式写入等叶子用模型替代。
这不是 GPU 硬件模拟。

## 原生对象与透明合成

`supervisor/control_center_buttons.inc` 仅在确认的控制中心根内绑定该按钮类，
最多扫描 256 节点、管理 16 个按钮，添加自己的 DRAW_MAIN/DELETE 事件监听。
官方页面、按钮、图标、输入、布局和状态回调继续由系统管理。
DRAW_MAIN 在官方底板之后、图标子对象之前提交局部玻璃图。

纹理就绪后临时降低原生 BG_OPA，关闭时恢复保存的原值。
如果其他代码已更改 BG_OPA，则保留它的新值。DELETE 只退休纹理，
释放在 owner poll 通过 fence 后进行，模糊源已消失也能完成回收。

默认玻璃不透明度 75%（191/255），状态底色不透明度 38%（97/255），
中心两层叠加约 84.5% 不透明度，背景约透出 15.5%。两项可分别设置为 0..100%。
按钮颜色保留 75% 原通道、25% 亮度，再按“按钮亮度”缩放，默认 44%。
这是合成模型，不能代替实际壁纸下的可读性验收。

## 实时刷新与开销

40f4b35 为解决重复 CPU 调色/圆角计算和逐张 cache clean/fence，取消了位置缓存键；
其副作用是滚动时底图冻结。本轮恢复逐帧位置取样，并分离素材生成与取样：

1. 从已发布的 336×480 BGR888 V2 模糊源建立共享 168×240 BGRA atlas。
   只有源发布代数或参数代数变化时调色一次。atlas 约 161 KB、静态对齐存储。
2. 大按钮纹理 78×64、编辑按钮 80×33，圆角半径缩半、4×4 抗锯齿覆盖率。
   alpha 仅在首次分配或形状改变时计算；奇数宽高使用 1× 纹理避免取整误差。
3. 显示事件 52 START 通过默认空的 `P67_BLUR_FRAME_READY` 扩展调用实时更新。
   遍历已有绑定和有界父链，读取当前坐标，从 atlas 复制区域。
   内部不透明像素为单个 32 位复制，只有圆角边缘需要预乘。
   同一批次最多一次 GPU fence、一次 cache clean，再使更新对象失效。
4. START 不分配/释放、不写样式、不做全局对象树扫描、不重算圆角。
   没有源/位置/参数变化时不复制、不 fence。DRAW_MAIN 只提交原生 GPU 图像任务，
   不进行纹理生成或全局树扫描。GPU 负责 2× 缩放和透明混合。
5. owner poll 承担绑定、形状变化、样式调整和回收；稳定时每 8 次检查一次子树。
   已有纹理的纯位置变化留给 START 处理，避免 owner poll 重复复制。
6. 控制中心按钮可见且亮屏时，按配置间隔（默认 500 ms）请求新的表盘源；
   复用原 provider 的安全 START 预算、入口/所有权门控、双缓冲与发布流程。
   采集对象始终是表盘 root，不包含按钮图标；熄屏、隐藏、关闭时不请求。
   忙碌时可延后，因此刷新间隔是请求间隔，不是硬性帧率承诺。

滤波仍为软件 IIR，局部取样仍由 CPU 完成；没有把未经核对的其他机型
GPU 模糊或裁剪结构移植过来。新实现降低每帧重复计算，但实际 FPS 需真机测量。
典型 9 个主按钮纹理约 180 KB、编辑按钮约 11 KB，另有 stride 对齐开销；
atlas 独立共享，不为每个按钮额外生成全屏快照。

## 区域、参数与诊断

“系统模糊 → 模糊区域 → 控制中心按钮”默认开启、仅对 V2 生效，
保存在 `/data/chaos/blur_cc_buttons.cfg`。四个背景全关时可单独启用按钮，
原四背景位图 0..15 不变。`P67_BLUR_EXTRA_DEMAND` 默认零扩展提供独立需求。

“系统美化 → 模糊V2参数”支持数字键盘调整并保存六项参数及恢复默认。
范围、格式及错误处理见 [模糊V2移植说明](BLUR-V2-PORT.md)。

诊断保留 `cc_buttons / cc_button_draws / cc_button_updates / cc_button_errors`，
新增 `cc_frame_updates`（START 更新按钮数）、`cc_atlas`（共享素材生成代数）、
`cc_params`（当前参数代数），便于比较静止、拖动和源刷新。

## 验证

```sh
python3 tools/test_cc_buttons.py
python3 tools/test_blur_params.py
python3 tools/test_corona_blur.py
python3 tools/test_blur_v2.py
rustc --edition=2021 --test supervisor/tests/network_platform_harness.rs -o /tmp/chaos-clock-tests
/tmp/chaos-clock-tests
sh tools/test_keyboard.sh
/tmp/chaos-palm-verify/bin/python tools/probe_cc_buttons_abi.py
python3 tools/build.py --firmware 10p-3.101.043
```

主机回归覆盖：连续 120 帧位置变化的实际像素更新，无额外分配/decoder eviction/
全局扫描，单帧单批 fence/clean；静止无复制；源与参数变化；可见/亮屏刷新请求限频；
透明度恢复、旧代事件、注册/分配/fence 失败、回调删除、关闭时无源回收；
灰阶/饱和色、圆角 AA、预乘、stride padding 与对齐/边界拒绝；
参数短读写、校验/范围拒绝、重启加载、写入/关闭失败重试和 native busy 延迟应用。
这些操作计数与像素回归不能代替设备 GPU/cache、手势和耗电验收。

额外执行旧 `tools/test_network_platform.py` 时，其 Rust 8 项通过，但 C glue 旧 harness
缺少已有 `network_connect_address` 叶子，且仍假定旧密码最短 8 位；这是既有测试漂移。
本轮保留该 harness 原样，时钟变更用上面的生产 Rust wrapper 回归单独验证。
