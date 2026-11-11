#!/usr/bin/env python3
"""CIPK compatibility, production Lua delivery, and streamed-player regressions."""
from pathlib import Path
import io
import json
import struct
import subprocess
import sys
import tempfile
import unittest
from PIL import Image
from lupa import LuaRuntime
import build_icon_pack as pack
from gen_chaos_icon import encode_icon,render_icon

ROOT=Path(__file__).resolve().parents[1]
ANIMATION=(ROOT/'supervisor/assets/dynamic-icon.frames').read_bytes()
with Image.open(ROOT/'tools/chaos.png') as source: POSTER=encode_icon(render_icon(source,96))

def manifest(mask): return struct.pack('<4s4HQ',b'CIA1',96,96,38,0,mask)

class Delivery:
    def __init__(self,payload,*,index=None,short=None,write_failure=None,rename_failure=False,script=None):
        self.files={b'/resource/pack.bin':payload}
        if index is not None:self.files[b'/data/chaos/icons/index.txt']=index
        self.handles={};self.next=1;self.max_read=0;self.short=short;self.write_failure=write_failure;self.rename_failure=rename_failure
        self.lua=LuaRuntime(encoding=None,unpack_returned_tuples=True)
        g=self.lua.globals()
        for key,value in {b'py_open':self.open,b'py_read':self.read,b'py_write':self.write,b'py_close':self.close,b'py_remove':self.remove,b'py_rename':self.rename,b'py_execute':self.execute}.items():g[key]=value
        self.lua.execute(b'''
        observed={messages={}}
        io={open=function(path,mode)
            local id=py_open(path,mode);if not id then return nil,'open failed' end
            local f={}
            function f:read(n) return py_read(id,n) end
            function f:write(s) if py_write(id,s) then return self end return nil,'full' end
            function f:close() return py_close(id) end
            return f
        end}
        os={remove=py_remove,rename=py_rename,execute=py_execute}
        local function node(parent,props)
            local n={props=props}
            function n:set(p) if p.text then table.insert(observed.messages,p.text) end end
            function n:clear_flag(_) end
            function n:add_flag(_) end
            function n:onClicked(cb) observed.click=cb end
            return n
        end
        local lvgl={Object=node,Label=node,OPA=function(v)return v end,Font=function(...)return 0 end,
          HOR_RES=function()return 336 end,VER_RES=function()return 480 end,
          FLAG={SCROLLABLE=1,CLICKABLE=2,EVENT_BUBBLE=4},ALIGN={CENTER=0}}
        lvgl.Timer=function(options)
            options.paused=true
            function options:pause()self.paused=true end
            function options:resume()self.paused=false end
            function options:ready()end
            observed.timer=options;return options
        end
        require=function(name)assert(name=='lvgl');return lvgl end
        SCRIPT_PATH='/resource/'
        ''')
        self.lua.execute(script if script is not None else pack.delivery_lua('Test','图标投递 Test').encode())
    def open(self,path,mode):
        if mode==b'rb' and path not in self.files:return None
        if mode==b'wb':self.files[path]=b''
        handle=self.next;self.next+=1;self.handles[handle]=[path,mode,0];return handle
    def read(self,handle,n):
        assert isinstance(n,int),'delivery must not read whole files'
        self.max_read=max(self.max_read,n)
        name,mode,at=self.handles[handle]
        if self.short is not None:n=min(n,self.short)
        result=self.files[name][at:at+n];self.handles[handle][2]+=len(result)
        return result or None
    def write(self,handle,data):
        name,mode,at=self.handles[handle]
        if self.write_failure and self.write_failure in name:return False
        self.files[name]+=data;return True
    def close(self,handle):self.handles.pop(handle);return True
    def remove(self,name):self.files.pop(name,None);return True
    def rename(self,a,b):
        if self.rename_failure:return False
        self.files[b]=self.files.pop(a);return True
    def execute(self,command):
        prefix=b'rm -rf '
        if command.startswith(prefix):
            directory=command[len(prefix):]+b'/'
            for name in list(self.files):
                if name.startswith(directory):del self.files[name]
        else:assert command.startswith(b'mkdir -p /data/chaos/icons/')
        return True
    def run(self):
        observed=self.lua.globals().observed;observed.click()
        for _ in range(20000):
            timer=observed.timer
            if timer is None or timer.paused:break
            timer.cb(timer)
        else:raise AssertionError('Lua delivery did not terminate')
        return observed.messages[len(observed.messages)].decode()

