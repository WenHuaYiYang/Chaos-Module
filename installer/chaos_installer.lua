-- chaos_sup 原生应用安装器
-- 流程约束：
-- 1. 不做 rmmod。模块已加载时只更新磁盘文件，重启后加载更新。
--    根因: rmmod 释放模块内存，但 app 注册链表的条目留在原地悬空 → 切表盘时
--    launcher 遍历到悬空回调 → 崩。
-- 2. 每条命令一个独立 timer 拍，让 miwear 的事件循环在两个注册阶段之间转一圈。
-- 3. 文件部署通过协程分批推进；各注册步骤仍隔开事件循环。
-- 4. 完整状态与错误保留在有上限的内存日志，不逐条写闪存。

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

local SUPERVISOR_RESOURCE = SCRIPT_PATH .. "chaos_sup.bin"
local SUPERVISOR_PATH = "/data/chaos/sup.ko"
local FETCH_RESOURCE = SCRIPT_PATH .. "fetch-module.bin"
local FETCH_PATH = "/data/chaos/fetch-module.ko"
local CAPATCH_RESOURCE = SCRIPT_PATH .. "capatch.bin"
local CAPATCH_PATH = "/data/ssl/capatch.elf"
local PEM_RESOURCE = SCRIPT_PATH .. "ca.pem"
local PEM_PATH = "/data/ssl/ca.pem"
local SSL_DIR = "/data/ssl"
local ICON_RESOURCE = SCRIPT_PATH .. "chaos_icon.bin"
local SETTINGS_ICON_RESOURCE = SCRIPT_PATH .. "chaos_icon_settings.bin"
local ICON_PATH = "/data/chaos/chaos_icon.bin"
local SETTINGS_ICON_PATH = "/data/chaos/chaos_icon_settings.bin"
local DATA_DIR = "/data/chaos"
local DEVICE_PATH = "/dev/chaos"

local CMD_MAGIC = 0x53484331           -- "1CHS"
local STATUS_MAGIC = 0x53484332        -- "2CHS"
local STATUS_VERSION = 0x43485302      -- "CHS2"
local CMD_BOOT_DQ = 0x53484333
local CMD_INSTALL = 2
local STATUS_SIZE = 192
local STATE_ACTIVE = 1

local status_label
-- 运行器状态: runner = 当前执行中的 timer;
-- started_once = 本次会议跑过没有(一个模块只能装一次, 跑过必须重启);
-- wipe_armed = 清除按钮的两段式确认闸。
local runner
local settings_enabled = true
local system_update_enabled = false
local launcher_enabled = true
-- fetch 自启动独立选择；没有明确保存的开启配置时默认关闭。
local FETCH_CONFIG = DATA_DIR .. "/fetch.cfg"
local fetch_enabled = false
local fetch_reboot_needed = false
local as_set, settings_refresh
local runner_mode
local settings_visible = false
local started_once = false
local wipe_armed = false

-- 状态行是单行居中(卡片 304 宽 / 22px 字): 超宽会被屏幕裁掉, 显示前按"半角宽度"截断。
-- 中文/全角算 2 个单位, ASCII 算 1; 上限 26 个单位 = 13 个汉字 = 286px。
-- 拼接出来的消息(如 "更新失败: " .. err)单条看都不长、拼起来会超, 所以截断放在显示层。
local function fit_status(text)
  local s = tostring(text)
  local units, i = 0, 1
  while i <= #s do
    local b = s:byte(i)
    local len = (b >= 0xF0 and 4) or (b >= 0xE0 and 3) or (b >= 0xC0 and 2) or 1
    local w = (len >= 3) and 2 or 1
    if units + w > 26 then return s:sub(1, i - 1) .. "…" end
    units = units + w
    i = i + len
  end
  return s
end

