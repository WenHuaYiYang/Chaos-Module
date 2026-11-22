# 字体投递包的字形补全（日文假名 / Emoji）

## 现象

第三方字体投递包（如 `chaos-fontpack-p67-Phi_3.bin`）投递后，日文假名与 Emoji 显示为空白。

## 原因（实测）

包里 `font.ttf` 是**子集化**字体，实测覆盖：

| 区段 | 码位数 |
|---|---|
| CJK 汉字 U+4E00–9FFF | 15437 |
| **假名 U+3040–30FF** | **0** |
| 半角片假名 U+FF66–FF9D | 56 |
| **Emoji U+1F300–1FAFF** | **0** |

投递流程是：`main.lua` 把 `font.ttf` 写成 `/data/chaos/font/stN.ttf` →
通过 `/dev/chaos` 让内核模块切槽并重新应用 → 模块按 `(名字, 字号)` 建 face
（名字 `ChaosSans-*`，固件没见过所以必新建）。字体文件里没有的字形就是空白，
不会自动回退到系统字体（固件的 fallback 链配置里没有 `ChaosSans`）。

## 解决：把假名字形合并进包里的 font.ttf

```bash
# 需要 fonttools（pip install fonttools）
python tools/font_pack_merge.py 原包.bin -o 新包.bin
```

- 默认源字体：`C:/Windows/Fonts/YuGothM.ttc`（Yu Gothic，Win10/11 自带）
- 默认补充码位：`3040-3100`（平假名+片假名）+ `FF66-FFA0`（半角片假名）+ `30FB/30FC`
- 处理：从源字体子集化 → 按 `unitsPerEm` 缩放到与目标一致（实测 2048 → 1000）→
  逐字形复制 glyf/hmtx/cmap（含组合字形递归）→ 按原容器布局重打包
  （只替换最后一个 `font.ttf` 块，其它块与头部字段原样保留）

实测结果：`+189` 个假名字形，包 4,613,785 → 4,641,661 B。

## Emoji 为什么补不了

1. Emoji 在固件里是**独立的共享字体节点**（字体管理器里名字为 `Emoji`，
   销毁路径对它有特殊处理：不释放载荷），由固件的 fallback 链使用。
2. Windows 的 `seguiemj.ttf` 是 CBDT/CBLC 彩色位图字体，合并进 TrueType
   轮廓字体后 LVGL/FreeType 渲染路径不确定。
3. 真正可行的是**运行时把投递字体 face 的 fallback（结构 +0x1C）指向固件的
   `Emoji` 节点**，但需要正确维护该节点的引用计数（销毁时 `[节点+0x2c] -= 1`），
   偏移或计数错会导致系统字体被提前释放 —— 高风险，需真机验证后才敢合入。
