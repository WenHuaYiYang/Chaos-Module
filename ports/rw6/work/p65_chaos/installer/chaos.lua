-- Chaos 安装器（Redmi Watch 6）
-- 走固件自己的表盘 Lua 通道：本文件被装进表盘目录的 _lua/mood/mood.lua，
-- 选上这张表盘时由固件装载执行。SCRIPT_PATH 指向它自己所在目录。
--
-- 流程约束（与 10P 那条一致）：
--   1. 不做 rmmod。模块已在内存里时 rmmod 会留下悬空的 app 注册链，切表盘必崩。
--   2. 每个动作一个独立 timer 拍，让事件循环在两个注册阶段之间转一圈。
--   3. 不在 /data/chaos 里留调试文件。

local lvgl = require("lvgl")

-- ===== 视觉：与 10P 安装器同一套 macOS 毛玻璃单屏翻译 =====
-- 层级即色彩（页底/卡片/按钮三档灰阶），分隔一律 1px 边框，无渐变无阴影无动画；
-- 强调色与状态色只用于文字，唯一破例是主按钮的底色用标准按钮蓝。
local V_DEEP = 0x1C1C1E
local V_MID = 0x2C2C2E
local V_LINE2 = 0x2A2A2C
local V_TXT = 0xF2F2F2
local V_TXT2 = 0xB3B3B3
local V_ACC_BG = 0x2563EB
local V_OK = 0x30D158
local V_WARN = 0xFFD60A
local V_ERR = 0xFF453A
local V_R = 24

local RES_DIR = SCRIPT_PATH
local KO_RES = RES_DIR .. "chaos_sup.bin"
local ICON_RES = RES_DIR .. "chaos_icon.bin"
local DATA_DIR = "/data/chaos"
local KO_PATH = DATA_DIR .. "/chaos_sup.bin"
local ICON_PATH = DATA_DIR .. "/chaos_icon.bin"
local LSMOD_SNAP = DATA_DIR .. "/lsmod.snap"
local MODULE_NAME = "chaos_p65"

-- ===== 落盘原语 =====
local function write_file(path, text)
  local f = io.open(path, "wb")
  if not f then return false end
  f:write(text)
  f:close()
  return true
end

local function read_file(path)
  local f = io.open(path, "rb")
  if not f then return nil end
  local s = f:read("*a")
  f:close()
  return s
end

local function copy_file(src, dst)
  local a = io.open(src, "rb")
  if not a then return false, "源文件缺失" end
  local b = io.open(dst, "wb")
  if not b then a:close() return false, "目标不可写" end
  while true do
    local blk = a:read(8192)
    if not blk then break end
    b:write(blk)
  end
  a:close()
  b:close()
  return true
end

local function run(cmd)
  local ok, _, code = os.execute(cmd)
  if ok == nil then ok = false end
  return ok and (code == nil or code == 0), code
end

local function module_loaded()
  run("lsmod > " .. LSMOD_SNAP)
  local s = read_file(LSMOD_SNAP)
  if not s then return false end
  os.remove(LSMOD_SNAP)
  return string.find(s, MODULE_NAME, 1, true) ~= nil
end

-- ===== 安装步骤：每步一个拍 =====
local pipeline = {
  {
    name = "1 部署模块",
    body = function()
      run("mkdir " .. DATA_DIR)
      return copy_file(KO_RES, KO_PATH)
    end,
  },
  {
    name = "2 部署桌面图标",
    body = function()
      run("mkdir " .. DATA_DIR)
      -- 图标缺了不影响能不能用，只影响桌面上那个圆
      copy_file(ICON_RES, ICON_PATH)
      return true
    end,
  },
  {
    name = "3 加载模块",
    body = function()
      if module_loaded() then
        return false, "旧模块还在，请重启后再装"
      end
      local ok = run("insmod " .. KO_PATH .. " " .. MODULE_NAME)
      if not ok then return false, "insmod 失败" end
      return true
    end,
  },
  {
    name = "4 核对",
    body = function()
      if not module_loaded() then return false, "模块没起来" end
      return true
    end,
  },
}

local status_label
local runner
local step_index = 1
local started_once = false

-- 状态行是单行居中的窄卡片：超宽会被屏幕裁掉，按"半角宽度"截断。
-- 中文/全角算 2 个单位，ASCII 算 1。
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

local function set_status(text, color)
  if status_label then
    status_label:set { text = fit_status(text), text_color = color or V_TXT2 }
  end
end

