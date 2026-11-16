#!/usr/bin/env python3
"""Run the real installer Lua against a filesystem, UI and timer fixture."""
from pathlib import Path
import fnmatch
import shlex
import struct
import unittest
from lupa import LuaRuntime

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
          function n:add_flag(_)end
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


if __name__ == '__main__':
    unittest.main()
