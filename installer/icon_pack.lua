-- 图标投递包: 把静态封面、可选 DGF1 动画及索引解到 /data/chaos/icons/<包号>/,
-- 再把这一包登记进清单 index.txt。装完之后在"系统美化"页的图标包列表里点一下就能切过去,
-- 不需要重启手表, 也不需要重刷主包。
--
-- 为什么要单独做一层容器: 表盘容器的文件记录表只有 4 条位置(记录从 0x158 起, 0x1A8 是另一张
-- 表), 38 张图标塞不进单槽 —— 图标包自己再套一层流式容器:
--   [4 字节 'CIPK'][u32 count]
--   count 次: [u32 len][u8 name_len][name][data]
-- 旧静态 CIPK 格式不变；动画为 <stem>_anim.bin，索引为 animations.bin。
-- 每拍搬运或复核最多 64KB，不 read_all 单个动画或整包。
--
-- 清单格式(定长记录, 与内核模块 icon_apply.rs 必须逐字节一致):
--   /data/chaos/icons/index.txt 恒为 128 字节 = 8 行 x 16 字节
--   行 = [2 位十进制包号][空格][12 字节短名, 空格补齐][换行]
--   空槽 = 16 个空格
--   为什么定长: 这样"改写清单"永远是 128 字节整块重写, 不需要先删文件也不需要 O_TRUNC。
--
-- 包号怎么来的: 1..8 里最小的空号; 如果清单里已经有同名的一行, 就复用那个号(重投递 = 覆盖)。
-- 满了(8 个)就报错提示先删一个 —— 不静默顶掉别人已登记的包。
--
-- 界面写法与字体投递包完全同源:
--   1. 定位与居中一律用 align 表, Label 不设宽度; 不要用 "x/w + text_align"(实测那两条属性不生效)。
--   2. 定时器在**点按钮时才建**: 表盘脚本在构建期抛错会让整棵 UI 树不提交, 表现就是全黑。
--   3. 所有可能失败的动作都过 pcall, 把错误写进状态行(可见的失败优于黑屏)。
--   4. 界面文案不带换行(Label 宽度自适应时多行文本较短的行走会各自左对齐)。

local lvgl = require("lvgl")

-- ===== 视觉: macOS 毛玻璃(vibrancy), 与主包安装器/字体包共用同一套 token =====
local V_DEEP  = 0x1C1C1E
local V_MID   = 0x2C2C2E
local V_LINE2 = 0x2A2A2C
local V_TXT   = 0xF2F2F2
local V_TXT2  = 0xB3B3B3
local V_OK    = 0x30D158
local V_ERR   = 0xFF453A
local V_ACC_BG = 0x2563EB
-- 圆角 24: 高于界面风格里的 12 上限, 按按钮实际尺寸定的刻意取值, 不要改回 12
local V_R     = 24
-- 字体只用系统自带的 MiSans-Regular: 文楷的 face 只在美化页手动点过之后才登记,
-- 重启后没点过就登记不上 => 中文变空白豆腐块(实测踩过)。
local F_BODY  = "MiSans-Regular"

local DATA_DIR   = "/data/chaos"
local ICON_DIR   = DATA_DIR .. "/icons"
local INDEX_PATH = ICON_DIR .. "/index.txt"
local INDEX_SIZE = 128
local LINE_SIZE  = 16
local PACK_MAX   = 8

-- 构建期替换: tools/build_icon_pack.py 把下面这个占位符换成素材短名(恰好出现一次,
-- 打包脚本会断言这一点)。直接写在脚本里而不是从容器头读: 表盘容器里读不到自己的显示名
-- (0x68 那个字段固件才认), 而短名要写进清单给内核模块显示, 必须两侧拿到同一个值。
local PACK_NAME = "__PACK_NAME__"

-- 表盘上那行标题(整句, 构建期替换)。默认是"图标投递 <短名>"。
local PACK_TITLE = "__PACK_TITLE__"

-- SCRIPT_PATH 是固件注入的"本脚本所在目录"。写成 (SCRIPT_PATH or "") 是防御:
-- 假如它是 nil, 直接拼接会在顶层抛错, 顶层抛错 = 整棵 UI 树不提交 = 黑屏。
local SCRIPT_BASE = SCRIPT_PATH or ""
local PACK_RESOURCE = SCRIPT_BASE .. "pack.bin"

