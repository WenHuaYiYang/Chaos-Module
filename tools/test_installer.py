#!/usr/bin/env python3
"""Run the real installer Lua against a filesystem, UI and timer fixture."""
from pathlib import Path
import fnmatch
import shlex
import struct
import unittest
from lupa import LuaRuntime

try:  # 预览块渲染器需要 Pillow; 只做几何比对时没有它也应当能跑
    import gen_preview
except ImportError:  # pragma: no cover - 该解释器没装 Pillow
    gen_preview = None

ROOT = Path(__file__).resolve().parents[1]
FETCH_SCRIPT = b'/data/rc.d/chaos-fetch.sh'
FETCH_CONFIG = b'/data/chaos/fetch.cfg'
SUP = b'/data/chaos/sup.ko'


class Installer:
    def __init__(self, files=None, *, write_fail=None, corrupt=None):
        elf = b'\x7fELF' + bytes(48)
        self.files = {
            b'/resource/chaos_sup.bin': elf + b'A' * 1048576,
            b'/resource/fetch-module.bin': elf + b'F' * 70000,
            b'/resource/capatch.bin': elf + b'C' * 8000,
            b'/resource/ca.pem': b'certificate' * 22000,
            b'/resource/chaos_icon.bin': b'I' * 36876,
            b'/resource/chaos_icon_settings.bin': b'S' * 36876,
            b'/resource/lxgw.ttf': bytes(68),
        }
        self.files.update(files or {})
        self.handles = {}
        self.next_fd = 1
        self.commands = []
        self.reads = []
        self.writes = []
        self.write_fail = write_fail
        self.corrupt = corrupt
        self.device_writes = []
        self.lua = LuaRuntime(encoding=None, unpack_returned_tuples=True)
        globals_ = self.lua.globals()
        for name in ('open', 'read', 'write', 'close', 'execute'):
            globals_[('py_' + name).encode()] = getattr(self, name)
        self.lua.execute(b'''
        observed={nodes={},timers={},messages={}}
        io={open=function(path,mode)
          local id=py_open(path,mode)
          if not id then return nil,'open failed' end
          local f={}
          function f:read(n)return py_read(id,n)end
          function f:write(s)if py_write(id,s)then return self end return nil,'write failed' end
          function f:close()return py_close(id)end
          return f
        end}
        os={execute=py_execute}
        print=function(...)end
        local function node(parent,props)
          local n={parent=parent,props=props,id=#observed.nodes+1}
          table.insert(observed.nodes,n)
          function n:set(p)
            for k,v in pairs(p)do self.props[k]=v end
            if p.text then table.insert(observed.messages,{text=p.text,color=p.text_color})end
          end
          function n:clear_flag(_)end
          function n:add_flag(f)self.flags=self.flags or {};self.flags[f]=true end
          function n:onClicked(cb)self.click=cb end
          return n
        end
        local lvgl={Object=node,Label=node,OPA=function(v)return v end,Font=function(...)return 0 end,
          HOR_RES=function()return 336 end,VER_RES=function()return 480 end,
          FLAG={SCROLLABLE=1,CLICKABLE=2,EVENT_BUBBLE=4},ALIGN={CENTER=0}}
        lvgl.Timer=function(options)
          function options:resume()self.paused=false end
          function options:ready()self.ready_count=(self.ready_count or 0)+1 end
          function options:delete()self.deleted=true end
          table.insert(observed.timers,options)
          return options
        end
        require=function(name)assert(name=='lvgl');return lvgl end
        SCRIPT_PATH='/resource/'
        ''')
        self.lua.execute((ROOT / 'installer/chaos_installer.lua').read_bytes())

    def open(self, path, mode):
        if mode in (b'r', b'rb') and path not in self.files:
            return None
        if mode in (b'w', b'wb') and path != b'/dev/chaos':
            self.files[path] = b''
        fd = self.next_fd
        self.next_fd += 1
        self.handles[fd] = [path, mode, 0]
        return fd

    def read(self, fd, count):
        path, _, offset = self.handles[fd]
        self.reads.append((path, count))
        data = self.files.get(path, b'')
        size = len(data) if count == b'*a' else count
        result = data[offset:offset + size]
        self.handles[fd][2] += len(result)
        return result or None

    def write(self, fd, data):
        path, _, _ = self.handles[fd]
        self.writes.append((path, len(data)))
        if path == self.write_fail:
            return False
        if path == b'/dev/chaos':
            self.device_writes.append(data)
        else:
            self.files[path] += data
        return True

    def close(self, fd):
        path, mode, _ = self.handles.pop(fd)
        if path == self.corrupt and mode == b'wb':
            self.files[path] += b'corruption'
        return True

    def execute(self, command):
        self.commands.append(command)
        parts = shlex.split(command.decode())
        if parts[0] == 'getprop':
            self.files[parts[-1].encode()] = b'zh-CN'
        elif parts[:2] in (['rm', '-f'], ['rm', '-rf']):
            pattern = parts[2].encode()
            for name in list(self.files):
                if fnmatch.fnmatch(name, pattern) or (parts[1] == '-rf' and name.startswith(pattern + b'/')):
                    del self.files[name]
        elif parts[0] == 'insmod':
            words = [0] * 48
            words[0], words[3], words[10], words[44] = 0x53484332, 0x43485302, 1, 1
            self.files[b'/dev/chaos'] = struct.pack('<48I', *words)
        elif parts[:2] != ['mkdir', '-p']:
            raise AssertionError(command)
        return True

    @property
    def observed(self):
        return self.lua.globals().observed

    def click(self, text):
        for node in self.observed.nodes.values():
            if node.props[b'text'] == text.encode() and node.parent and node.parent.click:
                node.parent.click()
                return
        raise AssertionError(f'Button not found: {text}')

    def step(self):
        timers = list(self.observed.timers.values())
        for timer in timers:
            if not timer.paused and not timer.deleted:
                timer.cb(timer)

    def finish(self):
        for _ in range(1500):
            if not any(not t.paused and not t.deleted for t in self.observed.timers.values()):
                return
            self.step()
        raise AssertionError('Installer never finished')

    def messages(self):
        return [(m.text.decode(), m.color) for m in self.observed.messages.values()]

    def injection_status(self):
        title = next(n for n in self.observed.nodes.values()
                     if n.props[b'text'] == '注入设置'.encode() and not n.parent.click)
        view = title.parent
        status = next(n for n in self.observed.nodes.values()
                      if n.parent and n.parent.id == view.id and n.id != title.id and n.props[b'text'] is not None)
        return view, status


