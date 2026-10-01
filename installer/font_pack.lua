-- 字体投递包: 把本容器里的 font.ttf 写进手环的一个字体槽位, 再让内核模块切过去并重新应用。
-- 整条链与安装器同源(表盘侧载 -> Lua 写 /data -> /dev/chaos 命令), 不经过蓝牙数据面,
-- 所以投递一次字体 = 侧载本包一次 + 点一下按钮, 不需要重启手表。
--
-- 槽位分配: **一个字体永远占同一个槽位**。
--   重投同一字体 = 覆写它自己那份文件; 若它此刻正在被使用, ko 的 0x30 门会拒
--   (内容没变, 拒了不打紧); 不在使用时没有任何 face 在读它。
--   绝不覆写正在用的那一份(font_apply.rs 定的红线): 固件为每个 (名字,尺寸) 建出的 face
--   会按需回读文件, 覆写正在使用的那份 = 让还活着的老脸读到别人的字节。
--   槽位归属由清单 /data/chaos/font/index.txt 决定, 格式与图标清单**逐字节同一套**:
--   恒 128 字节 = 8 行 x 16 字节, 行 = `[2 位槽号][空格][12 字节短名][换行]`, 空槽 = 16 空格。
--   分配规则: 文件没投过 -> 取**最小空槽**; 投过 -> 复用原槽位(幂等, 不堆副本); 满 8 个 -> 拒绝。
--
-- 判据(可分辨, 不靠猜): 命令发完之后回读状态字第 47 个字(设备 +0x46 = 当前池位),
-- 等于本次请求的槽位 => ko 接受了; 不等 => 被拒。被拒的四种原因在 ko 侧各有一道门:
-- 槽位非法 / 就是当前在用的那份 / 正在跑应用 / 目标文件打不开(没写全)。
--
-- 界面写法:
--   1. 定位与居中一律用 align 表(有实测证据的写法), Label 不设宽度让它自适应。
--      不要用 "x/w + text_align" —— 实测表现是这两条属性被忽略(文字左对齐、整体不居中)。
--   2. 定时器在**点按钮时才建**(与安装器同序): 表盘脚本在构建期抛错会让整棵 UI 树
--      不提交, 表现就是全黑。
--   3. 建完基础层立刻写一行状态字, 且所有可能失败的动作都过 pcall 把错误写进那一行
--      (可见的失败优于黑屏)。
--   4. 界面文案不带换行: Label 宽度自适应时, 多行文本里较短的行走会各自左对齐,
--      看起来就是"没居中"。

local lvgl = require("lvgl")