-- 日志按显示宽度拆行，最多保留 96 行；不保留无限增长的错误字符串。
local log_lines = {}
local log_label, log_title
local log_page = 1
local log_visible = false
local LOG_ROWS = 8
local function refresh_log()
  if not log_label then return end
  local pages = math.max(1, math.ceil(#log_lines / LOG_ROWS))
  log_page = math.max(1, math.min(log_page, pages))
  local lines = {}
  for i = (log_page - 1) * LOG_ROWS + 1, math.min(log_page * LOG_ROWS, #log_lines) do
    lines[#lines + 1] = log_lines[i]
  end
  log_title:set { text = "安装日志 " .. log_page .. "/" .. pages }
  log_label:set { text = #lines > 0 and table.concat(lines, "\n") or "暂无日志" }
end
local function append_log(message)
  local text = tostring(message):gsub("[\r\n]", " ")
  local first, i, units, rows = 1, 1, 0, 0
  while i <= #text and rows < 12 do
    local byte = text:byte(i)
    local length = byte >= 0xF0 and 4 or byte >= 0xE0 and 3 or byte >= 0xC0 and 2 or 1
    local width = length >= 3 and 2 or 1
    if units + width > 26 then
      log_lines[#log_lines + 1] = text:sub(first, i - 1)
      first, units, rows = i, 0, rows + 1
    end
    units = units + width; i = i + length
  end
  if first < i then log_lines[#log_lines + 1] = text:sub(first, i - 1) end
  if i <= #text then log_lines[#log_lines + 1] = "（该条日志过长，已截断）" end
  while #log_lines > 96 do table.remove(log_lines, 1) end
  if log_visible then refresh_log() end
end
local function set_status(text, color)
  append_log(text)
  if status_label then status_label:set { text = fit_status(text), text_color = color or V_TXT2 } end
  if runner_mode == "fetch" and settings_visible and as_set then as_set(text, color, true) end
end
local function cooperate()
  local thread, main = coroutine.running()
  if thread and not main then coroutine.yield() end
end

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
  append_log(cmd)
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
  -- 一条命令只删一个路径: NuttX NSH 的 rm 不接受多个参数(会回
  -- "nsh: rm: too many arguments"), 之前这里一次删两个, 模拟器上安装流程
  -- 直接断在这一步, 后面 insmod 与注册链全没跑到。
  exec("rm -f " .. LANG_OUTPUT)
  exec("rm -f /data/chaos/allprop.txt")
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
  -- 一条命令只删一个路径(NSH 的 rm 不接受多参数, 见上面语言探测处的说明)
  exec("rm -f " .. FONT_DIR .. "/MiSans-*.ttf")
  exec("rm -f " .. FONT_DIR .. "/MiSansF-*.ttf")
  exec("rm -f " .. FONT_DIR .. "/BaiJamjuree-*.ttf")
  for _, n in ipairs(FONT_NAMES) do
    local f = io.open(FONT_DIR .. "/" .. n, "wb")
    if not f then return false, "open " .. n end
    local ok = pcall(f.write, f, c); f:close()
    if not ok then return false, "write " .. n end
  end
  return true
end
-- 模块已涨到 1.1MB 以上。整份读进 Lua 字符串再整份读回校验, 峰值是文件大小的
-- 两三倍; 更新时模块本体还驻留在内存里, 这点增量足以把 Lua 堆顶爆(表现是点
-- 「运行」当场卡死)。这里改成 64KB 分块: 峰值只多一个块, 内容相同就跳过写盘。
local COPY_CHUNK = 65536
local function chunked_compare(a_path, b_path)
  local a = io.open(a_path, "rb")
  local b = io.open(b_path, "rb")
  if not a or not b then
    if a then a:close() end
    if b then b:close() end
    return nil
  end
  local ok, same = pcall(function()
    while true do
      local x, xe = a:read(COPY_CHUNK)
      local y, ye = b:read(COPY_CHUNK)
      if xe or ye then return nil end
      if x ~= y then return false end -- 第一块差异即结束，无需继续读完整文件。
      if x == nil then return true end
      cooperate()
    end
  end)
  a:close(); b:close()
  if not ok then error(same) end
  return same
end
local function stage_resource(res, dest)
  append_log("检查 " .. dest)
  exec("mkdir -p " .. DATA_DIR)
  if chunked_compare(res, dest) == true then
    append_log("内容相同，跳过写入")
    return true
  end
  local src = io.open(res, "rb"); if not src then return false, "cannot read " .. res end
  local out = io.open(dest, "wb"); if not out then src:close(); return false, "cannot open " .. dest end
  local copied = 0
  local ok, wrote, why = pcall(function()
    while true do
      local chunk, err = src:read(COPY_CHUNK)
      if err then return false, "读取失败 " .. res end
      if chunk == nil then break end
      local value = out:write(chunk)
      if not value then return false, "write failed " .. dest end
      copied = copied + #chunk
      if copied % (COPY_CHUNK * 4) == 0 then append_log("已写入 " .. copied .. " 字节") end
      cooperate()
    end
    return true
  end)
  local closed, result = pcall(out.close, out)
  src:close()
  if not ok then return false, tostring(wrote) end
  if not wrote or not closed or not result then return false, why or "write failed" end
  append_log("回读校验 " .. dest)
  cooperate()
  if chunked_compare(res, dest) ~= true then return false, "写入校验失败 " .. dest end
  append_log("校验通过 " .. copied .. " 字节")
  return true
end
-- 只读 52 字节的 ELF 头, 不把整份模块读进内存。
local function resource_is_elf(path)
  local f = io.open(path, "rb")
  if not f then return false end
  local head = f:read(52) or ""
  f:close()
  -- read(52) 在真机上最多回 52 字节；测试替身会整份返回，所以按"至少 52"判。
  return #head >= 52 and head:sub(1, 4) == string.char(127) .. "ELF"
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
  local ok, err = stage_resource(ICON_RESOURCE, ICON_PATH)
  if not ok then return ok, err end
  return stage_resource(SETTINGS_ICON_RESOURCE, SETTINGS_ICON_PATH)
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

local RC_DIR = "/data/rc.d"
local RC_SH = RC_DIR .. "/chaos.sh"
local RC_FETCH_SH = RC_DIR .. "/chaos-fetch.sh"
local BOOT_FRAME = DATA_DIR .. "/boot.bin"
local AS_LOG = DATA_DIR .. "/autostart.log"
local FETCH_AS_LOG = DATA_DIR .. "/fetch-autostart.log"
local LEGACY_FLAG = DATA_DIR .. "/autostart.on"
local LEGACY_CHAOS_RC = DATA_DIR .. "/rc"
local LEGACY_RC_PATH = "/data/rc"
local LEGACY_HOP = "sh /data/chaos/rc &"
local function write_file(path, body)
  if read_all(path) == body then return true end
  local f = io.open(path, "wb")
  if not f then return false, "无法写入 " .. path end
  local ok, value = pcall(f.write, f, body)
  local closed, result = pcall(f.close, f)
  if not ok or value == false or not closed or result == false then return false, "写入失败" end
  if read_all(path) ~= body then return false, "回读核对失败" end
  return true
end

local function deploy_https()
  if not resource_is_elf(CAPATCH_RESOURCE) then return false, "安装包 capatch 模块无效" end
  local pem = io.open(PEM_RESOURCE, "rb")
  if not pem then return false, "安装包 CA 证书缺失" end
  local head = pem:read(1); pem:close()
  if not head then return false, "安装包 CA 证书无效" end
  exec("mkdir -p " .. SSL_DIR)
  local ok, err = stage_resource(CAPATCH_RESOURCE, CAPATCH_PATH)
  if not ok then return false, err end
  return stage_resource(PEM_RESOURCE, PEM_PATH)
end
local sync_fetch_autostart
-- 已加载的模块保持原样，仅部署下次启动的模块文件。
-- 更新也走逐拍流水线: 模块的读写不能挤在一个定时器拍里, 否则 UI 循环
-- 被占住、看门狗复位, 表现同样是点「运行」卡死。
local send_settings_command
local send_command
local update_pipeline = {
  { name = "1 部署模块", body = function()
      if not resource_is_elf(SUPERVISOR_RESOURCE) then return false, "安装包模块无效" end
      if not resource_is_elf(FETCH_RESOURCE) then return false, "安装包 fetch 模块无效" end
      return stage_resource(SUPERVISOR_RESOURCE, SUPERVISOR_PATH)
  end },
  { name = "1.5 部署fetch", body = function()
      return stage_resource(FETCH_RESOURCE, FETCH_PATH)
  end },
  { name = "1.6 部署HTTPS", body = deploy_https },
  { name = "1.7 fetch自启动", body = function() return sync_fetch_autostart() end },
  { name = "2 保存选项", body = function()
      local ok, err = write_file(DATA_DIR .. "/settings.cfg", string.char(settings_enabled and 1 or 0))
      if not ok then return false, err end
      ok, err = write_file(DATA_DIR .. "/system_update.cfg", string.char(system_update_enabled and 1 or 0))
      if not ok then return false, err end
      return write_file(DATA_DIR .. "/launcher.cfg", string.char(launcher_enabled and 1 or 0))
  end },
  { name = "3 应用选项", body = function()
      if not module_loaded() then return true end
      if settings_enabled then
        local sent, err = send_settings_command()
        if not sent then return false, err end
      end
      if system_update_enabled then
        local sent, err = send_command(0x45)
        if not sent then return false, err end
      end
      return send_command(0x46, launcher_enabled and 1 or 0)
  end },
}

local function build_fetch_sh()
  local L = {}
  local function w(s) L[#L + 1] = s end
  -- 特权脚本只负责网络恢复，不能把 Chaos 提前到 launcher 就绪之前。
  w("# top")
  w("set +e")
  w("echo start > " .. FETCH_AS_LOG)
  w("insmod " .. FETCH_PATH .. " tun > /data/chaos/fetch-insmod.log")
  w("echo fetch_insmod >> " .. FETCH_AS_LOG)
  w("insmod " .. CAPATCH_PATH .. " capatch > /data/chaos/capatch-insmod.log")
  w("echo capatch_insmod >> " .. FETCH_AS_LOG)
  for level = 1, 4 do
    w("echo " .. level .. " > /dev/tunentry")
    w("cat /dev/tunsvc > /dev/null")
    w("echo fetch_arm" .. level .. " >> " .. FETCH_AS_LOG)
  end
  w("echo done >> " .. FETCH_AS_LOG)
  return table.concat(L, "\n") .. "\n"
end

local function build_chaos_sh()
  local L = {}
  local function w(s) L[#L + 1] = s end
  -- 普通优先级：等系统及 launcher 就绪后再注册 Chaos。
  w("set +e")
  w("echo start > " .. AS_LOG)
  w("insmod " .. SUPERVISOR_PATH .. " chaos_sup")
  w("echo insmod >> " .. AS_LOG)
  w("dd if=" .. DEVICE_PATH .. " of=" .. DATA_DIR .. "/stage1.bin bs="
    .. STATUS_SIZE .. " count=1 conv=notrunc")
  w("dd if=" .. BOOT_FRAME .. " of=" .. DEVICE_PATH .. " bs=16 count=1 conv=notrunc")
  w("echo boot_cmd_sent >> " .. AS_LOG)
  w("dd if=" .. DEVICE_PATH .. " of=" .. DATA_DIR .. "/stage3.bin bs="
    .. STATUS_SIZE .. " count=1 conv=notrunc")
  w("echo done >> " .. AS_LOG)
  return table.concat(L, "\n") .. "\n"
end

local function build_boot_frame()
  return u32le(CMD_MAGIC) .. u32le(CMD_BOOT_DQ) .. u32le(0) .. u32le(0)
end

local function remove_file(path)
  if read_all(path) == nil then return true end
  exec("rm -f " .. path)
  if read_all(path) ~= nil then return false, "删除失败 " .. path end
  return true
end

local function directory_writable(path)
  local check = path .. "/.chaos-check"
  local ok, err = write_file(check, "ok")
  local cleaned, why = remove_file(check)
  if not cleaned then return false, why end
  return ok, err
end

local function rc_dir_ready()
  local ok = directory_writable(RC_DIR)
  if ok then return true end
  if directory_writable("/data") then return false, "先安装自启动管理器" end
  return false, "写不进 /data，请检查存储"
end

local function without_legacy_hop(raw)
  if type(raw) ~= "string" then return raw end
  local keep, start = {}, 1
  while start <= #raw do
    local last = raw:find("\n", start, true) or #raw
    local line = raw:sub(start, last)
    if line:gsub("[\r\n]+$", ""):gsub("[ \t]+$", "") ~= LEGACY_HOP then
      keep[#keep + 1] = line
    end
    start = last + 1
  end
  return table.concat(keep)
end

local function legacy_cleanup()
  local raw = read_all(LEGACY_RC_PATH)
  local cleaned = without_legacy_hop(raw)
  if cleaned ~= raw then
    local ok, err = write_file(LEGACY_RC_PATH, cleaned)
    if not ok then return false, err end
  end
  local ok, err = remove_file(LEGACY_FLAG)
  if not ok then return false, err end
  return remove_file(LEGACY_CHAOS_RC)
end

sync_fetch_autostart = function()
  if fetch_enabled then
    local ready, why = rc_dir_ready()
    if not ready then return false, why end
    local ok, err = write_file(RC_FETCH_SH, build_fetch_sh())
    if not ok then return false, err end
  else
    local ok, err = remove_file(RC_FETCH_SH)
    if not ok then return false, err end
  end
  local ok, err = write_file(FETCH_CONFIG, string.char(fetch_enabled and 1 or 0))
  if ok then append_log(fetch_enabled and "fetch自启动已开启，重启后生效" or "fetch自启动已关闭，已加载模块重启后停用") end
  return ok, err
end

local function autostart_install()
  local ready, why = rc_dir_ready()
  if not ready then return false, why end
  -- 资源部署失败时不留下指向未完成安装的 Chaos 开机入口。
  local removed, reason = remove_file(RC_SH)
  if not removed then return false, reason end
  for _, path in ipairs({ DATA_DIR, FONT_DIR, ICON_DIR, SSL_DIR }) do
    exec("mkdir -p " .. path)
    local ok, err = directory_writable(path)
    if not ok then return false, "目录不可写 " .. path .. ": " .. tostring(err) end
  end
  local resources = {
    { SUPERVISOR_RESOURCE, SUPERVISOR_PATH }, { FETCH_RESOURCE, FETCH_PATH },
    { ICON_RESOURCE, ICON_PATH }, { SETTINGS_ICON_RESOURCE, SETTINGS_ICON_PATH },
  }
  for _, resource in ipairs(resources) do
    local ok, err = stage_resource(resource[1], resource[2])
    if not ok then return false, err end
  end
  local ok, err = deploy_https()
  if not ok then return false, err end
  ok, err = write_file(BOOT_FRAME, build_boot_frame())
  if not ok then return false, err end
  ok, err = legacy_cleanup()
  if not ok then return false, err end
  ok, err = write_file(RC_SH, build_chaos_sh())
  if not ok then return false, err end
  return sync_fetch_autostart()
end
local function autostart_remove()
  local ok, err = remove_file(RC_SH)
  if not ok then return false, err end
  ok, err = remove_file(BOOT_FRAME)
  if not ok then return false, err end
  return legacy_cleanup()
end
local function autostart_registered()
  return read_all(RC_SH) == build_chaos_sh()
    and read_all(BOOT_FRAME) == build_boot_frame()
end
local function autostart_state_text()
  if autostart_registered() then
    return "已注册，开关在管理器"
  end
  return "尚未注册自启动"
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

-- 命令帧: magic | cmd | arg0 | arg1(默认 0)，与 chaos_sup 的 fops 写入口对齐
send_settings_command = function()
  local frame = u32le(0x53505331) .. u32le(0x5351001B) .. u32le(0x04350901) .. u32le(0)
  local fh = io.open(DEVICE_PATH, "wb")
  if not fh then return false, "设备打不开" end
  local ok, result = pcall(fh.write, fh, frame)
  local closed, close_result = pcall(fh.close, fh)
  if not ok or result == false or not closed or close_result == false then return false, "settings 命令写入失败" end
  return true
end
send_command = function(arg0, arg1)
  local frame = u32le(CMD_MAGIC) .. u32le(CMD_INSTALL) .. u32le(arg0) .. u32le(arg1 or 0)
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

local FETCH_BUILD = "v108-20261024-dbg-flag"
local function fetch_state()
  local status = read_all("/dev/tunentry", "r")
  local running = type(status) == "string" and status:match("mt_build=([^%s]+)") or nil
  local armed = type(status) == "string" and status:find("mt_armed=1", 1, true) ~= nil
  local cap_status = read_all("/dev/capatch", "r")
  local https_ok = type(cap_status) == "string" and cap_status:find("pret=0", 1, true) ~= nil
  return running, armed, https_ok
end

local function ensure_fetch()
  if not fetch_enabled then append_log("fetch自启动关闭，跳过加载检查"); return true end
  -- 仅在下一次开机由脚本早期加载；安装器不在当前会话补 insmod。
  local running, armed, https_ok = fetch_state()
  if running == FETCH_BUILD and armed and https_ok then return true end
  fetch_reboot_needed = true
  local reason = not running and "fetch尚未加载" or running ~= FETCH_BUILD and "fetch版本待更新"
    or not armed and "fetch早期布防待完成" or "HTTPS注入待完成"
  set_status(reason .. "，重启后生效", V_WARN)
  return true
end
-- 安装流水线: 每步一个 timer 拍(步骤之间让 miwear 的事件循环转一圈)。
local pipeline = {
  { name = "1 部署模块",     body = function() return stage_resource(SUPERVISOR_RESOURCE, SUPERVISOR_PATH) end },
  { name = "1.5 部署fetch", body = function() return stage_resource(FETCH_RESOURCE, FETCH_PATH) end },
  { name = "1.6 部署HTTPS", body = deploy_https },
  { name = "1.7 fetch自启动", body = function() return sync_fetch_autostart() end },
  { name = "2 部署应用图标", body = stage_icons },
  { name = "2.5 部署字体",   body = stage_fonts },
  { name = "3 检查旧模块", body = function()
      if module_loaded() then return false, "旧模块还在, 请重启" end
      return true
  end },
  -- 无自启动时跳过 fetch；有自启动时只验证早期加载结果，然后继续注入 Chaos。
  { name = "3.1 检查/跳过fetch", body = ensure_fetch },
  { name = "4 加载Chaos本体", body = function()
      if not exec(string.format("insmod %s chaos_sup", quote_arg(SUPERVISOR_PATH))) then
        return false, "Chaos insmod 失败"
      end
      return true
  end },
  { name = "4.1 检查设备", body = function()
      if not module_loaded() then return false, "/dev/chaos 不存在" end
      return true
  end },
  { name = "4.2 设置语言", body = function()
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
  { name = "5.5 应用列表选项", body = function()
      local ok, err = write_file(DATA_DIR .. "/launcher.cfg",
          string.char(launcher_enabled and 1 or 0))
      if not ok then return false, "无法保存应用列表选项: " .. tostring(err) end
      -- 注册前把选项交给原生侧；关闭时仍注册 Chaos，但不加入 Launcher。
      return send_command(0x46, launcher_enabled and 1 or 0)
  end },
  { name = "6 注册应用", body = function() return install_step(0x13) end },
  { name = "6.4 注入 settings", body = function()
      local fh = io.open(DATA_DIR .. "/settings.cfg", "wb")
      if not fh then return false, "无法保存 settings 选项" end
      fh:write(string.char(settings_enabled and 1 or 0)); fh:close()
      if settings_enabled then return send_settings_command() end
      return true
  end },
  { name = "6.45 注入系统更新入口", body = function()
      local ok, err = write_file(DATA_DIR .. "/system_update.cfg",
          string.char(system_update_enabled and 1 or 0))
      if not ok then return false, "无法保存系统更新选项: " .. tostring(err) end
      if system_update_enabled then return send_command(0x45) end
      return true
  end },
  { name = "6.5 通知系统", body = function()
      -- 直接走固件自己的 notify 完整链(0x0CA81D18 注册表查询
      -- + 0x0CA81FB9 notify_installed), 实测这一条必成。
      -- 根因: 手动复刻这条链绕开了固件, 只能靠 0x3F 等 launcher 订阅就绪, 那是缓解不是修好。
      return install_step(0x43)
  end },
  { name = "7 发布应用", body = function() return install_step(1) end },
  { name = "8 INSTALL arg0=2 (launcher entries)", body = function() return install_step(2) end },
}

local active_step
local selected_pipeline
local active_index = 1
local function stop_runner(timer, ok, message)
  timer:delete()
  if runner == timer then runner = nil end
  active_step = nil
  if not ok and runner_mode == "fetch" then
    fetch_enabled = read_all(FETCH_CONFIG, "rb") == string.char(1)
  end
  set_status(message, ok and V_OK or V_ERR)
  if runner_mode == "fetch" and settings_refresh then settings_refresh() end
end

local function run_next_step(timer)
  local item = selected_pipeline[active_index]
  if not item then
    if runner_mode == "autostart" then
      stop_runner(timer, true, "已注册，请到管理器重建")
    elseif runner_mode == "fetch" then
      stop_runner(timer, true, fetch_enabled and "fetch已开启，重启生效" or "fetch已关闭，重启生效")
    elseif runner_mode == "update" then
      stop_runner(timer, true, "模块已更新，请重启")
    else
      install_step(0x22)
      local final_status = read_status()
      local verify = final_status and final_status.dbg_verify or 0
      if verify == 0 or verify == 0xFFFFFFFF then
        stop_runner(timer, false, "运行完成, 但注册未生效")
      else
        stop_runner(timer, true, fetch_reboot_needed and "安装完成，fetch重启生效" or "运行完成")
      end
    end
    return
  end
  if not active_step then
    set_status(item.name)
    active_step = coroutine.create(item.body)
    return -- 先给状态文字一帧绘制机会，再执行该步骤。
  end
  local ok, result, why = coroutine.resume(active_step)
  if not ok then stop_runner(timer, false, "失败: " .. tostring(result)); return end
  if coroutine.status(active_step) ~= "dead" then return end
  active_step = nil
  if result == false then stop_runner(timer, false, "失败: " .. tostring(why)); return end
  if result ~= "retry" then active_index = active_index + 1 end
end

local function start_runner(mode)
  if runner then return false, "正在运行" end
  runner_mode = mode == true and "update" or mode or "install"
  selected_pipeline = runner_mode == "update" and update_pipeline or pipeline
  if runner_mode == "autostart" then
    selected_pipeline = { { name = "部署自启动", body = autostart_install } }
  elseif runner_mode == "fetch" then
    selected_pipeline = {
      { name = "准备fetch组件", body = function()
          if not fetch_enabled then return true end
          local ok, err = stage_resource(FETCH_RESOURCE, FETCH_PATH)
          if not ok then return false, err end
          return deploy_https()
      end },
      { name = "保存fetch自启动", body = function() return sync_fetch_autostart() end },
    }
  end
  active_index = 1; active_step = nil; fetch_reboot_needed = false
  set_status("正在准备，请稍候", V_ACC)
  local timer = lvgl.Timer {
    period = 20, repeat_count = -1, paused = true,
    cb = function(t)
      local ok, message = pcall(run_next_step, t)
      if not ok then stop_runner(t, false, "运行失败: " .. tostring(message)) end
    end,
  }
  if not timer then return false, "cannot create timer" end
  runner = timer
  timer:resume() -- 不 ready：确保先绘制点击反馈，再处理文件。
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

-- 重要: 这两个满屏层要带上与 root 同一组清零。主题给普通对象默认画 1px 边框, 而边框画在
-- 对象自己的坐标之内、纯色底让给它一圈 —— 满屏层不做这件事时, 屏幕四条边显出一圈比页底更
-- 暗的窄带, 看着就是"纯色背景没铺到边"。outline(描边画在对象之外)同理要关掉。
local OFF_Y = 3000
local main_view = lvgl.Object(root, {
  w = SCR_W, h = SCR_H, outline_width = 0, border_width = 0, pad_all = 0,
  bg_opa = lvgl.OPA(100), bg_color = V_DEEP,
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 0 },
})
local as_view = lvgl.Object(root, {
  w = SCR_W, h = SCR_H, outline_width = 0, border_width = 0, pad_all = 0,
  bg_opa = lvgl.OPA(100), bg_color = V_DEEP,
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = OFF_Y },
})
local log_view = lvgl.Object(root, {
  w = SCR_W, h = SCR_H, outline_width = 0, border_width = 0, pad_all = 0,
  bg_opa = lvgl.OPA(100), bg_color = V_DEEP,
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = OFF_Y },
})
local views = { main_view, as_view, log_view }
for i = 1, #views do
  views[i]:clear_flag(lvgl.FLAG.SCROLLABLE)
  views[i]:add_flag(lvgl.FLAG.CLICKABLE)
  views[i]:add_flag(lvgl.FLAG.EVENT_BUBBLE)
end
local function view_show(name)
  local top = name == "as" and as_view or name == "log" and log_view or main_view
  for _, view in ipairs(views) do
    view:set { align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = view == top and 0 or OFF_Y } }
  end
  log_visible = name == "log"
  settings_visible = name == "as"
  if log_visible then refresh_log() end
end

local as_label
as_set = function(text, color, logged)
  if not logged then append_log(text) end
  if as_label then as_label:set { text = fit_status(text), text_color = color or V_TXT2 } end
end

-- 标题带版本号: 版本由 build.py 在打包时注入(与容器头同一个 p67_pack.VERSION),
-- 不要把版本写死在这里, 否则改版本要动两处。
-- 固定单行: 按字节粗估宽度(ASCII≈0.56em, 三字节汉字≈1em), 超宽就逐档缩字号;
-- 宁可小一号, 也不要折行或溢出到屏幕外。
local CHAOS_VERSION = "__CHAOS_VERSION__"
local TITLE = "Chaos" .. CHAOS_VERSION .. "安装器"
local function title_size_for(text, max_width, preferred)
  local size = preferred
  while size > 16 do
    local w = 0
    for i = 1, #text do
      local b = text:byte(i)
      if b < 0x80 then w = w + size * 0.56
      elseif b >= 0xE0 then w = w + size end
    end
    if w <= max_width then break end
    size = size - 2
  end
  return size
end
local TITLE_SIZE = title_size_for(TITLE, SCR_W - 48, 32)

lvgl.Label(main_view, {
  text = TITLE,
  text_color = V_TXT,
  text_font = lvgl.Font(F_BODY, TITLE_SIZE),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -212 },
})

local card = lvgl.Object(main_view, {
  w = SCR_W - 32, h = 68,
  bg_opa = lvgl.OPA(100), bg_color = V_MID,
  border_width = 1, border_color = V_LINE2,
  radius = V_R, pad_all = 0,
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -150 },
})
card:clear_flag(lvgl.FLAG.SCROLLABLE)
card:add_flag(lvgl.FLAG.EVENT_BUBBLE)
status_label = lvgl.Label(card, {
  text = "准备就绪",
  text_color = V_TXT2,
  text_font = lvgl.Font(F_BODY, 22),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 0 },
})

local BUTTON_STYLES = {
  primary = { bg = V_ACC_BG, line = V_ACC_BG, fg = V_TXT,  opa = 100, bw = 1 },
  danger  = { bg = V_ERR,    line = V_ERR,    fg = V_ERR,  opa = 15,  bw = 0 },
  ghost   = { bg = V_DEEP,   line = V_LINE2,  fg = V_TXT2, opa = 100, bw = 1 },
}

local function make_button(parent, text, y_ofs, kind, on_clicked, height)
  local style = BUTTON_STYLES[kind] or BUTTON_STYLES.ghost
  local b = lvgl.Object(parent, {
    w = SCR_W - 32, h = height or 64,
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
  local button_label = lvgl.Label(b, {
    text = text,
    text_color = style.fg,
    text_font = lvgl.Font(F_BODY, 24),
    align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 0 },
  })
  b:onClicked(function() local ok, msg = pcall(on_clicked); if not ok then set_status("错误: " .. tostring(msg), V_ERR) end end)
  return b, button_label
end

-- 注入设置页: 自启动(装/删)、settings 入口与系统更新页入口。
-- 两个入口的选择在运行/更新模块时生效，切换时先落盘，下次进页面保留选择。
local saved_settings = read_all(DATA_DIR .. "/settings.cfg", "rb")
settings_enabled = (saved_settings == nil) or saved_settings == string.char(1)
local saved_system_update = read_all(DATA_DIR .. "/system_update.cfg", "rb")
system_update_enabled = saved_system_update == string.char(1)
local saved_launcher = read_all(DATA_DIR .. "/launcher.cfg", "rb")
launcher_enabled = (saved_launcher == nil) or saved_launcher == string.char(1)
fetch_enabled = read_all(FETCH_CONFIG, "rb") == string.char(1)
local autostart_btn_label, settings_btn_label, system_update_btn_label, launcher_btn_label, fetch_btn_label
settings_refresh = function()
  if fetch_btn_label then fetch_btn_label:set { text = fetch_enabled and "fetch自启动：开启" or "fetch自启动：关闭" } end
  if autostart_btn_label then
    autostart_btn_label:set { text = autostart_registered() and "自启动：删除" or "自启动：安装" }
  end
  if settings_btn_label then
    settings_btn_label:set { text = settings_enabled and "注入 settings：开启" or "注入 settings：关闭" }
  end
  if system_update_btn_label then
    system_update_btn_label:set { text = system_update_enabled and "系统更新入口：开启" or "系统更新入口：关闭" }
  end
  if launcher_btn_label then
    launcher_btn_label:set { text = launcher_enabled and "应用列表显示：开启" or "应用列表显示：关闭" }
  end
end
local run_button
detect_language()

run_button = make_button(main_view, "运行", -76, "primary", function()
  set_status("正在准备，请稍候", V_ACC)
  if runner then set_status("正在运行"); return end
  if module_loaded() then
    set_status("正在更新模块")
    local started, err = start_runner(true)
    if not started then set_status("更新失败: " .. tostring(err), V_ERR) end
    return
  end
  if started_once then set_status("每次开机只能运行一次", V_WARN); return end
  started_once = true
  local started, err = start_runner()
  if not started then set_status("运行失败: " .. tostring(err), V_ERR) end
end, 56)

-- 重启按钮: 点一次进入 3 秒倒计时, 倒计时结束再 reboot; 运行中禁止重启。
local restart_btn, restart_lbl
local reboot_timer = nil
restart_btn, restart_lbl = make_button(main_view, "重启", 116, "ghost", function()
  if reboot_timer then return end
  if runner then set_status("运行中, 请等待", V_WARN); return end
  local left = 3
  restart_btn:clear_flag(lvgl.FLAG.CLICKABLE)
  restart_lbl:set { text = "重启 " .. left }
  set_status("3 秒后重启", V_WARN)
  reboot_timer = lvgl.Timer {
    period = 1000, repeat_count = -1, paused = true,
    cb = function(t)
      left = left - 1
      if left > 0 then
        restart_lbl:set { text = "重启 " .. left }
        set_status(left .. " 秒后重启", V_WARN)
        return
      end
      t:delete()
      reboot_timer = nil
      if not exec("reboot") then
        restart_btn:add_flag(lvgl.FLAG.CLICKABLE)
        restart_lbl:set { text = "重启" }
        set_status("重启失败, 请手动重启", V_ERR)
      end
    end,
  }
  if not reboot_timer then
    restart_btn:add_flag(lvgl.FLAG.CLICKABLE)
    restart_lbl:set { text = "重启" }
    set_status("定时器创建失败", V_ERR)
    return
  end
  reboot_timer:resume()
end, 56)

make_button(main_view, "注入设置", -12, "ghost", function()
  settings_refresh()
  as_set(autostart_state_text(), V_TXT2)
  view_show("as")
end, 56)

make_button(main_view, "清除重置", 180, "danger", function()
  if runner then set_status("运行中, 请等待", V_WARN); return end
  if not wipe_armed then wipe_armed = true; set_status("再按一次以确认清除", V_WARN); return end
  wipe_armed = false
  local ok, err = autostart_remove()
  if not ok then set_status(tostring(err), V_ERR); return end
  ok, err = remove_file(RC_FETCH_SH)
  if not ok then set_status(tostring(err), V_ERR); return end
  exec("rm -rf " .. DATA_DIR)
  set_status("已清除, 重启后重新运行", V_OK)
end, 56)

lvgl.Label(as_view, {
  text = "注入设置",
  text_color = V_TXT,
  text_font = lvgl.Font(F_BODY, 32),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -212 },
})
as_label = lvgl.Label(as_view, {
  text = "",
  text_color = V_TXT2,
  text_font = lvgl.Font(F_BODY, 22),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -164 },
})

-- 行1: 自启动(点一下在"安装/删除"之间切换, 状态写在按钮文字上)
local autostart_button, autostart_lbl = make_button(as_view, "自启动：安装", -96, "ghost", function()
  if runner then as_set("正在运行, 请等待", V_WARN); return end
  if autostart_registered() then
    local ok, err = autostart_remove()
    if not ok then as_set("失败: " .. tostring(err), V_ERR); return end
    as_set("已删除，请到管理器重建", V_WARN)
  else
    local ok, err = start_runner("autostart")
    if not ok then as_set(tostring(err), V_ERR); return end
    view_show("main")
  end
  settings_refresh()
end, 48)
autostart_btn_label = autostart_lbl

-- 行2: 是否注入 settings(运行/更新模块时生效)
local settings_button, settings_lbl = make_button(as_view, "注入 settings：开启", -40, "ghost", function()
  if runner or started_once then as_set("请在运行前选择", V_WARN); return end
  settings_enabled = not settings_enabled
  local ok, err = write_file(DATA_DIR .. "/settings.cfg", string.char(settings_enabled and 1 or 0))
  if not ok then as_set("保存失败: " .. tostring(err), V_ERR); return end
  settings_refresh()
  as_set(settings_enabled and "运行时注入 settings" or "运行时跳过 settings", V_TXT2)
end, 48)
settings_btn_label = settings_lbl

-- 行3: 是否在“设置 → 我的设备 → 系统更新”中追加 Chaos 入口。
local system_update_button, system_update_lbl = make_button(as_view, "系统更新入口：关闭", 16, "ghost", function()
  if runner or started_once then as_set("请在运行前选择", V_WARN); return end
  system_update_enabled = not system_update_enabled
  local ok, err = write_file(DATA_DIR .. "/system_update.cfg", string.char(system_update_enabled and 1 or 0))
  if not ok then as_set("保存失败: " .. tostring(err), V_ERR); return end
  settings_refresh()
  as_set(system_update_enabled and "运行时注入系统更新入口" or "运行时跳过系统更新入口", V_TXT2)
end, 48)
system_update_btn_label = system_update_lbl

-- 行4: 是否在系统应用列表中显示 Chaos；默认开启并持久化。
local launcher_button, launcher_lbl = make_button(as_view, "应用列表显示：开启", 72, "ghost", function()
  if runner or started_once then as_set("请在运行前选择", V_WARN); return end
  launcher_enabled = not launcher_enabled
  local ok, err = write_file(DATA_DIR .. "/launcher.cfg", string.char(launcher_enabled and 1 or 0))
  if not ok then as_set("保存失败: " .. tostring(err), V_ERR); return end
  settings_refresh()
  as_set(launcher_enabled and "运行时显示应用列表入口" or "运行时跳过应用列表入口", V_TXT2)
end, 48)
launcher_btn_label = launcher_lbl

local fetch_button, fetch_lbl = make_button(as_view, "fetch自启动：关闭", 128, "ghost", function()
  if runner then as_set("正在运行，请等待", V_WARN); return end
  fetch_enabled = not fetch_enabled
  local ok, err = start_runner("fetch")
  if not ok then fetch_enabled = not fetch_enabled; as_set(tostring(err), V_ERR); return end
  settings_refresh()
end, 48)
fetch_btn_label = fetch_lbl

settings_refresh()

make_button(as_view, "< 返回", 184, "ghost", function()
  as_set(autostart_state_text(), V_TXT2)
  view_show("main")
end, 56)

-- 独立日志页：安装继续运行时也可翻页查看，不创建闪存日志文件。
make_button(main_view, "安装日志", 52, "ghost", function()
  log_page = math.max(1, math.ceil(#log_lines / LOG_ROWS))
  view_show("log")
end, 56)
log_title = lvgl.Label(log_view, {
  text = "安装日志", text_color = V_TXT, text_font = lvgl.Font(F_BODY, 28),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -212 },
})
log_label = lvgl.Label(log_view, {
  text = "暂无日志", text_color = V_TXT2, text_font = lvgl.Font(F_BODY, 20),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -56 },
})
make_button(log_view, "上一页", 88, "ghost", function() log_page = log_page - 1; refresh_log() end, 44)
make_button(log_view, "下一页", 140, "ghost", function() log_page = log_page + 1; refresh_log() end, 44)
make_button(log_view, "< 返回", 192, "ghost", function() view_show("main") end, 44)