class PackTests(unittest.TestCase):
    def test_legacy_cipk_wire_format_and_static_delivery(self):
        payload=pack.encode_cipk([('activities.bin',POSTER)])
        self.assertEqual(payload[:8],b'CIPK\x01\0\0\0')
        self.assertEqual(pack.decode_cipk(payload),[('activities.bin',POSTER)])
        delivery=Delivery(payload);self.assertIn('已投递',delivery.run())
        self.assertEqual(delivery.files[b'/data/chaos/icons/01/activities.bin'],POSTER)
        self.assertEqual(len(delivery.files[b'/data/chaos/icons/index.txt']),128)
    def test_animation_over_old_512k_limit_streams_and_reads_back_in_chunks(self):
        self.assertGreater(len(ANIMATION),512*1024)
        entries=[('activities.bin',POSTER),('activities_anim.bin',ANIMATION),('animations.bin',manifest(1))]
        delivery=Delivery(pack.encode_cipk(entries));self.assertIn('已投递 3',delivery.run())
        for name,data in entries:self.assertEqual(delivery.files[b'/data/chaos/icons/01/'+name.encode()],data)
        self.assertLessEqual(delivery.max_read,65536);self.assertFalse(delivery.handles)
    def test_short_body_reads_preserve_all_bytes(self):
        delivery=Delivery(pack.encode_cipk([('activities.bin',POSTER)]),short=257)
        self.assertIn('已投递',delivery.run());self.assertEqual(delivery.files[b'/data/chaos/icons/01/activities.bin'],POSTER)
    def test_existing_names_reuse_slots_and_full_index_is_not_overwritten(self):
        old=b'02 Test        \n'.ljust(128,b' ')
        delivery=Delivery(pack.encode_cipk([('activities.bin',POSTER)]),index=old)
        self.assertIn('已投递',delivery.run());self.assertIn(b'/data/chaos/icons/02/activities.bin',delivery.files)
        full=b''.join(f'{i:02d} {("Pack"+str(i)):<12}\n'.encode() for i in range(1,9))
        self.assertEqual(len(full),128)
        delivery=Delivery(pack.encode_cipk([('activities.bin',POSTER)]),index=full)
        self.assertIn('已满',delivery.run());self.assertEqual(delivery.files[b'/data/chaos/icons/index.txt'],full)
    def test_traversal_duplicates_truncation_and_trailing_data_do_not_register(self):
        good=pack.encode_cipk([('activities.bin',POSTER)])
        name=b'../outside.bin'
        bad_name=b'CIPK'+struct.pack('<IIB',1,1,len(name))+name+b'x'
        duplicate=b'CIPK'+struct.pack('<I',2)+good[8:]*2
        for payload in [b'BAD!'+good[4:],good[:-1],good+b'x',bad_name,duplicate]:
            with self.subTest(length=len(payload)):
                delivery=Delivery(payload);self.assertNotIn('已投递',delivery.run())
                self.assertNotIn(b'/data/chaos/icons/index.txt',delivery.files)
    def test_failed_file_or_index_writes_preserve_previous_index(self):
        old=b'02 Other       \n'.ljust(128,b' ')
        for failed in [b'activities.bin',b'index.txt.tmp']:
            delivery=Delivery(pack.encode_cipk([('activities.bin',POSTER)]),index=old,write_failure=failed)
            self.assertNotIn('已投递',delivery.run());self.assertEqual(delivery.files[b'/data/chaos/icons/index.txt'],old)
        delivery=Delivery(pack.encode_cipk([('activities.bin',POSTER)]),index=old,rename_failure=True)
        self.assertNotIn('已投递',delivery.run());self.assertEqual(delivery.files[b'/data/chaos/icons/index.txt'],old)
    def test_builder_makes_static_fallback_manifest_and_complete_p67_delivery(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);assets=root/'assets';assets.mkdir()
            (assets/'activities_anim.bin').write_bytes(ANIMATION)
            (assets/'activities.bin').write_bytes(POSTER)
            (assets/'alarm.bin').write_bytes(POSTER)
            result=pack.build(assets,'Test',root/'delivery.bin',raw_output=root/'pack.bin')
            entries=dict(pack.decode_cipk((root/'pack.bin').read_bytes()))
            self.assertEqual(entries['animations.bin'],manifest(1));self.assertEqual(entries['activities.bin'],POSTER)
            self.assertEqual(result['animations']['activities']['frames'],193)
            info=pack.p67_pack.inspect(root/'delivery.bin');self.assertEqual(len(info['files']),2)
            raw=(root/'delivery.bin').read_bytes();self.assertIn('图标投递 Test'.encode(),raw)
            cli=subprocess.run([sys.executable,str(ROOT/'tools/build_icon_pack.py'),'--assets',str(assets),
                '--name','Test','--output',str(root/'cli.bin')],check=True,capture_output=True,text=True)
            self.assertEqual(json.loads(cli.stdout)['files'],4)
    def test_bad_metadata_rle_dimensions_and_missing_posters_are_rejected(self):
        for at in [0,4,8,12,20,24]:
            bad=bytearray(ANIMATION);bad[at]^=255
            with self.assertRaises(ValueError):pack.validate_animation(bad)
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);(p/'activities_anim.bin').write_bytes(ANIMATION)
            with self.assertRaisesRegex(ValueError,'poster'):pack.collect(p)
            (p/'activities.bin').write_bytes(POSTER)
            (p/'set_notify.gif').write_bytes((ROOT/'tools/chaos.gif').read_bytes())
            with self.assertRaisesRegex(ValueError,'static'):pack.collect(p)
    def test_legacy_non96_native_static_images_remain_usable(self):
        with Image.open(ROOT/'tools/chaos.png') as image:data=encode_icon(render_icon(image,112))
        self.assertEqual(pack.validate_static(data),(112,112))
        with self.assertRaisesRegex(ValueError,'96x96'):pack.validate_static(data,True)
    def test_partial_static_pack_has_zero_animation_manifest_for_pack_presence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);(root/'alarm.bin').write_bytes(POSTER)
            entries,details=pack.collect(root)
            self.assertEqual(dict(entries),{'alarm.bin':POSTER,'animations.bin':manifest(0)})
            self.assertEqual(details,{})
    def test_gif_input_generates_its_poster_and_exact_animation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);(root/'activities.gif').write_bytes((ROOT/'tools/chaos.gif').read_bytes())
            entries,details=pack.collect(root);entries=dict(entries)
            self.assertEqual(entries['activities_anim.bin'],ANIMATION)
            self.assertEqual(entries['animations.bin'],manifest(1))
            with Image.open(root/'activities.gif') as image:expected=encode_icon(render_icon(image,96))
            self.assertEqual(entries['activities.bin'],expected)
            self.assertEqual(details['activities']['cycle_ms'],5200)
    @unittest.skipUnless(sys.platform=='linux','native low-address fixture runs on Linux')
    def test_production_streamed_player(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary=Path(tmp)/'player'
            subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'supervisor/tests/icon_pack_animation_harness.rs'),'-o',str(binary)],check=True,timeout=60)
            subprocess.run([str(binary),'--test-threads=1'],check=True,timeout=30)

if __name__=='__main__':unittest.main()