-- ===== 视觉: macOS 毛玻璃(vibrancy) =====
-- 与主包安装器共用同一套 token, 两个界面是同一种设计语言。
-- Web 上的 backdrop-blur / hover / transition-colors / 三栏布局在单屏 LVGL 上没有
-- 对应物, 按这套风格的核心理念翻译成三级层次:
--   页面底(最深 #1c1c1e) / 卡片(中间 #2c2c2e) / 按钮与高亮块(表面 #3a3a3c)。
-- 硬约束: 背景只用这三个灰阶; 强调色与状态色**只用于文字**; 边框一律 1px;
--         圆角上限 12(只有按钮破例, 见 V_R); 不要阴影、渐变与动画。
local V_DEEP  = 0x1C1C1E
local V_MID   = 0x2C2C2E
local V_SURF  = 0x3A3A3C
local V_LINE  = 0x3C3C3E
local V_LINE2 = 0x2A2A2C
local V_TXT   = 0xF2F2F2
local V_TXT2  = 0xB3B3B3
local V_TXT3  = 0x666666
local V_OK    = 0x30D158
local V_ERR   = 0xFF453A
-- 主按钮底的强调蓝: 禁止的是 #0a84ff 那类高亮色作底, 标准按钮蓝允许做背景。
-- 按钮全灰会认不出主操作, 主按钮就用这一档蓝。
local V_ACC_BG = 0x2563EB
-- 圆角 24: 高于上面 12 的上限, 按按钮实际尺寸定的刻意取值, 不要按上限改回 12。
-- 边界: 按钮高 64px, 圆角到 32(高度的一半)就成胶囊形, 那种形状不用, 不再往上加。
local V_R     = 24
-- 字体只用系统自带的 MiSans-Regular: 衬线标题在这块屏上做不到 ——
-- 唯一可能的衬线字体是侧载上来的文楷, 而它的 face 只在美化页手动点过"重新应用文楷"
-- 之后才登记; 重启后没点过就登记不上, 建 face 回退默认字体, 中文直接变空白豆腐块
-- (实测踩过)。层级改由字号与颜色建立。
local F_BODY  = "MiSans-Regular"

local DEVICE_PATH = "/dev/chaos"
local FONT_DIR = "/data/chaos/font"
-- 清单: 与图标清单(/data/chaos/icons/index.txt)格式逐字节同一套, ko 侧 font_list.rs 读它。
-- 恒 128 字节 = 8 行 x 16 字节, 行 = `[2 位槽号][空格][12 字节短名][换行]`, 空槽 = 16 空格。
local INDEX_PATH = FONT_DIR .. "/index.txt"
local INDEX_LINE = 16
local SLOT_MAX = 8
local INDEX_SIZE = INDEX_LINE * SLOT_MAX
-- SCRIPT_PATH 是固件注入的"本脚本所在目录"。写成 (SCRIPT_PATH or "") 是防御:
-- 假如它在这个容器里是 nil, 直接拼接会在**顶层**抛错, 而顶层抛错 = 整棵 UI 树不提交 = 黑屏。
local SCRIPT_BASE = SCRIPT_PATH or ""
local FONT_RESOURCE = SCRIPT_BASE .. "font.ttf"

-- 本包的字体短名(构建期替换成真名, 见 scripts/build_font_pack.py)。
-- 12 字节上限 = 4 个汉字(UTF-8); ko 的名字门允许非 ASCII(只挡控制字符)。
local FONT_NAME = "__FONT_NAME__"
-- 表盘标题里的短名(同一条, 便于在表盘列表里认出是哪个字体包)
local PACK_LABEL = "__PACK_LABEL__"
-- 表盘上那行标题(整句, 构建期替换)。默认是"字体投递 <短名>"; 打包时可以换成别的整句,
-- 但手环上一行只放得下约 16 个半角(一个汉字算 2 个), 超了会折行/截断 —— 界面上提醒。
local PACK_TITLE = "__PACK_TITLE__"

local CMD_MAGIC = 0x53484331        -- "1CHS", 与安装器同一套设备命令
local CMD_INSTALL = 2
local CMD_FONT_DELIVER = 0x30       -- 字体投递确认: 第 4 个字带目标槽位
local STATUS_MAGIC = 0x53484332     -- "2CHS"
local STATUS_SIZE = 192
local CHUNK = 0x10000               -- 每拍最多搬 64KB(与图标包同一档)
local TRY_MAX = 20                  -- 收尾重试拍数(等"正在应用"跑完)

local job = { stage = 0, slot = 0, src = nil, dst = nil, moved = 0, tries = 0 }
local status_label
local timer

-- ===== 投递逻辑(与界面无关) =====

local function set_status(text, color)
  if status_label then
    pcall(function()
      status_label:set { text = tostring(text), text_color = color or V_TXT2 }
    end)
  end
end

-- u32 小端打包: 逐字节压进 table 再拼(与主安装器/图标包同款新写法)
local function u32le(value)
  local n = math.floor(value)
  local pieces = {}
  for _ = 1, 4 do
    pieces[#pieces + 1] = string.char(n % 0x100)
    n = math.floor(n / 0x100)
  end
  return table.concat(pieces)
end

local function read_status()
  local f = io.open(DEVICE_PATH, "rb")
  if not f then return nil end
  local raw = f:read(STATUS_SIZE)
  f:close()
  if type(raw) ~= "string" or #raw ~= STATUS_SIZE then return nil end
  local words = {}
  local slot = 1
  for base = 1, STATUS_SIZE, 4 do
    local a, b, c, d = raw:byte(base, base + 3)
    words[slot] = a + b * 0x100 + c * 0x10000 + d * 0x1000000
    slot = slot + 1
  end
  if words[1] ~= STATUS_MAGIC then return nil end
  return words
end

-- 状态字 +0x46(1 基第 47 个) = 当前在用的字体槽位: 0 = 安装器那份, 1..8 = st1..st8。
-- 只用来判"要切的那一份现在在不在用"; 槽位分配靠清单, 不再靠它算。
local function live_slot()
  local w = read_status()
  if not w then return nil end
  local s = w[47]
  if type(s) ~= "number" or s < 0 or s > SLOT_MAX then return nil end
  return s
end

local function send_deliver(slot)
  local payload = u32le(CMD_MAGIC) .. u32le(CMD_INSTALL) .. u32le(CMD_FONT_DELIVER) .. u32le(slot)
  local f = io.open(DEVICE_PATH, "wb")
  if not f then return false end
  local wok = pcall(f.write, f, payload)
  pcall(f.close, f)
  return wok
end

-- ===== 清单读 / 分配 / 写(与 ko font_list.rs 同一套格式) =====
-- 读: 恒长 128 字节, 文件不存在 = 空清单(不是错误)。坏行跳过不猜。
local function index_read()
  local f = io.open(INDEX_PATH, "rb")
  if not f then return {} end
  local raw = f:read(INDEX_SIZE)
  f:close()
  if type(raw) ~= "string" or #raw < INDEX_LINE then return {} end
  local out = {}
  for i = 0, SLOT_MAX - 1 do
    local off = i * INDEX_LINE + 1
    if off + INDEX_LINE - 1 > #raw then break end
    local line = raw:sub(off, off + INDEX_LINE - 1)
    local d0, d1 = line:byte(1, 2)
    local slot = nil
    if d0 and d1 then
      local a, b = d0 - 0x30, d1 - 0x30
      if a >= 0 and a <= 9 and b >= 0 and b <= 9 then
        local v = a * 10 + b
        if v >= 1 and v <= SLOT_MAX then slot = v end
      end
    end
    if slot then
      local name = line:sub(4, 15)
      local cut = name:find("[\0-\31\127]")
      if cut then name = name:sub(1, cut - 1) end
      -- 尾部空格必须去掉: 定长记录右侧用空格补齐, 而"同名"是靠**字符串相等**判的
      -- (index_alloc 拿它跟 FONT_NAME 比) —— 留着补齐空格就永远比不中, 每次投递都会被
      -- 当成新字体占一个新槽(清单里的 `testfont   ` 不等于 `testfont`)。
      name = name:gsub("%s+$", "")
      if #name > 0 then out[#out + 1] = { slot = slot, name = name } end
    end
  end
  return out
end

-- 分配槽位: 同名(即同一个字体)复用原槽 -> 幂等; 否则取最小空槽; 空槽没有 => 满。
-- 返回 slot, reuse(是不是复用同名的原槽)。**注意调用处必须恰好收两个值** ——
-- Lua 里 `local slot, reuse = f()` 会把多出来的第 3 个返回值丢弃, 而写成
-- `job.slot, job.reuse = f()` 会把第 3 个塞进后面的字段(实测错过一次)。
local function index_alloc(entries)
  for i = 1, #entries do
    if entries[i].name == FONT_NAME then return entries[i].slot, true end
  end
  local used = {}
  for i = 1, #entries do used[entries[i].slot] = true end
  for s = 1, SLOT_MAX do
    if not used[s] then return s, false end
  end
  return nil, false
end

-- 写: 整块恒长写回(空槽 = 16 空格)。定长意味着不需要截断, 写失败时旧文件也还是完整的。
local function index_write(entries)
  local buf = {}
  for i = 1, SLOT_MAX do buf[i] = string.rep(" ", INDEX_LINE) end
  local n = 0
  for i = 1, #entries do
    n = n + 1
    if n > SLOT_MAX then break end
    local e = entries[i]
    if e.name ~= FONT_NAME and #e.name > 12 then e.name = e.name:sub(1, 12) end
    local name = e.name .. string.rep(" ", 12 - #e.name)
    buf[n] = string.format("%02d %s\n", e.slot, name)
  end
  local f, err = io.open(INDEX_PATH, "wb")
  if not f then return false, err end
  local ok = pcall(f.write, f, table.concat(buf))
  pcall(f.close, f)
  return ok
end

local function close_files()
  if job.src then pcall(function() job.src:close() end) job.src = nil end
  if job.dst then pcall(function() job.dst:close() end) job.dst = nil end
end

local function finish(ok, text)
  job.stage = 0
  close_files()
  if timer then pcall(function() timer:pause() end) end
  set_status(text, ok and V_OK or V_ERR)
end

local function step()
  -- 1) 准备: 读清单 -> 分配槽位 -> 打开源与目标
  if job.stage == 1 then
    local cur = live_slot()
    if cur == nil then
      finish(false, "读不到模块状态, 先跑一次主包")
      return
    end
    local src = io.open(FONT_RESOURCE, "rb")
    if not src then
      finish(false, "容器里没有 font.ttf")
      return
    end
    -- 目录必须先保证存在, 否则下面开目标文件就报"打不开目标文件"(实测踩过)。
    -- 根因: 这个目录本来由安装器 stage_fonts 里那句 mkdir -p 建, 而字体移出主包之后
    -- 那一步整段跳过, 于是再没有人建 /data/chaos/font。
    -- 写法与安装器同款(os.execute "mkdir -p"), 失败也不抛错。
    pcall(function() os.execute("mkdir -p " .. FONT_DIR) end)
    local entries = index_read()
    -- 多值返回必须显式接住: 直接写 `job.slot, job.reuse = index_alloc(...)` 会把
    -- 第 3 个返回值塞进表里的下一个字段, 而且 job.slot 会是 nil(实测错过一次)。
    local slot, reuse = index_alloc(entries)
    if not slot then
      src:close()
      finish(false, "槽位已满(" .. SLOT_MAX .. " 个), 先在界面删一个")
      return
    end
    job.slot, job.reuse, job.entries = slot, reuse, entries
    local dst, derr = io.open(FONT_DIR .. "/st" .. job.slot .. ".ttf", "wb")
    if not dst then
      src:close()
      finish(false, "打不开目标文件: " .. tostring(derr))
      return
    end
    job.src, job.dst, job.moved = src, dst, 0
    set_status((reuse and "覆盖 " or "写入 ") .. "槽 " .. job.slot .. " ...")
    job.stage = 2
    return
  end

  -- 2) 搬运: 每拍一块
  if job.stage == 2 then
    local blk = job.src:read(CHUNK)
    if blk and #blk > 0 then
      if not pcall(job.dst.write, job.dst, blk) then
        finish(false, "写失败, 已写 " .. job.moved .. " 字节")
        return
      end
      job.moved = job.moved + #blk
      set_status("写入槽 " .. job.slot .. " " .. math.floor(job.moved / 1024) .. "KB")
      return
    end
    -- 读完: 先把目标文件关掉(ko 会去开它验在不在), 再进收尾
    local cok = pcall(job.dst.close, job.dst)
    job.dst = nil
    pcall(function() job.src:close() end)
    job.src = nil
    if (not cok) or job.moved <= 0 then
      finish(false, "文件没写全(" .. job.moved .. " 字节)")
      return
    end
    -- 文件写完再登记清单: 顺序反过来会出现"清单里有、文件是空的"这种半截状态。
    -- 复用原槽时清单内容不变, 也照样写一遍(幂等, 恒长写)。
    local entries = job.entries or {}
    local found = false
    for i = 1, #entries do
      if entries[i].slot == job.slot then entries[i].name = FONT_NAME; found = true end
    end
    if not found then entries[#entries + 1] = { slot = job.slot, name = FONT_NAME } end
    local wok, werr = index_write(entries)
    if not wok then
      finish(false, "清单写不进: " .. tostring(werr))
      return
    end
    job.stage, job.tries = 3, 0
    return
  end

  -- 3) 收尾: 发命令 -> 回读槽位是否变成我们请求的那个
  if job.stage == 3 then
    job.tries = job.tries + 1
    send_deliver(job.slot)
    local now = live_slot()
    if now == job.slot then
      finish(true, "已投递 " .. math.floor(job.moved / 1024) .. "KB, 正在重建字体")
      return
    end
    if job.tries >= TRY_MAX then
      finish(false, "模块没接受(槽位未变)")
      return
    end
    set_status("等待接收槽 " .. job.slot .. " (" .. job.tries .. ")")
    return
  end
end

-- 点一下按钮: 这时才建定时器(与安装器同序), 建不出来就写在屏幕上
local function start()
  if job.stage ~= 0 then return end
  if not timer then
    local created = lvgl.Timer {
      period = 200, repeat_count = -1, paused = true,
      cb = function(ti)
        local ok, err = pcall(step)
        if not ok then
          finish(false, "内部错误: " .. tostring(err))
          return
        end
        if job.stage ~= 0 then ti:ready() end   -- 立即排下一拍, 不等满一个周期
      end,
    }
    if not created then
      set_status("定时器创建失败", V_ERR)
      return
    end
    timer = created
  end
  job.stage, job.moved, job.tries = 1, 0, 0
  pcall(function() timer:resume() end)
  set_status("开始投递 " .. PACK_LABEL .. " ...")
end

-- ===== 界面(与主包安装器同一套设计语言) =====
-- 定位与居中一律用 align 表(对象相对父居中), Label **不设 w/x/y** 让它按文字自适应。
-- 重要: 不要用 "x/w + text_align" 那套写法 —— 实测表现是文字左对齐、整体不居中,
-- 也就是这两条属性在设备上根本没生效; 居中只有 align 表这一条有实测证据的路。

local W = lvgl.HOR_RES()
local H = lvgl.VER_RES()

local root = lvgl.Object(nil, {
  w = W, h = H,
  outline_width = 0, border_width = 0, pad_all = 0,
  bg_opa = lvgl.OPA(100), bg_color = V_DEEP,
})
-- 重要(实测): 容器必须凑齐三件事, 少一件长按表盘就进不了表盘选择页。
--   1. clear_flag(SCROLLABLE) —— LVGL 对象默认带这一位, 会吃掉手势;
--   2. add_flag(EVENT_BUBBLE) —— **只清 SCROLLABLE 不够**(清完仍然无效),
--      事件要冒泡给父层(固件的表盘容器), 系统长按手势才收得到;
--   3. root 自己也 CLICKABLE —— 空白处按下时得有个接收者, 否则事件无处可冒。
-- 固件自带表盘用的 event_mask 同样是 CLICKABLE + EVENT_BUBBLE 这两句。
root:clear_flag(lvgl.FLAG.SCROLLABLE)
root:add_flag(lvgl.FLAG.CLICKABLE)
root:add_flag(lvgl.FLAG.EVENT_BUBBLE)

-- 标题: 字号与颜色建立层级(不用衬线, 原因见上面 F_BODY 的说明)。
-- 标题整句来自构建期替换的值(默认带字体短名): 表盘列表里可能同时存在好几个字体包,
-- 打开之前就要能认出是哪一个。
lvgl.Label(root, {
  text = PACK_TITLE,
  text_color = V_TXT,
  text_font = lvgl.Font(F_BODY, 40),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -170 },
})