class InstallerTests(unittest.TestCase):
    def test_click_paints_before_file_io_and_copy_yields_with_bounded_reads(self):
        app = Installer()
        app.reads.clear(); app.writes.clear()
        app.click('运行')
        self.assertIn('正在准备', app.messages()[-1][0])
        self.assertFalse(app.writes)
        self.assertFalse(any(p == b'/resource/chaos_sup.bin' for p, _ in app.reads))
        timer = app.observed.timers[1]
        self.assertEqual(timer.period, 20)
        self.assertIsNone(timer.ready_count)
        app.step()  # Step label gets its own paint opportunity.
        self.assertFalse(app.writes)
        app.step()
        self.assertEqual(len(app.files[SUP]), 65536)
        app.click('安装日志')  # Viewing logs must work while the copy is suspended.
        app.finish()
        self.assertEqual(app.files[SUP], app.files[b'/resource/chaos_sup.bin'])
        self.assertTrue(all(isinstance(n, int) and n <= 65536 for p, n in app.reads
                            if p in (SUP, b'/resource/chaos_sup.bin')))
        self.assertEqual(app.messages()[-1][0], '运行完成')
        self.assertFalse(app.handles)
        self.assertNotIn(FETCH_SCRIPT, app.files)
        self.assertEqual(app.files[FETCH_CONFIG], b'\x00')

    def test_fetch_opt_in_only_registers_next_boot_and_persists(self):
        app = Installer()
        app.click('注入设置'); app.click('fetch自启动：关闭')
        app.finish()
        self.assertEqual(app.files[FETCH_CONFIG], b'\x01')
        self.assertIn(b'insmod /data/chaos/fetch-module.ko', app.files[FETCH_SCRIPT])
        self.assertFalse(any(c.startswith(b'insmod ') for c in app.commands))
        self.assertFalse(any(b'chaos.sh' == p.rsplit(b'/', 1)[-1] for p in app.files))
        view, status = app.injection_status()
        self.assertEqual(view.props[b'align'][b'y_ofs'], 0)
        self.assertIn('重启生效', status.props[b'text'].decode())
        self.assertNotEqual(status.props[b'text_color'], 0xFF453A)
        app = Installer(app.files)
        app.click('注入设置'); app.click('fetch自启动：开启'); app.finish()
        self.assertEqual(app.files[FETCH_CONFIG], b'\x00')
        self.assertNotIn(FETCH_SCRIPT, app.files)
        self.assertFalse(any(c.startswith(b'insmod ') for c in app.commands))

    def test_fetch_switch_stays_in_settings_while_preparing_and_after_failure(self):
        app = Installer(write_fail=b'/data/chaos/fetch-module.ko')
        app.click('注入设置'); app.click('fetch自启动：关闭')
        view, status = app.injection_status()
        self.assertEqual(view.props[b'align'][b'y_ofs'], 0)
        self.assertIn('正在准备', status.props[b'text'].decode())
        app.finish()
        self.assertEqual(view.props[b'align'][b'y_ofs'], 0)
        self.assertEqual(status.props[b'text_color'], 0xFF453A)
        self.assertIn('失败', status.props[b'text'].decode())
        # A failed save restores the persisted switch value in place.
        app.click('fetch自启动：关闭')
        app.finish()

    def test_fetch_waiting_for_reboot_does_not_fail_chaos_install(self):
        app = Installer({FETCH_CONFIG: b'\x01'})
        app.click('运行'); app.finish()
        self.assertIn('fetch重启生效', app.messages()[-1][0])
        self.assertNotEqual(app.messages()[-1][1], 0xFF453A)
        self.assertFalse(any('fetch' in text and color == 0xFF453A for text, color in app.messages()))
        self.assertFalse(any(b'insmod /data/chaos/fetch-module' in cmd for cmd in app.commands))

    def test_copy_write_failure_or_corruption_stops_before_load_and_is_logged(self):
        for options in ({'write_fail': SUP}, {'corrupt': SUP}):
            with self.subTest(options=options):
                app = Installer(**options)
                app.click('运行'); app.finish()
                self.assertEqual(app.messages()[-1][1], 0xFF453A)
                self.assertFalse(any(c.startswith(b'insmod ') for c in app.commands))
                self.assertFalse(app.handles)
                app.click('安装日志')
                texts = [n.props[b'text'] for n in app.observed.nodes.values() if n.props[b'text']]
                self.assertTrue(any(b'/data/chaos/sup.ko' in t or '失败'.encode() in t for t in texts))

    def test_chaos_autostart_streams_resources_and_does_not_enable_fetch(self):
        app = Installer()
        app.reads.clear()
        app.click('注入设置'); app.click('自启动：安装'); app.finish()
        self.assertIn(b'/data/rc.d/chaos.sh', app.files)
        self.assertNotIn(FETCH_SCRIPT, app.files)
        self.assertFalse(any(p == b'/resource/chaos_sup.bin' and n == b'*a' for p, n in app.reads))
        self.assertFalse(app.handles)

    def test_reinstall_skips_identical_module_and_logs_can_be_paged(self):
        app = Installer()
        app.click('运行'); app.finish()
        app.writes.clear()
        app.click('运行'); app.finish()
        self.assertFalse(any(path == SUP for path, _ in app.writes))
        self.assertIn('请重启', app.messages()[-1][0])
        app.click('安装日志'); app.click('上一页'); app.click('下一页')
        self.assertFalse(app.handles)

    def test_old_implicit_fetch_script_is_removed_when_there_is_no_opt_in(self):
        app = Installer({FETCH_SCRIPT: b'old implicit fetch startup'})
        app.click('运行'); app.finish()
        self.assertNotIn(FETCH_SCRIPT, app.files)
        self.assertEqual(app.files[FETCH_CONFIG], b'\x00')

    def test_disabled_fetch_does_not_unload_a_running_module(self):
        app = Installer({FETCH_CONFIG: b'\x01', FETCH_SCRIPT: b'old fetch script',
                         b'/dev/tunentry': b'mt_build=v108-20261024-dbg-flag mt_armed=1'})
        app.click('注入设置'); app.click('fetch自启动：开启'); app.finish()
        self.assertIn(b'/dev/tunentry', app.files)
        self.assertNotIn(FETCH_SCRIPT, app.files)
        self.assertFalse(any(b'rmmod' in command for command in app.commands))

    def test_log_memory_is_bounded_and_pagination_preserves_utf8(self):
        app = Installer()
        for _ in range(120):
            app.click('注入设置')
        app.click('安装日志')
        log_title = next(n for n in app.observed.nodes.values()
                         if (n.props[b'text'] or b'').startswith('安装日志 '.encode()))
        self.assertEqual(log_title.props[b'text'], '安装日志 12/12'.encode())
        for _ in range(16):
            app.click('上一页')
        self.assertEqual(log_title.props[b'text'], '安装日志 1/12'.encode())
        for n in app.observed.nodes.values():
            if n.props[b'text']:
                n.props[b'text'].decode('utf-8')


