-- chaos_sup 原生应用安装器
-- 流程约束：
-- 1. 不做 rmmod。若 /dev/chaos 已存在说明旧模块还在内核里，此时要求重启设备再运行。
--    根因: rmmod 释放模块内存，但 app 注册链表的条目留在原地悬空 → 切表盘时
--    launcher 遍历到悬空回调 → 崩。
-- 2. 每条命令一个独立 timer 拍，让 miwear 的事件循环在两个注册阶段之间转一圈。
-- 3. INSTALL 0x0A(restore 占位) → 0 → 1 → 2 四拍，间隔 1000ms。
-- 4. 状态行只显示当前步骤与结果，安装过程不在 /data/chaos 留任何调试文件。

local lvgl = require("lvgl")

-- ===== 视觉: macOS 毛玻璃(vibrancy) 的单屏翻译 =====
-- Web 上的 backdrop-blur / hover / transition-colors / 三栏布局, 在 336x480 的单屏 LVGL
-- 上没有对应物, 所以按这套风格的**核心理念**翻译:
--   层级即色彩 —— 不用渐变和发光, 纯靠背景深浅区分层级;
--   1px 哲学  —— 所有分隔都是 1px 边框, 不用阴影制造深度;
--   无装饰主义 —— 没有渐变、没有发光、没有动画。
-- 三栏 -> 单屏的三级层次: 页面底(最深) / 卡片(中间) / 按钮与高亮块(表面)。
-- 硬约束: 背景只用下面三个灰阶; 强调色与状态色**只用于文字**, 绝不作为背景;
--         圆角上限 12, 只有按钮按下面 V_R 处的说明破例; 边框一律 1px;
--         不要阴影与动画。
-- 字体: 只用系统自带的无衬线 MiSans-Regular(标题不走衬线, 原因见 F_BODY 处的说明)。
local V_DEEP  = 0x1C1C1E   -- #1c1c1e 最深: 页面底
local V_MID   = 0x2C2C2E   -- #2c2c2e 中间: 卡片
local V_SURF  = 0x3A3A3C   -- #3a3a3c 表面: 按钮
local V_LINE  = 0x3C3C3E   -- 表面上的 1px 边框
local V_LINE2 = 0x2A2A2C   -- 最深上的 1px 边框
local V_TXT   = 0xF2F2F2   -- 主要文字(最亮一档)
local V_TXT2  = 0xB3B3B3   -- 次要文字
local V_TXT3  = 0x666666   -- 弱化文字
local V_ACC   = 0x0A84FF   -- 系统强调色(仅文字高亮)
-- 主按钮底的强调蓝: 禁止的是 #0a84ff 那类高亮色作底, 标准按钮蓝允许做背景。
-- 界面全是灰时认不出哪个是主操作, 主按钮就用这一档蓝。
local V_ACC_BG = 0x2563EB
local V_OK    = 0x30D158   -- 成功(仅文字)
local V_WARN  = 0xFFD60A   -- 提醒(仅文字)
local V_ERR   = 0xFF453A   -- 失败(仅文字)
-- 圆角 24: 高于上面那条 12 的上限, 这是按按钮实际尺寸定的刻意取值, 不要按上限改回 12。
-- 边界: 按钮高 64px, 圆角到 32(高度的一半)就成胶囊形, 那种形状不用, 不要再往上加。
local V_R     = 24
-- 字体: 只用系统自带的 MiSans-Regular。
-- 标题本想走衬线, 但手环上唯一可能的衬线字体是侧载上来的文楷, 而它的 face 只在美化页
-- 手动点过"重新应用文楷"之后才登记 —— 重启后没点过就登记不上, 建 face 会回退默认
-- 字体, 中文直接变空白豆腐块、字还变小(实测踩过)。
-- 所以标题也走系统字体, 层级靠**字号与颜色**建立。
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
-- 运行器状态: runner = 当前执行中的 timer; step_index = 步骤游标;
-- started_once = 本次会议跑过没有(一个模块只能装一次, 跑过必须重启);
-- wipe_armed = 清除按钮的两段式确认闸。
local runner
local step_index = 1
local started_once = false
local wipe_armed = false

local function set_status(text, color) status_label:set { text = tostring(text), text_color = color or V_TXT2 } end