-- 状态卡片: 中间灰 + 1px 边框 + 圆角走 V_R; 状态文字在卡片内居中
local card = lvgl.Object(root, {
  w = W - 32, h = 150,
  bg_opa = lvgl.OPA(100), bg_color = V_MID,
  border_width = 1, border_color = V_LINE2,
  radius = V_R, pad_all = 0,
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -20 },
})
card:clear_flag(lvgl.FLAG.SCROLLABLE)
card:add_flag(lvgl.FLAG.EVENT_BUBBLE)
status_label = lvgl.Label(card, {
  text = "已加载",
  text_color = V_TXT2,
  text_font = lvgl.Font(F_BODY, 22),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 0 },
})

-- 主按钮: 强调蓝底 + 白字(与安装器同一种颜色与圆角)
local btn = lvgl.Object(root, {
  w = W - 32, h = 64,
  bg_color = V_ACC_BG, bg_opa = lvgl.OPA(100),
  border_width = 1, border_color = V_ACC_BG,
  radius = V_R, pad_all = 0,
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 110 },
})
btn:clear_flag(lvgl.FLAG.SCROLLABLE)
btn:add_flag(lvgl.FLAG.CLICKABLE)
btn:add_flag(lvgl.FLAG.EVENT_BUBBLE)
lvgl.Label(btn, {
  text = "投递字体",
  text_color = V_TXT,
  text_font = lvgl.Font(F_BODY, 24),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 0 },
})
btn:onClicked(function()
  local ok, err = pcall(start)
  if not ok then set_status("启动失败: " .. tostring(err), V_ERR) end
end)