class PreviewLayoutTests(unittest.TestCase):
    """预览块里写死的几何必须与 chaos_installer.lua 建出来的节点一致。

    表盘列表的缩略图是 gen_preview.py 按 LAYOUTS/INSTALLER_BUTTONS 画的, 而真实
    界面在 installer/chaos_installer.lua。两边一旦漂开, 缩略图就和点进去的界面不是
    一回事(加过一张"安装日志"卡片而预览块没跟上就是这种事故)。这里拿夹具跑真 Lua,
    把主界面每个按钮的文案/y_ofs/宽高与预览块的常量逐个比一遍。
    """

    def setUp(self):
        if gen_preview is None:
            self.skipTest('gen_preview 需要 Pillow')
        self.app = Installer()
        self.nodes = self.app.observed.nodes
        self.root = self.nodes[1]
        self.main = next(n for n in self.nodes.values()
                         if n.parent is not None and n.parent.id == self.root.id
                         and self.y_ofs(n) == 0)

    @staticmethod
    def y_ofs(node):
        align = node.props[b'align']
        return None if align is None else align[b'y_ofs']

    def main_children(self):
        return [n for n in self.nodes.values()
                if n.parent is not None and n.parent.id == self.main.id]

    CLICKABLE = 2  # lvgl.FLAG.CLICKABLE: make_button 给每个按钮加的唯一标记

    def buttons(self):
        """主界面按钮: {文案: 按钮节点} —— 文案在按钮挂的 Label 上, 不在按钮自身。

        用 CLICKABLE 区分按钮与状态卡(卡片只加 EVENT_BUBBLE), 不去比对文案,
        否则文案改了就直接"少一个按钮", 报不出真正的问题。
        """
        found = {}
        for node in self.main_children():
            flags = node.flags
            if flags is None or not flags[self.CLICKABLE]:
                continue
            for label in self.nodes.values():
                text = label.props[b'text']
                if text is not None and label.parent is not None and label.parent.id == node.id:
                    found[text.decode()] = node
        return found

    def test_button_text_rows_and_sizes_match_preview_constants(self):
        layout = gen_preview.LAYOUTS['installer']
        buttons = self.buttons()
        self.assertEqual(set(buttons), {text for text, _ in gen_preview.INSTALLER_BUTTONS})
        self.assertEqual(len(buttons), len(layout['btn_ys']))
        for (text, _style), y_ofs in zip(gen_preview.INSTALLER_BUTTONS, layout['btn_ys']):
            node = buttons[text]
            self.assertEqual(self.y_ofs(node), y_ofs, text)
            self.assertEqual(node.props[b'w'], layout['btn_w'], text)
            self.assertEqual(node.props[b'h'], layout['btn_h'], text)

    def test_title_and_status_card_match_preview_constants(self):
        layout = gen_preview.LAYOUTS['installer']
        title = next(n for n in self.main_children() if n.props[b'text'] is not None)
        self.assertEqual(self.y_ofs(title), layout['title_y'])
        # 标题是个固定短串: 版本号在关于页与容器头, 标题里不重复。
        self.assertEqual(title.props[b'text'],
                         gen_preview.INSTALLER_TITLE.encode())
        card = next(n for n in self.main_children()
                    if n.props[b'text'] is None and self.y_ofs(n) == layout['card_y'])
        self.assertEqual(self.y_ofs(card), layout['card_y'])
        self.assertEqual((card.props[b'w'], card.props[b'h']),
                         (layout['card_w'], layout['card_h']))
        self.assertEqual(card.props[b'border_color'], 0x2A2A2C)

    FOREGROUND = {'primary': (0xF2, 0xF2, 0xF2), 'ghost': (0xB3, 0xB3, 0xB3),
                  'danger': (0xFF, 0x45, 0x3A)}

    def test_rendered_block_draws_button_labels_on_their_lua_rows(self):
        """画出来的文字行要落在 Lua 给的 y 上(证明常量真的进了画布)。"""
        layout = gen_preview.LAYOUTS['installer']
        width, height, palette, pixels = gen_preview.decode_block(gen_preview.build_blob())
        self.assertEqual((width, height), (gen_preview.W, gen_preview.H))

        def text_pixels(row, fg):
            base = row * width
            count = 0
            for x in range(width):
                i = pixels[base + x] * 4
                b, g, r, a = palette[i:i + 4]
                if a and all(abs(c - t) <= 12 for c, t in zip((r, g, b), fg)):
                    count += 1
            return count

        for (text, style), y_ofs in zip(gen_preview.INSTALLER_BUTTONS, layout['btn_ys']):
            center = gen_preview.H // 2 + y_ofs
            fg = self.FOREGROUND[style]
            # 文字的上下边界中点应落在按钮中心: 单看最亮一行会偏, 因为汉字
            # 各行的笔画密度不同(实测最大偏差 9px), 包围盒的中点才是排版中心。
            rows = [row for row in range(center - 24, center + 25) if text_pixels(row, fg)]
            self.assertTrue(rows, f'{text} 没画出来')
            midpoint = (rows[0] + rows[-1]) / 2
            self.assertLessEqual(abs(midpoint - center), 3, f'{text} 的文字行偏了: {midpoint} vs {center}')
            self.assertLessEqual(len(rows), 26, f'{text} 的文字高度异常: {len(rows)} 行')

if __name__ == '__main__':
    unittest.main()