local CIPK_MAGIC = "CIPK"
local CHUNK = 0x10000        -- 每拍最多搬 64KB(与字体包同一档)

local job = {
  stage = 0, pack = 0, src = nil, dst = nil, verify = nil, lines = nil, seen = nil,
  count = 0, idx = 0, moved = 0, left = 0, wrote = 0,
}
local status_label
local timer

-- ===== 小工具 =====

local function set_status(text, color)
  if status_label then
    pcall(function()
      status_label:set { text = tostring(text), text_color = color or V_TXT2 }
    end)
  end
end

-- 小端读一个 u32; 乘项从高位起累(与打包侧的 u32le 互为镜像)
local function read_u32(s, i)
  local a, b, c, d = s:byte(i, i + 3)
  if not d then return nil end
  return d * 0x1000000 + c * 0x10000 + b * 0x100 + a
end

local function spaces(n) return string.rep(" ", n) end

-- 清单 -> 行表(只收合法行: 2 位数字号 1..8 + 非空短名)。空槽与坏行一律跳过。
local function parse_index(raw)
  local lines = {}
  for i = 0, PACK_MAX - 1 do
    local off = i * LINE_SIZE + 1
    local ln = raw:sub(off, off + LINE_SIZE - 1)
    if #ln == LINE_SIZE then
      local digits = ln:sub(1, 2)
      local id = tonumber(digits)
      if id and id >= 1 and id <= PACK_MAX then
        local nm = ln:sub(4, 15):gsub("%s+$", "")
        if #nm > 0 then lines[#lines + 1] = { id = id, name = nm } end
      end
    end
  end
  return lines
end

local function read_index()
  local f = io.open(INDEX_PATH, "rb")
  if not f then return "" end
  local raw = f:read(INDEX_SIZE)
  pcall(f.close, f)
  if type(raw) ~= "string" then return "" end
  return raw
end

