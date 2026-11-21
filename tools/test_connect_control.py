#!/usr/bin/env python3
"""Focused production CCN1 gate checks; the UI mailbox is a test stand-in."""
import struct,unittest
from test_chaos_connect import ConnectTests
class ControlTests(ConnectTests):
 def setUp(self):
  super().setUp();self.c.cc_test_android(1,0,1);self.request(1,b'CCA1')
 def test_control_permission_revocation_and_bridge_disable(self):
  before=self.c.cc_test_control_counter(0)
  for op,p in [(0x70,b'\0\0'),(0x71,b'\x04\0\x01\0' + b'1'),(0x72,struct.pack('<I',1))]:self.assertEqual(self.request(op,p)[0],3)
  self.assertEqual(self.c.cc_test_control_counter(0),before)
  self.c.cc_test_android(1,0,16)
  status,b=self.request(0x70,b'\0\0');self.assertEqual(status,0);token=struct.unpack('<I',b)[0]
  self.assertEqual(self.request(0x72,struct.pack('<I',token)),(0,b'\1\0'))
  self.c.cc_test_android(1,0,1);self.assertEqual(self.request(0x72,struct.pack('<I',token))[0],3)
  self.c.cc_test_android(1,0,16);self.c.cc_test_service(0);self.assertEqual(self.request(0x71,b'\x04\0\x01\0'+b'1')[0],3)
 def test_plugin_cannot_impersonate_android_and_new_session_expires_token(self):
  self.c.cc_test_android(1,0,16);_,b=self.request(0x70,b'\0\0');token=struct.unpack('<I',b)[0]
  self.request(1,b'CCA1');self.assertEqual(self.request(0x72,struct.pack('<I',token))[0],11)
  self.c.cc_test_android(0,0,63);self.c.cc_test_plugin(0,63);_,hello=self.request(1,b'CCA1');self.assertEqual(hello[3]&64,0)
  self.assertEqual(self.request(0x70,b'\0\0')[0],12)
 def test_connection_confirmation_is_required_even_with_control_bit(self):
  self.c.cc_test_android(1,2,16);_,hello=self.request(1,b'CCA1');self.assertTrue(hello[3]&64)
  self.assertEqual(self.request(0x70,b'\0\0')[0],3)
  self.c.cc_test_decide(self.c.cc_test_token(),1);self.c.cc_files_tick(101)
  self.assertEqual(self.request(0x70,b'\0\0')[0],0)
if __name__=='__main__':
 suite=unittest.TestSuite(ControlTests(name) for name in ControlTests.__dict__ if name.startswith('test_'));result=unittest.TextTestRunner(verbosity=2).run(suite);raise SystemExit(not result.wasSuccessful())
