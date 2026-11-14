#!/usr/bin/env python3
"""Exercise the production private codec and VFS engine; throughput here is not Bluetooth evidence."""
from pathlib import Path
import ctypes as C
import os,random,struct,subprocess,tempfile,unittest,zlib
ROOT=Path(__file__).resolve().parents[1]
CHUNK=2048

def frame(op,req=1,payload=b'',flags=0,status=0):
    h=b'CCN1'+bytes((1,flags,op,status))+struct.pack('<HH',req,len(payload))
    return h+struct.pack('<I',zlib.crc32(h+payload))+payload

def path(s):
    s=s.encode();return struct.pack('<H',len(s))+s

class ConnectTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.work=tempfile.TemporaryDirectory(prefix='chaos-connect-');cls.root=Path(cls.work.name)
        lib=cls.root/'connect.so'
        subprocess.run(['clang','-std=c11','-D_DEFAULT_SOURCE','-shared','-fPIC','-O2','-Wall','-Wextra','-Werror','-I'+str(ROOT/'supervisor/bridge'),str(ROOT/'supervisor/tests/chaos_connect_host.c'),str(ROOT/'supervisor/bridge/chaos_connect.c'),str(ROOT/'supervisor/bridge/chaos_files.c'),'-o',str(lib)],check=True)
        cls.c=C.CDLL(str(lib));cls.c.cc_crc32.argtypes=[C.c_uint32,C.c_void_p,C.c_uint32];cls.c.cc_crc32.restype=C.c_uint32
        os.environ['CHAOS_CONNECT_TEST_ROOT']=str(cls.root)
        (cls.root/'data').mkdir();(cls.root/'resource').mkdir()
    @classmethod
    def tearDownClass(cls):cls.c.cc_files_reset();cls.work.cleanup()
    def setUp(self):
        self.c.cc_files_reset();self.c.cc_test_fault(0,0);self.c.cc_test_policy(0);self.c.cc_test_time(100)
        self.request(1);self.c.cc_test_clear_logs()
    def logs(self):
        out=[];buf=C.create_string_buffer(1024)
        for i in range(256):
            if not self.c.cc_test_log(i,buf):break
            out.append(buf.value.decode('utf-8'))
        return '\n'.join(out)
    def request(self,op,payload=b'',req=1):
        wire=frame(op,req,payload);self.assertEqual(self.c.cc_test_dispatch(wire,len(wire)),0)
        replies=[];buf=C.create_string_buffer(4096)
        for i in range(32):
            n=self.c.cc_test_reply(i,buf)
            if not n:break
            p=buf.raw[:n];self.assertEqual(p,frame(p[6],struct.unpack_from('<H',p,8)[0],p[16:],p[5],p[7]));replies.append(p)
        direct=[p for p in replies if p[5]==1];self.assertEqual(len(direct),1)
        self.assertEqual(direct[0][6],op);self.assertEqual(struct.unpack_from('<H',direct[0],8)[0],req)
        return direct[0][7],direct[0][16:]
    def open_write(self,target,size,overwrite=1):return self.request(0x20,path(target)+struct.pack('<IB',size,overwrite))
    def test_crc_and_invalid_private_frames(self):
        self.assertEqual(self.c.cc_crc32(0xffffffff,b'123456789',9)^0xffffffff,0xcbf43926)
        good=frame(1,payload=b'example')
        for n in range(len(good)):self.assertNotEqual(self.c.cc_test_dispatch(good,n),0)
        for at in range(len(good)):
            bad=bytearray(good);bad[at]^=1;self.assertNotEqual(self.c.cc_test_dispatch(bytes(bad),len(bad)),0)
        for old in (b'CPC2'+bytes(32),b'\x01\x01\x00'):self.assertNotEqual(self.c.cc_test_dispatch(old,len(old)),0)
    def test_upload_short_writes_crc_commit_and_windowed_download(self):
        data=random.Random(7141).randbytes(256*1024+19)
        self.assertEqual(self.request(1)[0],0);self.assertEqual(self.open_write('/data/roundtrip.bin',len(data))[0],0)
        self.c.cc_test_fault(31,0)
        for offset in range(0,len(data),CHUNK):
            part=data[offset:offset+CHUNK];status,ack=self.request(0x21,struct.pack('<I',offset)+part,offset//CHUNK+2)
            self.assertEqual(status,0);self.assertEqual(struct.unpack('<II',ack),(offset+len(part),zlib.crc32(data[:offset+len(part)])))
        self.assertEqual(self.request(0x22,struct.pack('<I',zlib.crc32(data)))[0],0)
        self.assertEqual((self.root/'data/roundtrip.bin').read_bytes(),data)
        self.assertEqual(self.request(0x10,path('/data/roundtrip.bin')),(0,struct.pack('<I',len(data))))
        result=bytearray(len(data))
        for offset in reversed(range(0,len(data),CHUNK)):
            n=min(CHUNK,len(data)-offset);status,part=self.request(0x11,struct.pack('<IH',offset,n));self.assertEqual(status,0);self.assertEqual(len(part),n);result[offset:offset+n]=part
        self.assertEqual(bytes(result),data);self.assertEqual(self.request(0x12)[0],0)
    def test_failed_crc_rename_and_disconnect_preserve_existing_target(self):
        target=self.root/'data/original';target.write_bytes(b'original')
        self.assertEqual(self.open_write('/data/original',3)[0],0);self.request(0x21,struct.pack('<I',0)+b'new')
        self.assertEqual(self.request(0x22,struct.pack('<I',0))[0],10);self.assertEqual(target.read_bytes(),b'original')
        self.open_write('/data/original',3);self.request(0x21,struct.pack('<I',0)+b'new');self.c.cc_test_fault(0,17)
        self.assertNotEqual(self.request(0x22,struct.pack('<I',zlib.crc32(b'new')))[0],0);self.assertEqual(target.read_bytes(),b'original')
        self.c.cc_test_fault(0,0);self.open_write('/data/original',10);self.c.cc_files_reset()
        self.assertEqual(target.read_bytes(),b'original');self.assertFalse(list((self.root/'data').glob('*.part')))
    def test_partial_io_failure_discards_transaction(self):
        target=self.root/'data/partial';target.write_bytes(b'keep')
        self.assertEqual(self.open_write('/data/partial',100)[0],0);self.c.cc_test_fault(-17,0)
        self.assertEqual(self.request(0x21,struct.pack('<I',0)+bytes(100))[0],9)
        self.assertEqual(self.request(0x22,struct.pack('<I',zlib.crc32(bytes(100))))[0],1)
        self.assertEqual(target.read_bytes(),b'keep');self.assertFalse(list((self.root/'data').glob('*.part')))
    def test_path_policy_directory_and_oversize(self):
        for p in ('/proc/test','/data/../dev/test','/sys/test'):
            self.assertEqual(self.open_write(p,1)[0],3)
        self.assertEqual(self.open_write('/resource/test',1)[0],3)
        self.assertEqual(self.request(0x40,b'\x01')[0],0);self.assertEqual(self.open_write('/resource/test',1)[0],0)
        self.assertEqual(self.request(0x23)[0],0)
        self.assertEqual(self.open_write('/data/huge',32*1024*1024+1)[0],13)
        self.assertEqual(self.request(4,path('/data/dir'))[0],0);(self.root/'data/dir/a').write_text('keep')
        self.assertEqual(self.request(6,path('/data/dir'))[0],8);self.assertTrue((self.root/'data/dir/a').exists())
        status,data=self.request(2,path('/data')+struct.pack('<IB',0,16));self.assertEqual(status,0);self.assertGreaterEqual(struct.unpack_from('<H',data)[0],1)
    def test_pb_namespace_and_frame_roundtrip(self):
        out=C.create_string_buffer(4096);f=frame(0x11,7,bytes(range(256))*8)
        n=self.c.cc_pb_wrap(out,len(out),f,len(f),0x12340007);self.assertGreater(n,0)
        packet=out.raw[:n];ptr=C.c_void_p();size=C.c_uint32();pid=C.c_uint32()
        self.assertEqual(self.c.cc_pb_unwrap(packet,n,C.byref(ptr),C.byref(size),C.byref(pid)),1)
        self.assertEqual((C.string_at(ptr,size.value),pid.value),(f,0x12340007))
        self.assertEqual(self.c.cc_pb_unwrap(b'\x08\x02\x10\x00',4,C.byref(ptr),C.byref(size),C.byref(pid)),0)
        for n in range(len(packet)):self.assertNotEqual(self.c.cc_pb_unwrap(packet,n,C.byref(ptr),C.byref(size),C.byref(pid)),1)
    def test_shared_c_rust_wire_vector(self):
        expected_frame,expected_packet=[bytes.fromhex(line) for line in (ROOT/'docs/CHAOSCONNECT-V1.hex').read_text().splitlines()]
        self.assertEqual(expected_frame,frame(1,0x1234))
        out=C.create_string_buffer(4096)
        n=self.c.cc_pb_wrap(out,len(out),expected_frame,len(expected_frame),0x12341234)
        self.assertEqual(out.raw[:n],expected_packet)
    def test_copy_rename_stat_and_negotiated_chunk(self):
        source=self.root/'data/source';source.write_bytes(b'copy contents')
        self.assertEqual(self.request(8,path('/data/source')+path('/data/copy')+b'\x00')[0],0)
        self.assertEqual((self.root/'data/copy').read_bytes(),source.read_bytes())
        self.assertEqual(self.request(7,path('/data/copy')+path('/data/moved'))[0],0)
        status,payload=self.request(3,path('/data/moved'));self.assertEqual(status,0);self.assertEqual(struct.unpack_from('<I',payload,1)[0],len(source.read_bytes()))
        self.c.cc_files_set_chunk(128);status,hello=self.request(1);self.assertEqual((status,struct.unpack_from('<H',hello,1)[0]),(0,128))
        self.assertEqual(self.open_write('/data/small',129)[0],0)
        self.assertEqual(self.request(0x21,struct.pack('<I',0)+bytes(129))[0],13)
        self.assertEqual(self.request(0x23)[0],0)
        status,echo=self.request(0x31,bytes(range(128)));self.assertEqual((status,echo),(0,bytes(range(128))))
        self.c.cc_files_set_chunk(CHUNK)

    def test_info_audit_tracks_paths_actions_results_and_never_file_contents(self):
        self.request(1)
        folder='/data/日志测试';target=folder+'/说明.txt'
        self.request(4,path(folder))
        self.request(2,path(folder)+struct.pack('<IB',0,16))
        data=b'private contents must not appear in audit'
        self.open_write(target,len(data));self.request(0x21,struct.pack('<I',0)+data)
        self.request(0x22,struct.pack('<I',zlib.crc32(data)))
        self.request(3,path(target));self.request(0x10,path(target))
        self.request(0x11,struct.pack('<IH',0,len(data)));self.request(0x12)
        self.request(8,path(target)+path(folder+'/副本.txt')+b'\x00')
        self.request(7,path(folder+'/副本.txt')+path(folder+'/移动.txt'))
        self.request(5,path(folder+'/移动.txt'))
        self.request(6,path(folder)) # not empty
        self.open_write('/proc/denied',1)
        logs=self.logs()
        for s in ['连接/协商协议','chunk=2048','浏览文件夹 "'+folder+'"',
                  '打开文件读取（预览/下载） "'+target+'"',
                  '打开文件写入（上传/保存编辑） "'+target+'"',
                  '查看文件/目录信息','复制文件','移动/重命名','删除文件',
                  '目录非空','没有权限','校验并提交文件 "'+target+'"','req=',
                  'committed_offset=','returned_bytes=','transferred_bytes=']:
            self.assertIn(s,logs)
        self.assertNotIn(data.decode(),logs)

    def test_failed_commit_and_session_reset_retain_auditable_target(self):
        self.open_write('/data/audit-failed.txt',3)
        self.request(0x21,struct.pack('<I',0)+b'abc')
        self.request(0x22,struct.pack('<I',0))
        self.assertIn('校验并提交文件 "/data/audit-failed.txt"',self.logs())
        self.assertIn('CRC校验失败',self.logs())
        self.open_write('/data/audit-reset.txt',3);self.c.cc_files_reset()
        self.assertIn('取消未提交写入并清理临时文件 "/data/audit-reset.txt"',self.logs())

    def test_large_transfer_logs_checkpoints_without_per_packet_flood(self):
        data=b'z'*(256*1024)
        self.open_write('/data/audit-large.bin',len(data))
        for offset in range(0,len(data),CHUNK):
            self.request(0x21,struct.pack('<I',offset)+data[offset:offset+CHUNK])
        self.request(0x22,struct.pack('<I',zlib.crc32(data)))
        self.assertLess(len(self.logs().splitlines()),12)
        self.assertIn('committed_offset=65536',self.logs())
        self.assertIn('written_bytes=262144',self.logs())

    def device_status(self):
        status,p=self.request(0x30)
        self.assertEqual(status,0);self.assertEqual(len(p),8)
        return tuple(p[:4])+(struct.unpack_from('<I',p,4)[0],)

    def test_developer_permission_allow_deny_ask_and_token_bound_decisions(self):
        self.c.cc_test_policy(1)
        self.assertEqual(self.request(0x40,b'\x01')[0],3)
        self.assertEqual(self.device_status()[:3],(1,0,1))
        self.c.cc_test_policy(2)
        status,p=self.request(0x40,b'\x01');self.assertEqual(status,14)
        token=struct.unpack('<I',p)[0];self.assertNotEqual(token,0)
        self.assertEqual(self.request(0x40,b'\x01'),(14,p)) # repeated request reuses one prompt
        self.assertEqual(self.open_write('/resource/not-yet-allowed',1)[0],3)
        self.c.cc_test_decide(token+1,1);self.c.cc_files_tick(100)
        self.assertEqual(self.device_status()[1],0)
        self.c.cc_test_decide(token,1);self.c.cc_files_tick(100)
        self.assertEqual(self.device_status(),(1,1,2,2,token))
        self.assertEqual(self.open_write('/resource/allowed',0)[0],0)
        self.request(0x23)
        self.request(0x40,b'\x00')
        status,p=self.request(0x40,b'\x01');token2=struct.unpack('<I',p)[0]
        self.assertNotEqual(token,token2)
        self.c.cc_test_decide(token2,2);self.c.cc_files_tick(100)
        self.assertEqual(self.device_status(),(1,0,2,3,token2))

    def test_revoke_cancels_system_write_and_preserves_existing_file(self):
        target=self.root/'resource/revoke';target.write_bytes(b'original')
        self.assertEqual(self.request(0x40,b'\x01')[0],0)
        self.assertEqual(self.open_write('/resource/revoke',3)[0],0)
        self.request(0x21,struct.pack('<I',0)+b'new')
        self.c.cc_test_policy(1);self.c.cc_files_tick(100)
        self.assertEqual(self.device_status()[1],0)
        self.assertNotEqual(self.request(0x22,struct.pack('<I',zlib.crc32(b'new')))[0],0)
        self.assertEqual(target.read_bytes(),b'original')
        self.assertFalse(list((self.root/'resource').glob('*.part')))

    def test_peer_cannot_overwrite_consent_policy_or_rename_its_ancestors(self):
        self.request(0x40,b'\x01')
        for p in ['/data/chaos/connect-permissions.cfg','/data/chaos/connect-permissions.tmp','/data/chaos','/data','/']:
            self.assertEqual(self.open_write(p,5)[0],3)
            self.assertEqual(self.request(7,path(p)+path('/data/moved-policy'))[0],3)
        self.assertEqual(self.request(7,path('/data/other')+path('/data/chaos'))[0],3)

    def test_authorization_cancel_new_session_timeout_and_lease_expiry(self):
        self.c.cc_test_policy(2)
        _,p=self.request(0x40,b'\x01');old=struct.unpack('<I',p)[0]
        self.request(0x40,b'\x00');self.c.cc_test_decide(old,1);self.c.cc_files_tick(100)
        self.assertEqual(self.device_status()[1],0)
        _,p=self.request(0x40,b'\x01');old=struct.unpack('<I',p)[0]
        self.request(1)
        _,p=self.request(0x40,b'\x01');new=struct.unpack('<I',p)[0]
        self.assertNotEqual(old,new)
        self.c.cc_test_decide(old,1);self.c.cc_files_tick(100)
        self.assertEqual(self.device_status()[1],0)
        # Keep the lease alive while allowing the 60-second authorization to expire.
        for now in range(10100,60101,10000):
            self.c.cc_test_time(now);self.request(0x30);self.c.cc_files_tick(now)
        self.assertEqual(self.device_status()[1:4],(0,2,4))
        self.c.cc_test_decide(new,1);self.c.cc_files_tick(60100)
        self.assertEqual(self.device_status()[1],0)
        self.c.cc_test_time(75100);self.assertEqual(self.c.cc_files_tick(75100),1)
        self.assertEqual(self.c.cc_test_state(),0)
        self.assertEqual(self.request(0x40,b'\x01')[0],3)
        self.request(1);self.assertEqual(self.device_status()[:2],(1,0))

    def test_lease_reset_closes_files_and_clock_wrap_is_safe(self):
        self.open_write('/data/lease-reset',3)
        self.c.cc_test_time(15100);self.assertEqual(self.c.cc_files_tick(15100),1)
        self.assertFalse(list((self.root/'data').glob('lease-reset*.part')))
        self.c.cc_test_time(0xfffffff0);self.request(1)
        self.assertEqual(self.c.cc_files_tick(0x10),0)
        self.assertEqual(self.c.cc_files_tick((0xfffffff0+15000)&0xffffffff),1)

class MemoryLogTests(unittest.TestCase):
    def test_permission_persistence_mailbox_and_global_prompt(self):
        with tempfile.TemporaryDirectory(prefix='chaos-connect-permission-') as work:
            binary=Path(work)/'permissions'
            subprocess.run(['rustc','--edition=2021',str(ROOT/'supervisor/tests/connect_permission_harness.rs'),'-o',str(binary)],check=True)
            for case in ['default','invalid','persist','mailbox','prompt']:
                subprocess.run([str(binary),case],check=True)
    def test_ring_wrap_pagination_clear_and_utf8(self):
        with tempfile.TemporaryDirectory(prefix='chaos-connect-log-') as work:
            binary=Path(work)/'log-tests'
            subprocess.run(['rustc','--test',str(ROOT/'supervisor/src/connect_log_buffer.rs'),'-o',str(binary)],check=True)
            subprocess.run([str(binary)],check=True)
    def test_log_page_long_paths_paging_and_clear(self):
        with tempfile.TemporaryDirectory(prefix='chaos-connect-log-ui-') as work:
            binary=Path(work)/'log-ui-tests'
            subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'supervisor/tests/connect_log_page_harness.rs'),'-o',str(binary)],check=True,env=dict(os.environ,CARGO_PKG_VERSION='test'))
            subprocess.run([str(binary),'--test-threads=1'],check=True)

if __name__=='__main__':unittest.main()
