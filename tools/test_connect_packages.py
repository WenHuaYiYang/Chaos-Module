#!/usr/bin/env python3
"""Production C resource installer against POSIX VFS; no firmware emulation."""
import struct,zlib,unittest,shutil
from test_chaos_connect import ConnectTests,path
class PackageTests(ConnectTests):
 def setUp(self):
  super().setUp();shutil.rmtree(self.root/'data/chaos',ignore_errors=True)
  self.c.cc_test_android(1,0,1);self.c.cc_test_install_policy(2);self.request(1,b'CCA1')
 def begin(self,name='测试',kind=1,files=None,crc_delta=0):
  self.files=files or [('font.ttf',b'\0\1\0\0\0\1'+bytes(6)+b'head'+bytes(4)+struct.pack('>II',28,16)+bytes(16))]
  raw=name.encode();meta=bytes((kind,len(raw)))+raw+struct.pack('<HI',len(self.files),sum(len(b)for _,b in self.files))
  crc=zlib.crc32(meta)
  for nm,b in self.files:crc=zlib.crc32(path(nm)+struct.pack('<II',len(b),zlib.crc32(b))+b,crc)
  status,b=self.request(0x60,meta+struct.pack('<I',crc^crc_delta));self.assertEqual(status,0)
  self.token=struct.unpack_from('<I',b)[0];return b[4]
 def decide(self,allow=1):
  self.c.cc_test_decide(self.token,allow);self.c.cc_files_tick(100)
  status,b=self.request(0x61,struct.pack('<I',self.token));self.assertEqual(status,0);return b[4]
 def send_files(self,corrupt=False):
  for nm,b in self.files:
   status,_=self.request(0x62,struct.pack('<I',self.token)+path(nm)+struct.pack('<II',len(b),zlib.crc32(b)));self.assertEqual(status,0)
   for at in range(0,len(b),2048):self.assertEqual(self.request(0x63,struct.pack('<II',self.token,at)+b[at:at+2048])[0],0)
   if corrupt:self.c.cc_test_fault(0,17)
   self.assertEqual(self.request(0x64,struct.pack('<I',self.token))[0],0)
 def commit(self):return self.request(0x65,struct.pack('<I',self.token))[0]
 def test_font_requires_named_consent_and_no_write_or_nsh_permission(self):
  self.assertEqual(self.begin(),1);self.assertTrue(self.c.cc_test_state()&(1<<9));self.assertEqual(self.c.cc_test_token(),self.token)
  self.assertEqual(self.request(0x62,struct.pack('<I',self.token)+path('font.ttf')+struct.pack('<II',1,0))[0],3)
  # Rejection of premature writes aborts that task: start a fresh request.
  self.assertEqual(self.begin(),1);self.assertEqual(self.decide(),2);self.send_files();self.assertEqual(self.commit(),0)
  root=self.root/'data/chaos/font';self.assertEqual((root/'st1.ttf').read_bytes(),self.files[0][1]);self.assertEqual((root/'index.txt').read_bytes()[:16],b'01 '+ '测试'.encode()+b' '*6+b'\n')
 def test_icons_install_all_files_then_publish_index(self):
  self.begin('TestIcons',2,[('app.bin',b'\x19'+bytes(64)),('foo.bin',b'\x19'+bytes(65536))]);self.decide()
  self.c.cc_test_fault(17,0);self.send_files();self.assertFalse((self.root/'data/chaos/icons/index.txt').exists())
  self.assertEqual(self.commit(),0);root=self.root/'data/chaos/icons';self.assertEqual((root/'01/foo.bin').read_bytes(),self.files[1][1]);self.assertFalse(list(root.glob('.ccnp-*')))
 def test_reject_timeout_cancel_and_connection_loss_leave_no_public_resources(self):
  self.begin();self.assertEqual(self.decide(2),3);self.assertFalse((self.root/'data/chaos/font').exists())
  self.begin();
  for now in range(10100,60101,10000):self.c.cc_test_time(now);self.request(0x30)
  self.assertEqual(self.request(0x61,struct.pack('<I',self.token))[1][4],4)
  self.c.cc_test_time(100);self.request(1,b'CCA1');self.begin();self.decide();self.send_files();self.c.cc_files_reset()
  root=self.root/'data/chaos/font';self.assertFalse((root/'index.txt').exists());self.assertFalse(list(root.glob('.ccnp-*')))
 def test_crc_and_rename_failure_keep_existing_resources(self):
  self.begin('first');self.decide();self.send_files();self.assertEqual(self.commit(),0)
  root=self.root/'data/chaos/font';old=(root/'index.txt').read_bytes()
  self.begin('second',crc_delta=1);self.decide();self.send_files();self.assertEqual(self.commit(),10);self.assertEqual((root/'index.txt').read_bytes(),old)
  self.begin('second');self.decide();self.send_files();self.c.cc_test_fault(0,17);self.assertEqual(self.commit(),9);self.c.cc_test_fault(0,0)
  self.assertEqual((root/'index.txt').read_bytes(),old);self.assertTrue((root/'st1.ttf').exists());self.assertFalse((root/'st2.ttf').exists())
 def test_revocation_stale_token_paths_and_policy_files(self):
  self.begin();self.decide();old=self.token;self.send_files();self.c.cc_test_install_policy(1);self.request(0x30)
  self.assertEqual(self.commit(),3);self.c.cc_test_install_policy(2)
  self.begin(files=[('../escape',b'data')]);self.decide();self.assertEqual(self.request(0x62,struct.pack('<I',self.token)+path('../escape')+struct.pack('<II',4,0))[0],1)
  self.assertEqual(self.request(0x66,struct.pack('<I',old))[0],11)
  self.c.cc_test_android(1,0,63)
  for nm in ['/data/chaos/connect-install.cfg','/data/chaos/connect-install.tmp']:
   self.assertEqual(self.open_write(nm,5)[0],3)
 def test_duplicate_name_full_index_and_concurrent_local_change(self):
  self.begin('first');self.decide();self.send_files();self.assertEqual(self.commit(),0)
  self.begin('first');self.decide();nm,b=self.files[0];self.assertEqual(self.request(0x62,struct.pack('<I',self.token)+path(nm)+struct.pack('<II',len(b),zlib.crc32(b)))[0],4)
  self.begin('second');self.decide();self.send_files();root=self.root/'data/chaos/font';changed=b' '*128;(root/'index.txt').write_bytes(changed)
  self.assertEqual(self.commit(),8);self.assertEqual((root/'index.txt').read_bytes(),changed)
  full=b''.join(('%02d %-12s\n'%(i,'pack%d'%i)).encode()for i in range(1,9));self.assertEqual(len(full),128);(root/'index.txt').write_bytes(full)
  self.begin('third');self.decide();nm,b=self.files[0]
  self.assertEqual(self.request(0x62,struct.pack('<I',self.token)+path(nm)+struct.pack('<II',len(b),zlib.crc32(b)))[0],7)
 def test_all_256_icon_records_are_accepted_without_overflow(self):
  files=[('icon%03d.bin'%i,b'\x19'+bytes(12))for i in range(256)]
  self.begin('AllIcons',2,files);self.decide();self.send_files();self.assertEqual(self.commit(),0)
  self.assertEqual(len(list((self.root/'data/chaos/icons/01').glob('*.bin'))),256)
 def test_invalid_font_or_per_file_crc_never_publish(self):
  self.begin('broken',files=[('font.ttf',b'not a real font')]);self.decide()
  nm,b=self.files[0];self.assertEqual(self.request(0x62,struct.pack('<I',self.token)+path(nm)+struct.pack('<II',len(b),zlib.crc32(b)))[0],0)
  self.assertEqual(self.request(0x63,struct.pack('<II',self.token,0)+b)[0],0)
  self.assertEqual(self.request(0x64,struct.pack('<I',self.token))[0],1)
  self.assertFalse((self.root/'data/chaos/font/index.txt').exists())
  self.begin('badcrc');self.decide();nm,b=self.files[0]
  self.assertEqual(self.request(0x62,struct.pack('<I',self.token)+path(nm)+struct.pack('<II',len(b),zlib.crc32(b)^1))[0],0)
  self.assertEqual(self.request(0x63,struct.pack('<II',self.token,0)+b)[0],0)
  self.assertEqual(self.request(0x64,struct.pack('<I',self.token))[0],10)
  self.assertFalse((self.root/'data/chaos/font/index.txt').exists())
if __name__=='__main__':
 suite=unittest.TestSuite(PackageTests(n)for n in PackageTests.__dict__ if n.startswith('test_'));result=unittest.TextTestRunner(verbosity=2).run(suite);raise SystemExit(not result.wasSuccessful())