-- 128 字节定长清单(空槽 = 16 个空格)。行序 = entries 的顺序。
local function render_index(entries)
  local out = {}
  for i = 1, PACK_MAX do
    local e = entries[i]
    if e then
      local nm = e.name
      if #nm > 12 then nm = nm:sub(1, 12) end
      out[i] = string.format("%02d %s%s\n", e.id, nm, spaces(12 - #nm))
    else
      out[i] = spaces(LINE_SIZE)
    end
  end
  return table.concat(out)
end

local function write_index(entries)
  local raw = render_index(entries)
  local temporary = INDEX_PATH .. ".tmp"
  local f, err = io.open(temporary, "wb")
  if not f then return false, tostring(err) end
  local ok, result = pcall(f.write, f, raw)
  local cok, closed = pcall(f.close, f)
  if not ok or not result or not cok or not closed then
    pcall(os.remove, temporary)
    return false, "清单未写全"
  end
  local rf = io.open(temporary, "rb")
  local checked = rf and rf:read(INDEX_SIZE + 1)
  if rf then pcall(rf.close, rf) end
  if checked ~= raw then pcall(os.remove, temporary) return false, "清单回读失败" end
  local renamed, result = pcall(os.rename, temporary, INDEX_PATH)
  if not renamed or not result then pcall(os.remove, temporary) return false, "清单提交失败" end
  return true
end

-- 定包号: 同名复用, 否则取最小空号; 满了返回 nil
local function pick_pack(lines)
  local used = {}
  for _, e in ipairs(lines) do
    used[e.id] = true
    if e.name == PACK_NAME then return e.id, lines end
  end
  for id = 1, PACK_MAX do
    if not used[id] then
      lines[#lines + 1] = { id = id, name = PACK_NAME }
      return id, lines
    end
  end
  return nil, lines
end

local function close_files()
  if job.src then pcall(function() job.src:close() end) job.src = nil end
  if job.dst then pcall(function() job.dst:close() end) job.dst = nil end
  if job.verify then pcall(function() job.verify:close() end) job.verify = nil end
end

local function finish(ok, text)
  job.stage = 0
  close_files()
  if timer then pcall(function() timer:pause() end) end
  set_status(text, ok and V_OK or V_ERR)
end

-- ===== 解包(与界面无关) =====

local function step()
  -- 1) 准备: 定包号 -> 建目录 -> 打开容器读头
  if job.stage == 1 then
    local lines = parse_index(read_index())
    local id = pick_pack(lines)
    if not id then
      finish(false, "图标包已满(8), 请先删一个")
      return
    end
    job.pack = id
    job.lines = lines
    -- 先把这个包号的目录整个清掉再建: 上一包可能投递到一半失败(比如容器坏在中间),
    -- 留下几十张旧图; 而包号是按"最小空号"分配的, 下一包如果张数更少(素材池缺项),
    -- 那些旧图就会被当成这一包的图标 = 两套图标混在一起。
    -- 路径前缀 + 两位包号都是自己拼的, 没有外部输入, 不存在注入面。
    local dir = ICON_DIR .. "/" .. string.format("%02d", id)
    pcall(function() os.execute("rm -rf " .. dir) end)
    pcall(function() os.execute("mkdir -p " .. dir) end)
    local src = io.open(PACK_RESOURCE, "rb")
    if not src then
      finish(false, "容器里没有 pack.bin")
      return
    end
    local hdr = src:read(8)
    if type(hdr) ~= "string" or #hdr < 8 or hdr:sub(1, 4) ~= CIPK_MAGIC then
      src:close()
      finish(false, "pack.bin 头不符")
      return
    end
    local count = read_u32(hdr, 5)
    if not count or count < 1 or count > 256 then
      src:close()
      finish(false, "图标张数非法: " .. tostring(count))
      return
    end
    job.src = src
    job.seen = {}
    job.count, job.idx, job.wrote, job.moved, job.left = count, 0, 0, 0, 0
    set_status(PACK_NAME .. " 解包 0/" .. count)
    job.stage = 2
    return
  end

  -- 2) 每拍解一张(一张内部按 64KB 分块)
  if job.stage == 2 then
    if job.verify then
      local want = math.min(CHUNK, job.verify_left)
      local data = job.verify:read(want)
      if type(data) ~= "string" or #data == 0 then
        pcall(os.remove, job.target)
        finish(false, "第 " .. (job.idx + 1) .. " 个文件落盘不完整")
        return
      end
      job.verify_left = job.verify_left - #data
      if job.verify_left == 0 then
        local extra = job.verify:read(1)
        local ok, closed = pcall(job.verify.close, job.verify)
        job.verify = nil
        if (extra and #extra > 0) or not ok or not closed then
          finish(false, "文件大小复核失败")
          return
        end
        job.idx, job.wrote = job.idx + 1, job.wrote + 1
        set_status(PACK_NAME .. " 解包 " .. job.idx .. "/" .. job.count)
      end
      return
    end
    if not job.dst then
      if job.idx >= job.count then
        job.stage = 3
        return
      end
      local lh = job.src:read(4)
      local len = lh and read_u32(lh, 1)
      if not len or len < 1 or len > 0x100000 then
        finish(false, "第 " .. (job.idx + 1) .. " 张长度头非法")
        return
      end
      local nh = job.src:read(1)
      local nl = nh and nh:byte(1)
      -- 名字只允许 [A-Za-z0-9_]+.bin: 绝不让容器决定落点(路径穿越在这里断掉)
      local nm = (nl and nl > 0 and nl <= 64) and job.src:read(nl) or nil
      if type(nm) ~= "string" or #nm ~= nl or not nm:match("^[%w_]+%.bin$") then
        finish(false, "第 " .. (job.idx + 1) .. " 张名字非法")
        return
      end
      if job.seen[nm] or (not nm:match("_anim%.bin$") and len > 0x80000)
        or (nm == "animations.bin" and len ~= 20) then
        finish(false, "重复文件或长度非法: " .. nm)
        return
      end
      job.seen[nm] = true
      local dst, derr = io.open(ICON_DIR .. "/" .. string.format("%02d", job.pack) .. "/" .. nm, "wb")
      if not dst then
        finish(false, "打不开目标文件: " .. tostring(derr))
        return
      end
      job.nm = nm
      job.target = ICON_DIR .. "/" .. string.format("%02d", job.pack) .. "/" .. nm
      job.dst, job.left, job.moved, job.flen = dst, len, 0, len
      return
    end
    local want = job.left
    if want > CHUNK then want = CHUNK end
    local blk = want > 0 and job.src:read(want) or ""
    if type(blk) ~= "string" or #blk == 0 then
      finish(false, "第 " .. (job.idx + 1) .. " 张数据缺失")
      return
    end
    -- 满盘时 write 返回 nil(不抛异常) —— 不查返回值就会出现"已投递 N 张"的
    -- 假象, 实际磁盘上是 0 字节/半截文件: LVGL open 成功、解码失败 = 图标全灭
    -- (2026-10-04 真机事故, Pure/Delta 两个包同灭的根因)。
    local wok, wobj = pcall(job.dst.write, job.dst, blk)
    if not wok or not wobj then
      close_files()
      pcall(function() os.remove(ICON_DIR .. "/" .. string.format("%02d", job.pack) .. "/" .. job.nm) end)
      finish(false, "第 " .. (job.idx + 1) .. " 张写入失败, 空间可能不足, 请先删几个图标包")
      return
    end
    job.left = job.left - #blk
    job.moved = job.moved + #blk
    if job.left == 0 then
      local cok, closed = pcall(job.dst.close, job.dst)
      job.dst = nil
      if not cok or not closed or job.moved ~= job.flen then
        finish(false, "第 " .. (job.idx + 1) .. " 张没写全")
        return
      end
      job.verify = io.open(job.target, "rb")
      if not job.verify then finish(false, "打不开回读文件") return end
      job.verify_left = job.flen
    end
    return
  end

  -- 3) 登记清单: 128 字节定长整块写(不删文件, 不需要 O_TRUNC)
  if job.stage == 3 then
    local extra = job.src:read(1)
    if extra and #extra > 0 then finish(false, "容器尾部有多余数据") return end
    local ok, err = write_index(job.lines)
    if not ok then
      finish(false, "清单写失败: " .. tostring(err))
      return
    end
    pcall(function() job.src:close() end)
    job.src = nil
    finish(true, "已投递 " .. job.wrote .. " 张, 包号 " .. string.format("%02d", job.pack))
    return
  end
end

-- 点一下按钮: 这时才建定时器(与安装器/字体包同序), 建不出来就写在屏幕上
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
  job.stage, job.idx, job.wrote, job.moved = 1, 0, 0, 0
  pcall(function() timer:resume() end)
  set_status("开始...")
end

-- ===== 界面(与主包安装器/字体包同一套设计语言) =====

local W = lvgl.HOR_RES()
local H = lvgl.VER_RES()

local root = lvgl.Object(nil, {
  w = W, h = H,
  outline_width = 0, border_width = 0, pad_all = 0,
  bg_opa = lvgl.OPA(100), bg_color = V_DEEP,
})
-- 容器必须凑齐三件事, 少一件长按表盘就进不了表盘选择页:
-- clear_flag(SCROLLABLE) + add_flag(EVENT_BUBBLE) + 根对象 add_flag(CLICKABLE)
root:clear_flag(lvgl.FLAG.SCROLLABLE)
root:add_flag(lvgl.FLAG.CLICKABLE)
root:add_flag(lvgl.FLAG.EVENT_BUBBLE)

lvgl.Label(root, {
  text = PACK_TITLE,
  text_color = V_TXT,
  text_font = lvgl.Font(F_BODY, 40),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = -170 },
})

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
  text = PACK_NAME .. " 已加载",
  text_color = V_TXT2,
  text_font = lvgl.Font(F_BODY, 22),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 0 },
})

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
  text = "投递图标",
  text_color = V_TXT,
  text_font = lvgl.Font(F_BODY, 24),
  align = { type = lvgl.ALIGN.CENTER, x_ofs = 0, y_ofs = 0 },
})
btn:onClicked(function()
  local ok, err = pcall(start)
  if not ok then set_status("启动失败: " .. tostring(err), V_ERR) end
end)
