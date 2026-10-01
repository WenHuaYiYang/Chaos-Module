-- Chaos installer v3 自研 chaos_sup 原生应用安装器（生产版，无探针）
-- 2026-08-22 重构（对齐 官方 installer_main.lua 的可行流程）：
-- 1. 不再 rmmod！官方 从不 rmmod。若 /dev/chaos 已存在（旧模块在内核）→ 要求重启手表再 Run。
-- 根因假设：rmmod 释放模块内存但 app 注册链表条目悬空 → 切表盘时 launcher 遍历到悬空回调 → 崩。
-- 2. 每条命令一个独立 timer 拍（对齐 官方："let miwear receive an event-loop turn\n-- between native registration stages"）。
-- 3. INSTALL 0x0A(restore 占位) → 0 → 1 → 2 四拍，间隔 1000ms。
-- 4. 状态行只显示当前步骤与结果。早期"把崩点步号落盘再读回来"的那条探针链已整体删掉，
--    安装过程不在 /data/chaos 留任何调试文件。

local lvgl = require("lvgl")

-- ===== 视觉: macOS 毛玻璃(vibrancy) =====
-- Web 上的 backdrop-blur / hover / transition-colors / 三栏布局, 在 336x480 的单屏 LVGL
-- 上没有对应物, 所以按这套风格的**核心理念**翻译:
--   层级即色彩 —— 不用渐变和发光, 纯靠背景深浅区分层级;
--   1px 哲学  —— 所有分隔都是 1px 边框, 不用阴影制造深度;
--   无装饰主义 —— 没有渐变、没有发光、没有动画。
-- 三栏 -> 单屏的三级层次: 页面底(最深) / 卡片(中间) / 按钮与高亮块(表面)。
-- 硬约束: 背景只用下面三个灰阶; 强调色与状态色**只用于文字**, 绝不作为背景;
--         圆角最大 12(rounded-xl); 边框一律 1px; 不要阴影与动画。
-- 字体: 标题走衬线(文楷 ChaosSans-Regular, 由美化页登记; 若该名字还没登记,
--       固件会回退默认字体, 界面依然完整可用), 正文走系统无衬线 MiSans-Regular。
local V_DEEP  = 0x1C1C1E   -- #1c1c1e 最深: 页面底
local V_MID   = 0x2C2C2E   -- #2c2c2e 中间: 卡片
local V_SURF  = 0x3A3A3C   -- #3a3a3c 表面: 按钮
local V_LINE  = 0x3C3C3E   -- 表面上的 1px 边框
local V_LINE2 = 0x2A2A2C   -- 最深上的 1px 边框
local V_TXT   = 0xF2F2F2   -- 主要文字(white/95)
local V_TXT2  = 0xB3B3B3   -- 次要文字(white/70)
local V_TXT3  = 0x666666   -- 弱化文字(white/40)
local V_ACC   = 0x0A84FF   -- 系统强调色(仅文字高亮)
-- 主按钮底的强调蓝: 规范「颜色角色」那节把 bg-blue-600 列在**允许的背景强调色**里
-- (bg-[#3a3a3c], bg-blue-600, bg-indigo-600)。禁止的是 #0a84ff 那类高亮色作底,
-- 不是这种标准按钮蓝。全是灰的时候界面太单调, 主按钮就该是这个颜色。
local V_ACC_BG = 0x2563EB
local V_OK    = 0x30D158   -- 成功(仅文字)
local V_WARN  = 0xFFD60A   -- 提醒(仅文字)
local V_ERR   = 0xFF453A   -- 失败(仅文字)
-- 圆角 24(用户明确要求"放大一倍": 12 -> 24)。
-- 演进: 8px(规范给 Web 40px 高按钮的值) -> 12(按手环 64px 按钮高度换算) -> 24(用户拍板)。
-- 说明: 规范上限是 rounded-xl(12px), 24 相当于 rounded-3xl, 这属于**用户明确要求的破例**,
-- 不是风格漂移, 别再按规范改回去。边界: 按钮 64px 高, 圆角到 32 就成胶囊
-- (rounded-full, 规范也禁), 所以不要再往上加。
local V_R     = 24
-- 字体: 只用系统自带的 MiSans-Regular。
-- 规范要求"标题用衬线", 但手环上唯一可能的衬线字体是用户换上来的文楷, 而它的 face
-- **只在美化页手动点"重新应用文楷"之后**才登记(59 轮按用户要求去掉了开机自动应用)。
-- 重启后没点过就登记不上, 建 face 会回退默认字体 —— 中文直接变空白豆腐块、字还变小
-- (2026-09-26 真机踩过, 用户报"上面有空白框、字体用错了")。
-- 所以标题改用系统字体, 层级靠**字号与颜色**建立。
local F_BODY  = "MiSans-Regular"

local SUPERVISOR_RESOURCE = SCRIPT_PATH .. "chaos_sup.ko"
local SUPERVISOR_PATH = "/data/chaos/sup.ko"
local ICON_RESOURCE = SCRIPT_PATH .. "chaos_icon.bin"
local ICON_PATH = "/data/chaos/chaos_icon.bin"
local DATA_DIR = "/data/chaos"
local DEVICE_PATH = "/dev/chaos"

local CMD_MAGIC = 0x53484331           -- "1CHS"
local STATUS_MAGIC = 0x53484332        -- "2CHS"
local STATUS_VERSION = 0x43485302      -- "CHS2"
local CMD_INSTALL = 2
local STATUS_SIZE = 192
local STATE_ACTIVE = 1

local status_label
local run_timer = nil
local run_phase = 1
local run_attempted = false
local clear_armed = false

local function shell_quote(v) return "'" .. tostring(v):gsub("'", "'\\''") .. "'" end
local function run(c) print("[chaos] exec: " .. c); local ok = os.execute(c); return ok == true or ok == 0 end
local function set_status(text, color) status_label:set { text = tostring(text), text_color = color or V_TXT2 } end

local function read_all(path, mode)
  if type(io) ~= "table" or type(io.open) ~= "function" then return nil end
  local f = io.open(path, mode or "rb"); if not f then return nil end
  local c = f:read("*a"); f:close(); return c
end

-- 语言检测: getprop <键> 含 "zh" = 中文界面。
-- 只保留两个布尔量: lang_seen(读到过任何语言属性没有, 决定是否走 fallback)、
-- lang_zh(是不是中文, 决定发 0x18 还是 0x19)。
-- 原来还顺手攒了一个给人看的 lang_probe 字符串(候选键的原始值), 于界面无用, 已删。
local LANG_OUTPUT = "/data/chaos/lang.txt"
local lang_zh = false
local lang_seen = false
local function detect_language()
  local candidates = {"persist.locale", "ro.system.language", "persist.sys.language", "persist.sys.locale", "ro.product.locale", "sys.language", "ro.product.language", "persist.sys.language_code", "ro.miwear.language"}
  for _, k in ipairs(candidates) do
    run("getprop " .. k .. " > " .. LANG_OUTPUT)
    local raw = read_all(LANG_OUTPUT, "r")
    if type(raw) == "string" and #raw > 0 then
      local v = raw:gsub("%s+$", "")
      if #v > 0 then
        lang_seen = true
        if not lang_zh and (v:match("zh") or v:match("CN") or v:match("Chinese") or v:match("chinese")) then lang_zh = true end
      end
    end
  end
  -- 方法B: getprop 无参全量 dump, 筛含 lang/locale 的行
  if not lang_seen then
    run("getprop > /data/chaos/allprop.txt")
    local all = read_all("/data/chaos/allprop.txt", "r")
    if type(all) == "string" then
      for line in all:gmatch("[^\\r\\n]+") do
        local low = line:lower()
        if low:match("lang") or low:match("locale") then
          lang_seen = true
          if not lang_zh and (low:match("zh") or low:match("cn") or low:match("chinese")) then lang_zh = true end
        end
      end
    end
  end
  -- 两种方法都得靠临时文件把 getprop 的输出绕回 Lua（这个运行时没有 popen）。
  -- 用完就删, 设备上不留残留文件。
  run("rm -f " .. LANG_OUTPUT .. " /data/chaos/allprop.txt")
end
local FONT_RESOURCE = SCRIPT_PATH .. "lxgw.ttf"
local FONT_DIR = "/data/chaos/font"
-- 只需要 2 个名字。真机确认: 真正生效的机制是"用系统没见过的名字建 face
-- + 把 face 写回样式对象"(实验5); 而"字体记录改向"那条路无效。
-- 多部署名字纯属白占空间: 14 名 x 3.4MB = 48MB, 且本设备文件系统不支持
-- 硬链接(ln 失败退化成写副本), 2026-09-13 真机实测多占 50MB。
local FONT_NAMES = {
  "ChaosWenKai.ttf", "ChaosWenKai-All.ttf",
}
-- 字体魔数判定: 00010000=TrueType / OTTO=CFF / true|ttcf=Apple 变体。
-- 2026-09-26 起字体已移出主包(包体 5.6MB -> 2.1MB), 这一槽改成占位说明文件,
-- 所以第 2.5 步必须先认内容 —— 不是字体就跳过, 不能报错(否则整个 Run 流程断在这)。
-- 字体改由独立的字体投递包送上机(见 docs/Chaos_字体投递_20260926.md)。
local function looks_like_font(c)
  if type(c) ~= "string" or #c < 4 then return false end
  local m = c:sub(1, 4)
  return m == string.char(0, 1, 0, 0) or m == "OTTO" or m == "true" or m == "ttcf"
end
local function stage_fonts()
  -- 目录必须**无条件**先建: 字体改由投递包送上机之后, 它落盘的 /data/chaos/font
  -- 就是这里创建出来的。2026-09-26 真机踩过: 字体包报"打不开目标文件", 根因就是
  -- 下面两句 return 把这一句 mkdir 也一起跳过了, 于是那个目录再没人建。
  run("mkdir -p " .. FONT_DIR)
  local c = read_all(FONT_RESOURCE, "rb")
  if not c then return true end                    -- 容器里没这一槽: 交给投递包
  if not looks_like_font(c) then return true end   -- 占位文件: 同上
  -- 清理历史上误部署的多余名字(约 48MB 副本), 恢复只留 2 份
  run("rm -f " .. FONT_DIR .. "/MiSans-*.ttf " .. FONT_DIR .. "/MiSansF-*.ttf "
      .. FONT_DIR .. "/BaiJamjuree-*.ttf")
  for _, n in ipairs(FONT_NAMES) do
    local f = io.open(FONT_DIR .. "/" .. n, "wb")
    if not f then return false, "open " .. n end
    local ok = pcall(f.write, f, c); f:close()
    if not ok then return false, "write " .. n end
  end
  return true
end
local function stage_resource(res, dest)
  local c = read_all(res, "rb"); if not c then return false, "cannot read " .. res end
  run("mkdir -p " .. DATA_DIR)
  local f = io.open(dest, "wb"); if not f then return false, "cannot open " .. dest end
  local wok, wres = pcall(f.write, f, c); local cok, cres = pcall(f.close, f)
  if not wok or not cok then return false, "write failed" end
  return true
end
-- 图标素材投递。2026-09-26 起主包**不再内嵌图标包**: 这一槽只有我们自己的应用图标
-- chaos_icon.bin(注册原生应用必用), 整份写到 /data/chaos/chaos_icon.bin。
-- 桌面图标包改由独立的图标投递包侧载上机(scripts/build_icon_pack.py + installer/icon_pack.lua),
-- 与字体同一条路 —— 换一套图标不再需要重刷主包, 也不会把主包撑到 2MB。
--
-- 顺带清理 2026-09-26 之前的扁平布局(/data/chaos/icons/*.bin, 约 1.9MB): 那时素材直接
-- 铺在 icons/ 下; 现在每个包一个子目录(icons/<包号>/, 由投递脚本建), 通配不会误删。
local ICON_DIR = DATA_DIR .. "/icons"
local function stage_icons()
  run("mkdir -p " .. DATA_DIR)
  run("mkdir -p " .. ICON_DIR)
  run("rm -f " .. ICON_DIR .. "/*.bin")
  return stage_resource(ICON_RESOURCE, ICON_PATH)
end

local function device_present() local f = io.open(DEVICE_PATH, "rb"); if not f then return false end; f:close(); return true end

local function word(v)
  v = math.floor(v)
  return string.char(v % 0x100, math.floor(v / 0x100) % 0x100,
      math.floor(v / 0x10000) % 0x100, math.floor(v / 0x1000000) % 0x100)
end
local function bytes_to_words(content)
  if type(content) ~= "string" or #content ~= STATUS_SIZE then return nil end
  local w = {}
  for off = 1, STATUS_SIZE, 4 do
    local a, b, c, d = content:byte(off, off + 3)
    w[#w + 1] = a + b * 0x100 + c * 0x10000 + d * 0x1000000
  end
  return w
end
local function read_status()
  local f = io.open(DEVICE_PATH, "rb"); if not f then return nil, "cannot open " .. DEVICE_PATH end
  local raw = f:read(STATUS_SIZE); f:close()
  local w = bytes_to_words(raw)
  if not w or #w < 8 or w[1] ~= STATUS_MAGIC then return nil, "status ABI mismatch" end
  if w[4] ~= STATUS_VERSION then return nil, string.format("old sup version=0x%X reboot", w[4]) end
  return { state = w[11], write_count = w[3], dbg0 = w[43], dbg1 = w[44], dbg2 = w[45], step = w[48] }
end
local function write_install(arg0)
  local payload = word(CMD_MAGIC) .. word(CMD_INSTALL) .. word(arg0) .. word(0)
  local f = io.open(DEVICE_PATH, "wb"); if not f then return false, "cannot open " .. DEVICE_PATH end
  local wok, wres = pcall(f.write, f, payload); local cok, cres = pcall(f.close, f)
  if not wok or wres == nil or not cok or cres == nil then return false, "write failed" end
  return true
end
local function execute_install(arg0)
  local ok, err = write_install(arg0); if not ok then return false, err end
  local st, e = read_status(); if not st then return false, e end
  if st.state ~= STATE_ACTIVE then
    return false, string.format("state=%d wr=%d step=%d", st.state, st.write_count, st.step or -1)
  end
  -- DBG 值含义（ko 侧 fops）：dbg0=lookup结果 dbg1=register_app返回值 dbg2=lookup验证结果
  return true, string.format("d0=%X d1=%X d2=%X st=%d",
      st.dbg0 or 0, st.dbg1 or 0, st.dbg2 or 0, st.step or -1)
end

-- 每步一个 timer 拍（对齐 官方：步骤间让 miwear 事件循环转一圈）
local notify_retry = 0  -- notify step 重试计数（等 launcher 订阅就绪）
local steps = {
    { "1 部署模块",     function() return stage_resource(SUPERVISOR_RESOURCE, SUPERVISOR_PATH) end },
  { "2 部署应用图标", stage_icons },
  { "2.5 部署字体", stage_fonts },
  { "3 加载模块", function()
      if device_present() then
        return false, "old module active: power OFF watch, power ON, then Run again"
      end
      local ok = run(string.format("insmod %s chaos_sup", shell_quote(SUPERVISOR_PATH)))
      if not ok then return false, "insmod failed" end
      return true
  end },
  { "4 检查设备", function()
      if not device_present() then return false, "/dev/chaos missing" end
      return true
  end },
  { "4.5 设置语言", function()
      -- 一个语言属性都没读到 => 无法判断, 按中文装
      if not lang_seen then
        set_status("语言: 未知, 按中文装", V_WARN)
        return execute_install(0x18)
      end
      if lang_zh then
        set_status("语言: 中文", V_OK)
        return execute_install(0x18)
      end
      set_status("语言: 英文", V_OK)
      return execute_install(0x19)
  end },
  { "5 恢复占位", function()
      return execute_install(0x0A)
  end },
  { "6 注册应用", function()
      return execute_install(0x13)
  end },
  { "6.5 通知系统", function()
      -- 2026-08-25 根本修复: 直接调固件 notify 完整链(0x0CA81D18 注册表查询
      -- + 0x0CA81FB9 notify_installed), 官方 同款 100% 成功路径。
      -- 替代手动复刻链(0x3F 订阅等待只是缓解, 根因是绕开了固件)。
      return execute_install(0x43)
  end },
    { "7 发布应用", function()
      return execute_install(1)
  end },
  { "8 INSTALL arg0=2 (launcher entries)", function()
      return execute_install(2)
  end },
}

local function finish_run(timer, success, message)
    timer:delete()
    if run_timer == timer then run_timer = nil end
    set_status(message, success and V_OK or V_ERR)
end

local function run_next_step(timer)
    local step = steps[run_phase]
    if not step then
        finish_run(timer, true, "运行完成")
        return
    end
    set_status("RUN " .. step[1])
    local ok, message, extra = pcall(step[2])
    if not ok then
        finish_run(timer, false, "RUN " .. step[1] .. " lua err: " .. tostring(message))
        return
    elseif message == "retry" then
        -- 重试当前步（等下一拍, 用于 notify 等 launcher 就绪）
        timer:ready()
        return
    elseif message == false then
        finish_run(timer, false, "RUN " .. step[1] .. " failed: " .. tostring(extra))
        return
    end
    run_phase = run_phase + 1
    if run_phase > #steps then
        execute_install(0x22)
        local st1 = read_status()
        local d2 = 0
        if st1 then d2 = st1.dbg2 or 0 end
        -- 注册没生效时不能只说"运行完成", 否则用户会以为已经装好。
        -- (原来这里还会把 app_id/reg 版本号落盘到 reg.txt 并读 pread.txt 拼成一长串 ——
        --  那些落盘用户根本读不到, 对应的 file_read 探针也早删了, 一并清掉。)
        if d2 == 0 or d2 == 0xFFFFFFFF then
          finish_run(timer, false, "运行完成, 但注册未生效")
        else
          finish_run(timer, true, "运行完成")
        end
    else
        timer:ready()   -- 立即排下一拍（中间仍有事件循环 turn）
    end
end

local function start_run_timer()
    run_phase = 1
    local created = lvgl.Timer {
        period = 1000, repeat_count = -1, paused = true,
        cb = function(timer)
            local ok, message = pcall(run_next_step, timer)
            if not ok then
                finish_run(timer, false, "运行失败: " .. tostring(message))
            end
        end,
    }
    if not created then return false, "cannot create timer" end
    run_timer = created
    created:resume()
    created:ready()
    return true
end

-- ===== 界面(macOS 毛玻璃: 页面底 / 卡片 / 按钮 三级层次) =====
-- 定位与居中一律用 align 表(对象相对父居中), 且 Label **不设 w/x/y** —— 让它按文字
-- 自适应宽度, 再整体居中, 这才是视觉居中。
-- 重要(2026-09-26 返工): 不要用 "x/w + text_align" 那套写法。它来自自研表盘"墟",
-- 而"墟"从来没有在真机上显示成功过(一直是黑的), 所以那条路径**没有被验证过**;
-- 真机上表现就是文字左对齐、整体不居中。唯一的居中证据是安装器原版用的 align 表。
local SCR_W = lvgl.HOR_RES()
local SCR_H = lvgl.VER_RES()

local root = lvgl.Object(nil, {
  w = SCR_W, h = SCR_H,
  outline_width = 0, border_width = 0, pad_all = 0,
  bg_opa = lvgl.OPA(100), bg_color = V_DEEP,
})
-- 重要(2026-09-26 真机): LVGL 对象默认带 SCROLLABLE, 会把手势吃掉 —— 表现就是
-- **长按表盘进不了表盘选择页**(用户实测, 主包与投递包都一样)。
-- 原版安装器对三个根对象全都显式清了这一位, 重构时被删掉过。凡是本脚本创建的
-- 容器对象, 一律清掉 SCROLLABLE; 需要点击的再单独加 CLICKABLE。
root:clear_flag(lvgl.FLAG.SCROLLABLE)
-- 指南 §12A.7「10 Pro 已验证写法」那条: 容器缺 flag => clear_flag(SCROLLABLE) + add_flag(EVENT_BUBBLE)。
-- **只清 SCROLLABLE 不够** —— 真机验证过: 清完长按仍然进不了表盘选择页。
-- EVENT_BUBBLE 才让触摸事件**冒泡给父层**(固件的表盘容器), 系统的长按手势才收得到;
-- 官方 theme1 表盘的 event_mask 也是这两句(CLICKABLE + EVENT_BUBBLE)。
-- root 自己也加 CLICKABLE: 空白处按下时得有个接收者, 否则事件无处可冒。
root:add_flag(lvgl.FLAG.CLICKABLE)
root:add_flag(lvgl.FLAG.EVENT_BUBBLE)

-- 标题: 一个 Label 说完, 字号与颜色建立层级(不用衬线, 原因见上面 F_BODY 的说明)
lvgl.Label(root, {
  text = "Chaos 安装器",
  text_color = V_TXT,
  text_font = lvgl.Font(F_BODY, 40),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -170 },
})

-- 状态卡片: 中间灰 + 1px 边框 + 12 圆角(卡片里不再套卡片); 状态文字在卡片内居中
local card = lvgl.Object(root, {
  w = SCR_W - 32, h = 150,
  bg_opa = lvgl.OPA(100), bg_color = V_MID,
  border_width = 1, border_color = V_LINE2,
  radius = V_R, pad_all = 0,
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -20 },
})
card:clear_flag(lvgl.FLAG.SCROLLABLE)
card:add_flag(lvgl.FLAG.EVENT_BUBBLE)
status_label = lvgl.Label(card, {
  text = "准备就绪",
  text_color = V_TXT2,
  text_font = lvgl.Font(F_BODY, 22),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 0 },
})

-- 按钮三态。颜色抄自 StyleKit 示例站的真实实现(用户给了其离线导出源码):
--   站上 Danger 按钮原文:
--     bg-[#ff453a]/15 text-[#ff453a] rounded-lg ... (无边框)
--   站上 Success 按钮同构: bg-[#30d158]/15 text-[#30d158] ...
--   即**同色系淡底(15% 透明度) + 同色字 + 无边框**。
--   LVGL 里对应 bg_color=同色 + bg_opa=OPA(15)(底下的深灰透出来, 就是 /15 的效果)。
--   之前两版都不对: "深灰底 + 红字"缺那层红底(用户说"太奇怪");
--   "实心红底 + 白字"又太实。
--   primary = 强调蓝底 + 白字(主操作)
--   danger  = 红 15% 底 + 红字, 无边框
--   ghost   = 深底 + 1px 边框 + 次要文字
local function make_button(text, y_ofs, kind, on_clicked)
  local bg, border, fg, opa, bw = V_DEEP, V_LINE2, V_TXT2, 100, 1
  if kind == "primary" then
    bg, border, fg = V_ACC_BG, V_ACC_BG, V_TXT
  elseif kind == "danger" then
    bg, border, fg, opa, bw = V_ERR, V_ERR, V_ERR, 15, 0
  end
  local b = lvgl.Object(root, {
    w = SCR_W - 32, h = 64,
    bg_opa = lvgl.OPA(opa),
    bg_color = bg,
    border_width = bw,
    border_color = border,
    radius = V_R, pad_all = 0,
    align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = y_ofs },
  })
  b:clear_flag(lvgl.FLAG.SCROLLABLE)
  b:add_flag(lvgl.FLAG.CLICKABLE)
  b:add_flag(lvgl.FLAG.EVENT_BUBBLE)
  lvgl.Label(b, {
    text = text,
    text_color = fg,
    text_font = lvgl.Font(F_BODY, 24),
    align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 0 },
  })
  b:onClicked(function() local ok, msg = pcall(on_clicked); if not ok then set_status("错误: " .. tostring(msg), V_ERR) end end)
  return b
end

local run_button
detect_language()

run_button = make_button("运行", 110, "primary", function()
  if run_timer then set_status("运行中"); return end
  if run_attempted then set_status("仅一次, 重启后重试", V_WARN); return end
  if device_present() then
    set_status("已激活, 请重启后运行", V_WARN)
    return
  end
  run_attempted = true
  run_button:clear_flag(lvgl.FLAG.CLICKABLE)
  local started, err = start_run_timer()
  if not started then set_status("运行失败: " .. tostring(err), V_ERR) end
end)

make_button("清除重置", 186, "danger", function()
  if run_timer then set_status("运行中, 请等待", V_WARN); return end
  if not clear_armed then clear_armed = true; set_status("再点一次确认清除", V_WARN); return end
  clear_armed = false
  run("rm -rf " .. DATA_DIR)
  set_status("已清除, 重启后重新运行", V_OK)
end)