-- 字体：表盘 Lua 这边只保证有内置字体，中文字族名各家不同，建不出来就用默认脸。
local function face(size)
  for _, name in ipairs({ "MiSans-Regular", "montserrat" }) do
    local ok, f = pcall(function() return lvgl.Font(name, size) end)
    if ok and f then return f end
  end
  return nil
end

-- 把建好的脸并进样式表；没建成就不带 text_font 这个键。
local function with_font(spec, size)
  local f = face(size)
  if f then spec.text_font = f end
  return spec
end

local function label(parent, text, y_ofs, color, size)
  return lvgl.Label(parent, with_font({
    text = text,
    text_color = color,
    align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = y_ofs },
  }, size))
end

local function stop_runner(ok, message)
  if runner then
    runner:delete()
    runner = nil
  end
  set_status(message, ok and V_OK or V_ERR)
end

-- 每步一个拍：这一步成了就把游标往前挪一格，下一拍再跑下一步，
-- 让 miwear 的事件循环能在两个注册阶段之间转一圈。
local function run_next()
  local item = pipeline[step_index]
  if not item then
    stop_runner(true, "运行完成，重启生效")
    return
  end
  set_status("RUN " .. item.name, V_TXT2)
  local called, ret, err = pcall(item.body)
  if not called then
    stop_runner(false, "异常: " .. tostring(ret))
  elseif ret then
    step_index = step_index + 1
  else
    stop_runner(false, err or (item.name .. " 失败"))
  end
end

local function start()
  if runner then return end
  if started_once then
    set_status("装过一次了，重启后才能再装", V_WARN)
    return
  end
  started_once = true
  step_index = 1
  runner = lvgl.Timer {
    period = 1000,
    repeat_count = -1,
    paused = true,
    cb = function() run_next() end,
  }
  if not runner then
    set_status("建定时器失败", V_ERR)
    return
  end
  runner:resume()
  runner:ready()
end

local function wipe()
  if runner then return end
  run("rmmod " .. MODULE_NAME)
  run("rm -r " .. DATA_DIR)
  set_status("已清除，重启后生效", V_WARN)
end

-- ===== 界面 =====
local SCR_W = lvgl.HOR_RES()
local SCR_H = lvgl.VER_RES()

local root = lvgl.Object(nil, {
  w = SCR_W,
  h = SCR_H,
  outline_width = 0,
  border_width = 0,
  pad_all = 0,
  bg_opa = lvgl.OPA(100),
  bg_color = V_DEEP,
})
-- 与固件自带表盘同一套：清 SCROLLABLE + 挂 EVENT_BUBBLE，root 自己 CLICKABLE。
-- 少了 EVENT_BUBBLE，长按表盘进不了表盘选择页。
root:clear_flag(lvgl.FLAG.SCROLLABLE)
root:add_flag(lvgl.FLAG.EVENT_BUBBLE)
root:add_flag(lvgl.FLAG.CLICKABLE)

label(root, "Chaos 安装器", -170, V_TXT, 32)

local card = lvgl.Object(root, {
  w = SCR_W - 32,
  h = 68,
  bg_opa = lvgl.OPA(100),
  bg_color = V_MID,
  border_width = 1,
  border_color = V_LINE2,
  radius = V_R,
  pad_all = 0,
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -100 },
})
card:clear_flag(lvgl.FLAG.SCROLLABLE)
card:add_flag(lvgl.FLAG.EVENT_BUBBLE)
status_label = label(card, "准备就绪", 0, V_TXT2, 22)

local function make_button(text, y_ofs, bg, fg, opa, bw, on_clicked)
  local b = lvgl.Object(root, {
    w = SCR_W - 32,
    h = 64,
    bg_opa = lvgl.OPA(opa),
    bg_color = bg,
    border_width = bw,
    border_color = bg,
    radius = V_R,
    pad_all = 0,
    align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = y_ofs },
  })
  b:clear_flag(lvgl.FLAG.SCROLLABLE)
  b:add_flag(lvgl.FLAG.CLICKABLE)
  b:add_flag(lvgl.FLAG.EVENT_BUBBLE)
  label(b, text, 0, fg, 24)
  b:onClicked(function()
    local called, err = pcall(on_clicked)
    if not called then set_status("错误: " .. tostring(err), V_ERR) end
  end)
  return b
end

make_button("运行", 0, V_ACC_BG, V_TXT, 100, 1, start)
make_button("清除 Chaos", 90, V_ERR, V_ERR, 15, 0, wipe)

local chaos = {}

function pageOnPause()
end

function pageOnResume()
end

function chaos.init(theme)
  return root
end

chaos.init("theme1")