-- 把参数变成 POSIX 安全的单引号字面量。单引号内部没有任何转义机制,
-- 遇到引号只能"关引号 + 转义引号 + 重开引号"。
local function quote_arg(value)
  local pieces = { "'" }
  for ch in tostring(value):gmatch(".") do
    if ch == "'" then
      pieces[#pieces + 1] = "'\\''"
    else
      pieces[#pieces + 1] = ch
    end
  end
  pieces[#pieces + 1] = "'"
  return table.concat(pieces)
end

local function exec(cmd)
  print("[chaos-installer] " .. cmd)
  local rc = os.execute(cmd)
  return rc == true or rc == 0
end

-- 读整个小文件。这个 Lua 运行时没有 popen, 命令输出全靠重定向到临时文件再读回来。
local function read_all(path, mode)
  if type(io) ~= "table" then return nil end
  local open = io.open
  if type(open) ~= "function" then return nil end
  local fh = open(path, mode or "rb")
  if not fh then return nil end
  local body = fh:read("*a")
  fh:close()
  return body
end

-- 语言检测: getprop <键> 含 "zh" = 中文界面。
-- 只留两个布尔量: lang_seen(有没有读到过任何语言属性, 决定是否走 fallback)、
-- lang_zh(是不是中文, 决定发 0x18 还是 0x19)。
local LANG_OUTPUT = "/data/chaos/lang.txt"
local lang_zh = false
local lang_seen = false
local function detect_language()
  local candidates = {"persist.locale", "ro.system.language", "persist.sys.language", "persist.sys.locale", "ro.product.locale", "sys.language", "ro.product.language", "persist.sys.language_code", "ro.miwear.language"}
  for _, k in ipairs(candidates) do
    exec("getprop " .. k .. " > " .. LANG_OUTPUT)
    local raw = read_all(LANG_OUTPUT, "r")
    if type(raw) == "string" and #raw > 0 then
      local v = raw:gsub("%s+$", "")
      if #v > 0 then
        lang_seen = true
        if not lang_zh and (v:match("zh") or v:match("CN") or v:match("Chinese") or v:match("chinese")) then lang_zh = true end
      end
    end
  end
  -- 备选: getprop 无参全量 dump, 筛含 lang/locale 的行
  if not lang_seen then
    exec("getprop > /data/chaos/allprop.txt")
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
  exec("rm -f " .. LANG_OUTPUT .. " /data/chaos/allprop.txt")
end
local FONT_RESOURCE = SCRIPT_PATH .. "lxgw.ttf"
local FONT_DIR = "/data/chaos/font"
-- 只需要 2 个名字。实测确认生效的机制是"用系统没见过的名字建 face
-- + 把 face 写回样式对象"; 而"改字体记录指向"那条路无效。
-- 多铺名字只是占空间: 14 名 x 3.4MB = 48MB, 且本设备文件系统不支持硬链接
-- (ln 失败退化成写副本), 实测多占 50MB。
local FONT_NAMES = {
  "ChaosWenKai.ttf", "ChaosWenKai-All.ttf",
}
-- 字体魔数判定: 00010000=TrueType / OTTO=CFF / true|ttcf=Apple 变体。
-- 主包不内嵌字体(包体 5.6MB -> 2.1MB), 容器里这一槽是占位说明文件,
-- 所以第 2.5 步必须先认内容 —— 不是字体就跳过, 不能报错(报错整个运行流程断在这)。
-- 字体由独立的字体投递包侧载上机(见 installer/font_pack.lua)。
local function looks_like_font(c)
  if type(c) ~= "string" or #c < 4 then return false end
  local m = c:sub(1, 4)
  return m == string.char(0, 1, 0, 0) or m == "OTTO" or m == "true" or m == "ttcf"
end
local function stage_fonts()
  -- 目录必须**无条件**先建: 字体改由投递包送上机之后, 它落盘的 /data/chaos/font
  -- 就是这里创建出来的。
  -- 注意: mkdir 不能挪到下面两个提前返回之后 —— 一起被跳过就没人建这个目录,
  -- 投递时报"打不开目标文件"。
  exec("mkdir -p " .. FONT_DIR)
  local c = read_all(FONT_RESOURCE, "rb")
  if not c then return true end                    -- 容器里没这一槽: 交给投递包
  if not looks_like_font(c) then return true end   -- 占位文件: 同上
  -- 清掉早期版本多铺出来的名字副本(约 48MB), 只留上面那 2 份
  exec("rm -f " .. FONT_DIR .. "/MiSans-*.ttf " .. FONT_DIR .. "/MiSansF-*.ttf "
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
  exec("mkdir -p " .. DATA_DIR)
  local f = io.open(dest, "wb"); if not f then return false, "cannot open " .. dest end
  local wok, wres = pcall(f.write, f, c); local cok, cres = pcall(f.close, f)
  if not wok or not cok then return false, "write failed" end
  return true
end
-- 图标素材投递。主包**不内嵌图标包**: 这一槽只有本应用自己的图标
-- chaos_icon.bin(注册原生应用必用), 整份写到 /data/chaos/chaos_icon.bin。
-- 桌面图标包由独立的图标投递包侧载上机(scripts/build_icon_pack.py + installer/icon_pack.lua),
-- 与字体同一条路 —— 换一套图标不需要重刷主包, 也不会把主包撑大 2MB。
--
-- 顺带清掉早期扁平布局的残留(/data/chaos/icons/*.bin, 约 1.9MB): 那时素材直接
-- 铺在 icons/ 下; 现在每个包一个子目录(icons/<包号>/, 由投递脚本建), 通配不会误删。
local ICON_DIR = DATA_DIR .. "/icons"
local function stage_icons()
  exec("mkdir -p " .. DATA_DIR)
  exec("mkdir -p " .. ICON_DIR)
  exec("rm -f " .. ICON_DIR .. "/*.bin")
  return stage_resource(ICON_RESOURCE, ICON_PATH)
end

local function module_loaded()
  local fh = io.open(DEVICE_PATH, "rb")
  if not fh then return false end
  fh:close()
  return true
end

-- u32 小端打包: 逐字节压进 table 再拼, 避免一行四个取模堆在一起
local function u32le(value)
  local n = math.floor(value)
  local pieces = {}
  for _ = 1, 4 do
    pieces[#pieces + 1] = string.char(n % 0x100)
    n = math.floor(n / 0x100)
  end
  return table.concat(pieces)
end

local function status_words(raw)
  if type(raw) ~= "string" or #raw ~= STATUS_SIZE then return nil end
  local words = {}
  local slot = 1
  for base = 1, STATUS_SIZE, 4 do
    local a, b, c, d = raw:byte(base, base + 3)
    words[slot] = a + b * 0x100 + c * 0x10000 + d * 0x1000000
    slot = slot + 1
  end
  return words
end

local function read_status()
  local fh = io.open(DEVICE_PATH, "rb")
  if not fh then return nil, "设备打不开" end
  local raw = fh:read(STATUS_SIZE)
  fh:close()
  local words = status_words(raw)
  if not words or #words < 8 then return nil, "状态字太短" end
  if words[1] ~= STATUS_MAGIC then return nil, "状态字不符" end
  if words[4] ~= STATUS_VERSION then
    return nil, "模块版本旧, 先重启"
  end
  -- 词位含义(见 ko 侧 fops): 11=运行态 3=写次数 43/44/45=调试字 48=步骤号
  return {
    state = words[11],
    writes = words[3],
    dbg_lookup = words[43],
    dbg_register = words[44],
    dbg_verify = words[45],
    step = words[48],
  }
end

-- 命令帧: magic | cmd | arg0 | arg1(恒 0), 与 chaos_sup 的 fops 写入口对齐
local function send_command(arg0)
  local frame = u32le(CMD_MAGIC) .. u32le(CMD_INSTALL) .. u32le(arg0) .. u32le(0)
  local fh = io.open(DEVICE_PATH, "wb")
  if not fh then return false, "设备打不开" end
  local wrote, werr = pcall(fh.write, fh, frame)
  local closed = pcall(fh.close, fh)
  if not wrote or not closed then return false, "写入失败" end
  return true
end

local function install_step(arg0)
  local sent, err = send_command(arg0)
  if not sent then return false, err end
  local st, err2 = read_status()
  if not st then return false, err2 end
  if st.state ~= STATE_ACTIVE then
    return false, string.format("state=%d", st.state)
  end
  -- DBG 字含义(ko 侧 fops): lookup 结果 / register_app 返回值 / lookup 复核结果
  return true, string.format("d0=%X d1=%X d2=%X st=%d",
      st.dbg_lookup or 0, st.dbg_register or 0, st.dbg_verify or 0, st.step or -1)
end

-- 安装流水线: 每步一个 timer 拍(步骤之间让 miwear 的事件循环转一圈)。
-- notify_retry: 通知步骤的重试计数(等 launcher 订阅就绪)。
local notify_retry = 0
local pipeline = {
  { name = "1 部署模块",     body = function() return stage_resource(SUPERVISOR_RESOURCE, SUPERVISOR_PATH) end },
  { name = "2 部署应用图标", body = stage_icons },
  { name = "2.5 部署字体",   body = stage_fonts },
  { name = "3 加载模块", body = function()
      if module_loaded() then
        return false, "旧模块还在, 请重启"
      end
      if not exec(string.format("insmod %s chaos_sup", quote_arg(SUPERVISOR_PATH))) then
        return false, "insmod 失败"
      end
      return true
  end },
  { name = "4 检查设备", body = function()
      if not module_loaded() then return false, "/dev/chaos 不存在" end
      return true
  end },
  { name = "4.5 设置语言", body = function()
      -- 一个语言属性都没读到 => 无法判断, 按中文装
      if not lang_seen then
        set_status("语言: 未知, 按中文装", V_WARN)
        return install_step(0x18)
      end
      if lang_zh then
        set_status("语言: 中文", V_OK)
        return install_step(0x18)
      end
      set_status("语言: 英文", V_OK)
      return install_step(0x19)
  end },
  { name = "5 恢复占位", body = function() return install_step(0x0A) end },
  { name = "6 注册应用", body = function() return install_step(0x13) end },
  { name = "6.5 通知系统", body = function()
      -- 直接走固件自己的 notify 完整链(0x0CA81D18 注册表查询
      -- + 0x0CA81FB9 notify_installed), 实测这一条必成。
      -- 根因: 手动复刻这条链绕开了固件, 只能靠 0x3F 等 launcher 订阅就绪, 那是缓解不是修好。
      return install_step(0x43)
  end },
  { name = "7 发布应用", body = function() return install_step(1) end },
  { name = "8 INSTALL arg0=2 (launcher entries)", body = function() return install_step(2) end },
}

local function stop_runner(timer, ok, message)
    timer:delete()
    if runner == timer then runner = nil end
    set_status(message, ok and V_OK or V_ERR)
end

local function run_next_step(timer)
    local item = pipeline[step_index]
    if not item then
        stop_runner(timer, true, "运行完成")
        return
    end
    set_status("RUN " .. item.name)
    local ok, message, extra = pcall(item.body)
    if not ok then
        -- 失败文案不含步骤名(那一拍之前已经显示过), 一行 13-14 个汉字
        -- 是状态 Label 的容量上限, 超了会被裁切。
        stop_runner(timer, false, "失败: " .. tostring(message))
        return
    elseif message == "retry" then
        -- 重试当前步（等下一拍, 用于 notify 等 launcher 就绪）
        timer:ready()
        return
    elseif message == false then
        stop_runner(timer, false, "失败: " .. tostring(extra))
        return
    end
    step_index = step_index + 1
    if step_index <= #pipeline then
        timer:ready()   -- 立即排下一拍（中间仍有事件循环 turn）
        return
    end
    -- 流水线走完: 先发 0x22 终验命令, 再用状态字里的 dbg2 判注册是否真的生效。
    -- 判据只用状态字, 不读设备上的任何落盘文件。注册没生效时不能只说"运行完成",
    -- 否则界面会让人以为已经装好。
    install_step(0x22)
    local final_status = read_status()
    local dbg_verify = 0
    if final_status then dbg_verify = final_status.dbg_verify or 0 end
    if dbg_verify == 0 or dbg_verify == 0xFFFFFFFF then
        stop_runner(timer, false, "运行完成, 但注册未生效")
    else
        stop_runner(timer, true, "运行完成")
    end
end

local function start_runner()
    step_index = 1
    local timer = lvgl.Timer {
        period = 1000, repeat_count = -1, paused = true,
        cb = function(t)
            local ok, message = pcall(run_next_step, t)
            if not ok then
                stop_runner(t, false, "运行失败: " .. tostring(message))
            end
        end,
    }
    if not timer then return false, "cannot create timer" end
    runner = timer
    timer:resume()
    timer:ready()
    return true
end

-- ===== 界面(页面底 / 卡片 / 按钮 三级层次) =====
-- 定位与居中一律用 align 表(对象相对父居中), 且 Label **不设 w/x/y** —— 让它按文字
-- 自适应宽度, 再整体居中, 这才是视觉居中。
-- 重要: 不要用 "x/w + text_align" 那套写法 —— 实测表现是文字左对齐、整体不居中,
-- 也就是这两个属性在真机上没生效, 居中只有 align 表这一条有证据的路。
local SCR_W = lvgl.HOR_RES()
local SCR_H = lvgl.VER_RES()

local root = lvgl.Object(nil, {
  w = SCR_W, h = SCR_H,
  outline_width = 0, border_width = 0, pad_all = 0,
  bg_opa = lvgl.OPA(100), bg_color = V_DEEP,
})
-- 重要(实测): LVGL 对象默认带 SCROLLABLE, 会把手势吃掉 —— 表现就是
-- **长按表盘进不了表盘选择页**(主包与投递包都一样)。凡是本脚本创建的容器对象,
-- 一律清掉 SCROLLABLE; 需要点击的再单独加 CLICKABLE。
root:clear_flag(lvgl.FLAG.SCROLLABLE)
-- 只清 SCROLLABLE **不够**(清完长按仍然进不了表盘选择页) —— 还要 add_flag(EVENT_BUBBLE)。
-- EVENT_BUBBLE 让触摸事件**冒泡给父层**(固件的表盘容器), 系统的长按手势才收得到;
-- 固件自带表盘用的也是 CLICKABLE + EVENT_BUBBLE 这两句。
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

-- 状态卡片: 中间灰 + 1px 边框 + 圆角走 V_R(卡片里不再套卡片); 状态文字在卡片内居中
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

-- 按钮三态查表(替代 if/elseif 链)。danger 用**同色系淡底 + 同色字 + 无边框**:
--   Web 上那套写法是 bg-[#ff453a]/15 text-[#ff453a], 即 15% 透明度的红底配红字;
--   LVGL 里对应 bg_color=同色 + bg_opa=OPA(15)(底下的深灰透出来, 就是 /15 的效果)。
--   对照过两种不对的写法: "深灰底 + 红字"缺那层红底, "实心红底 + 白字"又太实。
--   primary = 强调蓝底 + 白字(主操作)
--   danger  = 红 15% 底 + 红字, 无边框
--   ghost   = 深底 + 1px 边框 + 次要文字
local BUTTON_STYLES = {
  primary = { bg = V_ACC_BG, line = V_ACC_BG, fg = V_TXT,  opa = 100, bw = 1 },
  danger  = { bg = V_ERR,    line = V_ERR,    fg = V_ERR,  opa = 15,  bw = 0 },
  ghost   = { bg = V_DEEP,   line = V_LINE2,  fg = V_TXT2, opa = 100, bw = 1 },
}

local function make_button(text, y_ofs, kind, on_clicked)
  local style = BUTTON_STYLES[kind] or BUTTON_STYLES.ghost
  local b = lvgl.Object(root, {
    w = SCR_W - 32, h = 64,
    bg_opa = lvgl.OPA(style.opa),
    bg_color = style.bg,
    border_width = style.bw,
    border_color = style.line,
    radius = V_R, pad_all = 0,
    align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = y_ofs },
  })
  b:clear_flag(lvgl.FLAG.SCROLLABLE)
  b:add_flag(lvgl.FLAG.CLICKABLE)
  b:add_flag(lvgl.FLAG.EVENT_BUBBLE)
  lvgl.Label(b, {
    text = text,
    text_color = style.fg,
    text_font = lvgl.Font(F_BODY, 24),
    align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 0 },
  })
  b:onClicked(function() local ok, msg = pcall(on_clicked); if not ok then set_status("错误: " .. tostring(msg), V_ERR) end end)
  return b
end

local run_button
detect_language()

run_button = make_button("运行", 110, "primary", function()
  if runner then set_status("正在运行"); return end
  if started_once then set_status("每次开机只能运行一次", V_WARN); return end
  if module_loaded() then
    set_status("已激活, 请重启后运行", V_WARN)
    return
  end
  started_once = true
  run_button:clear_flag(lvgl.FLAG.CLICKABLE)
  local started, err = start_runner()
  if not started then set_status("运行失败: " .. tostring(err), V_ERR) end
end)

make_button("清除重置", 186, "danger", function()
  if runner then set_status("运行中, 请等待", V_WARN); return end
  if not wipe_armed then wipe_armed = true; set_status("再按一次以确认清除", V_WARN); return end
  wipe_armed = false
  exec("rm -rf " .. DATA_DIR)
  set_status("已清除, 重启后重新运行", V_OK)
end)
