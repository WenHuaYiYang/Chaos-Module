-- main.lua -- 墟 · 数字表盘 v1
-- 数据语义(指南第 11 章实机例): 时/分/月/日 = 直接值; 心率 = Q24.8 定点需 //256;
-- 电量 = 直接值; 步数 = 直接整数(存在 Digit1~5 逐位键佐证)。
-- 字体: lvgl.Font("MiSans-Regular", N) (探针 v178 实机可用)。
local lvgl    = require("lvgl")
local dataman = require("dataman")

local BG      = 0x07111F
local WHITE   = 0xFFFFFF
local GRAY    = 0x999999
local ACCENT  = 0x5381FE
local GREEN   = 0x92DE00
local RED     = 0xFF5555

local function uiCreate()
    local W = lvgl.HOR_RES()
    local H = lvgl.VER_RES()
    local root = lvgl.Object(nil, {
        outline_width = 0, border_width = 0, pad_all = 0,
        bg_opa = lvgl.OPA(100), bg_color = BG,
        w = W, h = H,
    })
    local t = {}

    -- 日期(小灰)
    t.date = lvgl.Label(root, {
        x = 0, y = H * 8 // 100, w = W,
        text = "--月--日 --", text_color = GRAY,
        text_font = lvgl.Font("MiSans-Regular", 24),
        text_align = lvgl.ALIGN.CENTER,
    })

    -- 大时间 HH:MM
    t.time = lvgl.Label(root, {
        x = 0, y = H * 24 // 100, w = W,
        text = "--:--", text_color = WHITE,
        text_font = lvgl.Font("MiSans-Regular", 92),
        text_align = lvgl.ALIGN.CENTER,
    })

    -- 秒(小绿, 时间右下)
    t.sec = lvgl.Label(root, {
        x = 0, y = H * 43 // 100, w = W,
        text = "--", text_color = GREEN,
        text_font = lvgl.Font("MiSans-Regular", 26),
        text_align = lvgl.ALIGN.CENTER,
    })

    -- 标志字"墟"(小米蓝)
    t.logo = lvgl.Label(root, {
        x = 0, y = H * 48 // 100, w = W,
        text = "墟", text_color = ACCENT,
        text_font = lvgl.Font("MiSans-Regular", 64),
        text_align = lvgl.ALIGN.CENTER,
    })

    -- 底部三栏: 步数 / 心率 / 电量
    local col = W // 3
    local function stat(y, txt, color)
        return lvgl.Label(root, {
            x = 0, y = y, w = col,
            text = txt, text_color = color,
            text_font = lvgl.Font("MiSans-Regular", 22),
            text_align = lvgl.ALIGN.CENTER,
        })
    end
    t.step  = stat(H * 80 // 100, "--", WHITE)
    t.hr    = stat(H * 80 // 100, "--", RED)
    t.bat   = stat(H * 80 // 100, "--", GREEN)
    -- x 定位三列
    t.step:set { x = 0 }
    t.hr:set   { x = col }
    t.bat:set  { x = col * 2 }
    t.stepLbl = lvgl.Label(root, { x = 0,       y = H * 88 // 100, w = col,
        text = "步数", text_color = GRAY, text_font = lvgl.Font("MiSans-Regular", 16),
        text_align = lvgl.ALIGN.CENTER })
    t.hrLbl = lvgl.Label(root, { x = col,       y = H * 88 // 100, w = col,
        text = "心率", text_color = GRAY, text_font = lvgl.Font("MiSans-Regular", 16),
        text_align = lvgl.ALIGN.CENTER })
    t.batLbl = lvgl.Label(root, { x = col * 2,  y = H * 88 // 100, w = col,
        text = "电量", text_color = GRAY, text_font = lvgl.Font("MiSans-Regular", 16),
        text_align = lvgl.ALIGN.CENTER })
    return t
end

local t = uiCreate()

local function two(v)
    return string.format("%02d", v)
end

-- 时间: 分/时 High 键直接值(指南第 11 章)
local function updTime()
    local hh = dataman.get("timeHourHigh")
    local mm = dataman.get("timeMinuteHigh")
    t.time:set_text_static(two(hh) .. ":" .. two(mm))
end
local function updSec()
    local ss = dataman.get("timeSecondHigh")
    t.sec:set_text_static(two(ss))
end

-- 日期
local function updDate()
    local mon  = dataman.get("dateMonthHigh")
    local day  = dataman.get("dateDayHigh")
    local week = dataman.get("dateWeekStringFullCN")
    t.date:set_text_static(string.format("%d月%d日 %s", mon, day, tostring(week)))
end

-- 心率: Q24.8 定点, //256; 无效(过大)显示 --
local function updHr(k, v)
    local hr = (v or 0) // 256
    if hr <= 0 or hr > 240 then hr = 0 end
    t.hr:set_text_static(hr > 0 and tostring(hr) or "--")
end

-- 电量: 直接值; 充电中显示 数字+ 号
local function updBat(k, v)
    local b = v or 0
    if b < 0 or b > 100 then b = 0 end
    -- systemStatusCharge 定点/直接语义未定案: 非零即认为在充电(待真机校准)
    local chg = (dataman.get("systemStatusCharge") or 0) ~= 0
    t.bat:set_text_static(string.format("%d%%", b) .. (chg and "+" or ""))
end

-- 步数: 直接整数(若真机偏差 256 倍, 此处即是校准点)
local function updStep(k, v)
    t.step:set_text_static(tostring(v or 0))
end

updTime()
updSec()
updDate()
dataman.subscribe("timeMinuteHigh", updTime)
dataman.subscribe("timeSecondHigh", updSec)
dataman.subscribe("dateDayHigh", updDate)
dataman.subscribe("healthHeartRate", updHr)
dataman.subscribe("systemStatusBattery", updBat)
dataman.subscribe("healthStepCount", updStep)

function pageOnPause()
end

function pageOnResume()
    updTime()
    updDate()
end
