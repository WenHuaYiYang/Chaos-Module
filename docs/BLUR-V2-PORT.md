# 模糊V2：SysPatch IIR第一阶段移植

当前保留Corona实现及许可。“Corona模糊”恢复为独立开关，使用原来的box滤波；“模糊v2”使用SysPatch四方向12位定点IIR滤波，radius=12。开启任一项会关闭另一项并分别保存配置；历史配置两项均开时V2优先。系统背景会关闭两项。

原始来源为用户提供的`~/Downloads/Vela_NativeApp-syspatch 4/ELF/src/syspatch_blur.c`和同名头文件，原样保存在`supervisor/syspatch_blur/`。文件头记载其来源为LVGL软件模糊，保留全部上游注释和相邻LVGL 9.5许可文本；该许可文本不代表为整个SysPatch项目重新指定许可。

第一阶段只移植纯像素滤波：输入为已核对的3.101.043表盘快照，经既有4×降采样得到84×120 RGB565，运行SysPatch原生RGB565分支并应用70%暗化，再由既有原生绘制进行4×放大。并未移植其他机型的466×466结构、硬编码地址或七级GPU高斯管线，也尚未接入其GPU缩小/软件滤波/GPU放大的完整后台。

四区区域选择、绘制/滚动/DELETE hook、GPU fence、快照所有权、缓存退役及通知图标透明处理仍复用当前p67适配。切换算法仅在原生idle门通过后执行，作废旧滤波结果并重新请求采集；不修改vendor原文件。诊断`algorithm=0`为Corona，`algorithm=1`为SysPatch IIR。

因此当前V2不是独立于Corona的完整实现，不能删除Corona代码、AGPL许可及界面署名。后续需要逐项替换四区生命周期及快照/发布适配，并在真机核对效果、帧耗时和低内存表现，再做最终清理。

主机测试`supervisor/tests/syspatch_filter_test.c`验证Corona分支逐像素不变、V2与原始SysPatch调用逐像素一致、两算法输出不同、白图不变黑和边界拒绝。完整构建仅证明目标ABI链接，不代表真机验收。

运行`python3 tools/test_blur_v2.py`验证源码摘要、滤波和生产选择器；`python3 tools/test_corona_blur.py`验证原有四区和19项配置回归。真机进入系统模糊，分别选择Corona与V2，对照四区效果及进出活力指标后的恢复，再决定下一阶段适配。
