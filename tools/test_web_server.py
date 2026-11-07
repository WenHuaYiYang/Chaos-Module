#!/usr/bin/env python3
"""Run the production HTTP/WebSocket parser against a host C transport.

No firmware, Bluetooth, private lwIP, or real NSH execution is simulated as a
successful device test. Those integrations require the p67 hardware matrix.
"""
import base64
import ctypes
import gzip
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile
import unittest

from embed_web import WEB, generated_header, rendered_html

ROOT = Path(__file__).resolve().parents[1]
AUTH = base64.b64encode(b'chaos:host-test-pass')


def request(path='/', method='GET', body=b'', auth=True, headers=b''):
    return (f'{method} {path} HTTP/1.1\r\nHost: 192.168.44.2\r\n'.encode()
            + (b'Authorization: Basic ' + AUTH + b'\r\n' if auth else b'')
            + headers + f'Content-Length: {len(body)}\r\n\r\n'.encode() + body)


def frame(payload=b'', op=2, fin=True, masked=True):
    n = len(payload)
    mask = b'\x12\x34\x56\x78'
    prefix = bytes([(128 if fin else 0) | op, (128 if masked else 0) | (n if n < 126 else 126)])
    if n >= 126:
        prefix += struct.pack('!H', n)
    return prefix + (mask + bytes(v ^ mask[i % 4] for i, v in enumerate(payload)) if masked else payload)


def decode_frames(data):
    result = []
    while data:
        op, n = data[0] & 15, data[1] & 127
        at = 2
        if n == 126:
            n = struct.unpack('!H', data[2:4])[0]
            at = 4
        result.append((op, data[at:at+n]))
        data = data[at+n:]
    return result


HANDSHAKE = (b'Upgrade: websocket\r\nConnection: keep-alive, Upgrade\r\n'
             b'Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n')


class View(ctypes.Structure):
    _fields_ = [(n, ctypes.c_uint32) for n in ['enabled', 'listening', 'clients', 'requests', 'failures', 'refused']] + [('last_error', ctypes.c_int32)]


class WebTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='chaos-httpd-')
        binary = Path(cls.temp.name) / 'httpd.so'
        compiler = os.environ.get('CC') or shutil.which('clang') or shutil.which('cc')
        if not compiler:
            raise RuntimeError('clang or a C11 host compiler is required')
        subprocess.run([compiler, '-std=c11', '-Wall', '-Wextra', '-Werror', '-O1', '-g',
                        '-DCHAOS_HTTPD_TEST', '-shared', '-fPIC', '-I' + str(WEB),
                        '-I' + str(ROOT / 'supervisor/network/include'),
                        str(WEB / 'chaos_httpd.c'), str(WEB / 'websocket_codec.c'),
                        str(ROOT / 'supervisor/tests/web_httpd_harness.c'), '-o', str(binary)], check=True)
        cls.c = ctypes.CDLL(str(binary))
        for name in ['test_output', 'test_terminal_tx']:
            getattr(cls.c, name).restype = ctypes.c_void_p

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def setUp(self):
        self.c.test_reset()
        self.c.chaos_httpd_enable(1)
        self.poll()

    def poll(self, n=1):
        for _ in range(n):
            self.c.test_poll()

    def output(self):
        return ctypes.string_at(self.c.test_output(), self.c.test_output_size())

    def tx(self):
        return ctypes.string_at(self.c.test_terminal_tx(), self.c.test_terminal_tx_size())

    def inject(self, data):
        self.c.test_inject(data, len(data))
        self.poll()

    def connect(self, data, index=0):
        self.c.test_select(index)
        self.c.test_accept()
        self.inject(data)

    def exchange(self, data, index=0):
        self.connect(data, index)
        for _ in range(10000):
            if not self.c.test_open():
                break
            self.poll()
        self.assertFalse(self.c.test_open(), 'HTTP response did not finish')
        raw = self.output()
        self.assertIn(b'\r\n\r\n', raw)
        head, body = raw.split(b'\r\n\r\n', 1)
        code = int(head.split(b' ')[1])
        headers = dict(line.lower().split(b': ', 1) for line in head.split(b'\r\n')[1:])
        return code, headers, body

    def websocket(self, extra=b''):
        self.connect(request('/ws/nsh', auth=False, headers=HANDSHAKE) + extra)
        self.poll(3)
        self.assertIn(b'HTTP/1.1 101', self.output())
        self.assertIn(b's3pPLMBiTxaQ9kYGzzhZRbK+xOo=', self.output())
        self.c.test_clear_output()

    def authenticate(self):
        self.websocket()
        self.inject(frame(b'AUTH ' + AUTH))
        self.poll(3)
        self.assertEqual(self.c.test_terminal_starts(), 1)
        self.c.test_clear_output()

    def close_code(self):
        self.poll(5)
        frames = decode_frames(self.output())
        closes = [p for op, p in frames if op == 8]
        self.assertTrue(closes, frames)
        return struct.unpack('!H', closes[0][:2])[0]

    def test_default_disabled_and_password_required(self):
        self.c.test_reset()
        self.poll()
        self.assertEqual(self.c.chaos_httpd_enabled(), 0)
        self.assertEqual(self.c.test_listening(), 0)
        self.c.test_set_password(b'')
        self.c.chaos_httpd_enable(1)
        self.poll()
        self.assertEqual(self.c.test_listening(), 0)
        self.c.test_set_password(b'too-short')
        self.poll()
        self.assertEqual(self.c.test_listening(), 1)

    def test_gzip_console_and_head(self):
        code, headers, body = self.exchange(request(auth=False))
        self.assertEqual(code, 200)
        self.assertEqual(headers[b'content-encoding'], b'gzip')
        self.assertEqual(gzip.decompress(body), rendered_html())
        code, head, body2 = self.exchange(request(method='HEAD', auth=False))
        self.assertEqual(code, 200)
        self.assertEqual(int(head[b'content-length']), len(body))
        self.assertEqual(body2, b'')

    def test_authentication_and_real_optional_status(self):
        code, headers, _ = self.exchange(request('/api/system/status', auth=False))
        self.assertEqual(code, 401)
        self.assertIn(b'basic', headers[b'www-authenticate'])
        code, _, body = self.exchange(request('/api/system/status'))
        data = json.loads(body)
        self.assertEqual(code, 200)
        self.assertEqual(data['firmware'], '3.101.043')
        self.assertEqual(data['dns'], '10.23.45.67')
        self.assertEqual(data['uptime'], 3725)
        self.assertIsNone(data['cpu'])
        self.assertIsNone(data['temperature'])
        self.c.test_set_sysinfo_flags(0)
        _, _, body = self.exchange(request('/api/system/status'))
        data = json.loads(body)
        self.assertIsNone(data['heapTotal'])
        self.assertIsNone(data['uptime'])
        self.assertFalse(data['canReboot'])

    def test_invalid_header_framing(self):
        cases = [b'GET / HTTP/1.9\r\nHost: x\r\n\r\n',
                 b'GET / HTTP/1.1x\r\nHost: x\r\n\r\n',
                 b'GET / HTTP/1.1\r\n\r\n',
                 b'GET / HTTP/1.1\r\nHost: x\r\nHost: y\r\n\r\n',
                 b'GET / HTTP/1.1\r\nHost: \r\n\r\n',
                 b'GET / HTTP/1.1\r\nHost: x\x00y\r\n\r\n',
                 b'GET / HTTP/1.1\r\nHost: x\nFoo: y\r\n\r\n',
                 b'GET / HTTP/1.1\r\nHost: x\r\n Foo: y\r\n\r\n',
                 b'GET / HTTP/1.1\r\nHost : x\r\n\r\n',
                 b'GET / HTTP/1.1\r\nHost: x\r\nFoo\r\n\r\n']
        cases += [request(headers=h) for h in [b'Content-Length: 0\r\n', b'Content-Length: \r\n', b'Transfer-Encoding: chunked\r\n']]
        for data in cases:
            with self.subTest(data=data):
                self.assertEqual(self.exchange(data)[0], 400)
        for size in [b'-1', b'+1', b'1x', b'']:
            self.assertEqual(self.exchange(b'POST / HTTP/1.1\r\nHost: x\r\nContent-Length: '+size+b'\r\n\r\n')[0], 400)
        for size in [b'16385', b'9999999999999']:
            self.assertEqual(self.exchange(b'POST / HTTP/1.1\r\nHost: x\r\nContent-Length: '+size+b'\r\n\r\n')[0], 413)

    def test_origin_and_unknown_routes(self):
        self.assertEqual(self.exchange(request('/api/system/status', headers=b'Origin: http://evil.example\r\n'))[0], 403)
        self.assertEqual(self.exchange(request('/api/system/status', headers=b'Origin: null\r\n'))[0], 403)
        self.assertEqual(self.exchange(request('/not-found'))[0], 404)
        self.assertEqual(self.exchange(request(method='DELETE'))[0], 405)

    def test_fragmented_headers_and_body(self):
        data = request('/api/apps/upload', 'POST', b'<h1>hello</h1>')
        self.connect(data[:9])
        self.assertEqual(self.output(), b'')
        self.inject(data[9:-4])
        self.assertEqual(self.output(), b'')
        self.inject(data[-4:])
        self.poll(10)
        self.assertIn(b'200 OK', self.output())

    def test_limits_and_short_io(self):
        self.assertEqual(self.exchange(request(headers=b'X-Large: '+b'x'*2050+b'\r\n'))[0], 431)
        self.c.test_set_send_limit(1)
        self.c.test_set_receive_limit(1)
        self.assertEqual(self.exchange(request('/api/system/status'))[0], 200)
        self.c.test_set_send_limit(300)
        self.c.test_set_receive_limit(40000)
        self.c.test_set_finish_retries(5)
        self.assertEqual(self.exchange(request('/api/system/status'))[0], 200)

    def test_app_sandbox_and_exact_capacity(self):
        data = b'<script>alert(1)</script>' + b'x' * (16384-25)
        self.assertEqual(len(data), 16384)
        self.assertEqual(self.exchange(request('/api/apps/upload', 'POST', data))[0], 200)
        self.assertEqual(self.exchange(request('/api/apps/run', 'POST'))[0], 200)
        code, headers, body = self.exchange(request('/app/'))
        self.assertEqual(code, 200)
        self.assertEqual(body, data)
        self.assertIn(b'sandbox allow-scripts;', headers[b'content-security-policy'])
        self.assertIn(b"connect-src 'none'", headers[b'content-security-policy'])
        self.assertEqual(headers[b'connection'], b'close')
        self.assertEqual(self.exchange(request('/app/', auth=False))[0], 401)
        self.assertEqual(self.exchange(request('/api/apps/stop', 'POST'))[0], 200)
        self.assertEqual(self.exchange(request('/app/'))[0], 404)

    def test_http_command_atomic_lf_and_bounds(self):
        self.assertEqual(self.exchange(request('/api/nsh/exec', 'POST', b'ls'))[0], 200)
        self.assertEqual(self.tx(), b'ls\n')
        self.assertEqual(self.c.test_terminal_sends(), 1)
        self.assertEqual(self.exchange(request('/api/nsh/exec', 'POST', b'pwd\r'))[0], 200)
        self.assertEqual(self.tx(), b'ls\npwd\n')
        self.assertEqual(self.exchange(request('/api/nsh/exec', 'POST', b'x'*1025))[0], 413)
        self.c.test_set_terminal_capacity(self.c.test_terminal_tx_size()+2)
        before = self.tx()
        self.assertEqual(self.exchange(request('/api/nsh/exec', 'POST', b'ls'))[0], 503)
        self.assertEqual(self.tx(), before)
        self.assertEqual(self.exchange(request('/ws/nsh', auth=False, headers=HANDSHAKE))[0], 409)

    def test_http_output_and_idle_cleanup(self):
        self.exchange(request('/api/nsh/exec', 'POST', b'ls'))
        _, _, body = self.exchange(request('/api/nsh/output'))
        self.assertEqual(json.loads(body)['output'], 'nsh> ')
        self.c.test_advance(60001)
        self.poll()
        self.assertEqual(self.c.test_terminal_stops(), 1)
        self.assertEqual(self.exchange(request('/api/nsh/exec', 'POST', b'ls'))[0], 503)
        self.c.test_terminal_reap()
        self.assertEqual(self.exchange(request('/api/nsh/exec', 'POST', b'ls'))[0], 200)

    def test_websocket_auth_before_spawn_and_stream(self):
        self.websocket()
        self.assertEqual(self.c.test_terminal_starts(), 0)
        self.inject(frame(b'AUTH '+AUTH))
        self.poll(4)
        self.assertIn((2, b'nsh> '), decode_frames(self.output()))
        self.c.test_clear_output()
        self.inject(frame(b'ls\n'))
        self.assertEqual(self.tx(), b'ls\n')
        self.assertEqual(self.exchange(request('/api/nsh/exec', 'POST', b'pwd'), index=1)[0], 409)
        self.c.test_select(0)
        self.inject(frame(b'ping', op=9))
        self.assertIn((10, b'ping'), decode_frames(self.output()))

    def test_websocket_rejects_wrong_password(self):
        self.websocket()
        self.inject(frame(b'AUTH wrong'))
        self.assertEqual(self.close_code(), 1008)
        self.assertEqual(self.c.test_terminal_starts(), 0)

    def test_websocket_coalesced_auth_and_fragmented_command(self):
        self.websocket(frame(b'AUTH '+AUTH))
        self.assertEqual(self.c.test_terminal_starts(), 1)
        self.inject(frame(b'pw', fin=False)+frame(b'd\n', op=0))
        self.assertEqual(self.tx(), b'pwd\n')

    def test_websocket_fragmented_auth(self):
        self.websocket()
        self.inject(frame(b'AUTH ', fin=False))
        self.assertEqual(self.c.test_terminal_starts(), 0)
        self.inject(frame(AUTH, op=0))
        self.assertEqual(self.c.test_terminal_starts(), 1)

    def test_websocket_protocol_errors(self):
        cases = [(frame(b'AUTH '+AUTH, masked=False), 1002),
                 (frame(b'bad', op=0), 1002), (frame(b'x', op=1), 1003),
                 (frame(b'x'*1025), 1009), (frame(b'x', op=9, fin=False), 1002),
                 (frame(b'x'*126, op=9), 1002), (frame(b'x', op=8), 1002),
                 (frame(struct.pack('!H', 1005), op=8), 1002),
                 (frame(struct.pack('!H', 1000)+b'\xc0\xaf', op=8), 1007),
                 (b'\x82\xfe\x00\x01', 1002), (b'\xc2\x80', 1002)]
        for data, code in cases:
            with self.subTest(data=data):
                self.setUp()
                self.websocket()
                self.inject(data)
                self.assertEqual(self.close_code(), code)
                self.assertEqual(self.c.test_terminal_starts(), 0)

    def test_websocket_stopping_is_not_reused(self):
        for state in [1, 2, 5]:
            with self.subTest(state=state):
                self.setUp()
                self.c.test_set_terminal_state(state)
                self.websocket()
                self.inject(frame(b'AUTH '+AUTH))
                self.assertEqual(self.close_code(), 1013)
                self.assertEqual(self.c.test_terminal_starts(), 0)

    def test_websocket_close_and_backpressure(self):
        self.authenticate()
        self.c.test_set_terminal_capacity(1)
        self.inject(frame(b'ls\n'))
        self.assertEqual(self.close_code(), 1013)
        self.assertEqual(self.tx(), b'')
        self.assertEqual(self.c.test_terminal_stops(), 1)
        self.setUp()
        self.authenticate()
        self.inject(frame(struct.pack('!H', 1000)+'结束'.encode(), op=8))
        self.assertEqual(self.close_code(), 1000)
        self.assertEqual(self.c.test_terminal_stops(), 1)

    def test_link_loss_and_disable_stop_http_and_ws_sessions(self):
        for ws in [False, True]:
            for disable in [False, True]:
                with self.subTest(ws=ws, disable=disable):
                    self.setUp()
                    if ws:
                        self.authenticate()
                    else:
                        self.exchange(request('/api/nsh/exec', 'POST', b'ls'))
                    if disable:
                        self.c.chaos_httpd_enable(0)
                    else:
                        self.c.test_set_link(0)
                    self.poll()
                    self.assertEqual(self.c.test_listening(), 0)
                    self.assertEqual(self.c.test_terminal_stops(), 1)
                    self.assertEqual(self.c.test_open(), 0)

    def test_fast_off_on_reloads_password_and_kills_session(self):
        self.authenticate()
        self.c.chaos_httpd_enable(0)
        self.c.test_set_password(b'changed-password')
        self.c.chaos_httpd_enable(1)
        self.poll()
        self.assertEqual(self.c.test_terminal_stops(), 1)
        self.assertEqual(self.c.test_open(), 0)
        self.assertEqual(self.exchange(request('/api/system/status'))[0], 401)
        new_auth = base64.b64encode(b'chaos:changed-password')
        self.assertEqual(self.exchange(request('/api/system/status').replace(AUTH, new_auth))[0], 200)

    def test_auth_timeout_and_slow_request_timeout(self):
        self.websocket()
        self.c.test_advance(8001)
        self.poll()
        self.assertEqual(self.c.test_open(), 0)
        self.assertEqual(self.c.test_terminal_starts(), 0)
        self.connect(b'GET / HT')
        self.c.test_advance(8001)
        self.poll()
        self.assertEqual(self.c.test_open(), 0)

    def test_three_clients_and_refusal(self):
        for index in range(4):
            self.connect(b'GET / HT', index)
        view = View()
        self.c.chaos_httpd_snapshot(ctypes.byref(view))
        self.assertEqual(view.clients, 3)
        self.assertEqual(view.refused, 1)
        self.assertEqual(self.c.test_open(), 0)

    def test_listen_backoff(self):
        self.c.chaos_httpd_enable(0)
        self.poll()
        self.c.test_set_listen_error(-12)
        self.c.chaos_httpd_enable(1)
        self.poll()
        count = self.c.test_listen_attempts()
        self.poll(10)
        self.assertEqual(self.c.test_listen_attempts(), count)
        self.c.test_set_listen_error(0)
        self.c.test_advance(2000)
        self.poll()
        self.assertEqual(self.c.test_listening(), 1)

    def test_reboot_waits_for_terminal_reap(self):
        self.exchange(request('/api/nsh/exec', 'POST', b'ls'))
        self.assertEqual(self.exchange(request('/api/system/reboot', 'POST'))[0], 200)
        self.c.test_advance(1600)
        self.poll()
        self.assertEqual(self.c.test_reboots(), 0)
        self.c.test_terminal_reap()
        self.poll()
        self.assertEqual(self.c.test_reboots(), 1)
        self.c.test_set_sysinfo_flags(0)
        self.assertEqual(self.exchange(request('/api/system/reboot', 'POST'))[0], 501)

    def test_embedded_frontend(self):
        self.assertEqual((WEB / 'web_index.h').read_text(encoding='ascii'), generated_header())
        html = rendered_html().decode()
        self.assertNotIn('SYSPATCH', html)
        self.assertNotIn('q66', html)
        self.assertNotIn('Material Symbols', html)
        self.assertNotIn('page-audio', html)
        self.assertNotRegex(html, r'<(?:script|link)[^>]+(?:src|href)=')
        self.assertIn('sandbox="allow-scripts"', html)
        self.assertIn("connect-src 'none'", html)
        ids = re.findall(r'\bid="([^"]+)"', html)
        self.assertEqual(len(ids), len(set(ids)), 'duplicate DOM ids')
        js = (WEB / 'console.js').read_text()
        for identifier in re.findall(r"\$\('([^']+)'\)", js):
            self.assertIn(identifier, ids)
        node = shutil.which('node')
        if node:
            subprocess.run([node, '--check', str(WEB / 'console.js')], check=True)

    def test_monotonic_clock_wrap_and_long_uptime(self):
        self.c.test_reset()
        self.c.test_set_now(0x80000010)
        self.c.chaos_httpd_enable(1)
        self.poll()
        self.assertEqual(self.c.test_listening(), 1)
        self.c.test_set_now(0x100000000-60000)
        self.assertEqual(self.exchange(request('/api/nsh/exec', 'POST', b'ls'))[0], 200)
        self.assertEqual(self.exchange(request('/ws/nsh', auth=False, headers=HANDSHAKE))[0], 409)
        self.c.test_advance(60000)
        self.poll()
        self.assertEqual(self.c.test_terminal_stops(), 1)

    def test_failed_first_command_reclaims_terminal(self):
        self.c.test_set_terminal_capacity(2)
        self.assertEqual(self.exchange(request('/api/nsh/exec', 'POST', b'ls'))[0], 503)
        self.assertEqual(self.tx(), b'')
        self.assertEqual(self.c.test_terminal_stops(), 1)
        self.setUp()
        for command in [b'ls\x00', b'\x04']:
            self.assertEqual(self.exchange(request('/api/nsh/exec', 'POST', command))[0], 400)
        self.assertEqual(self.c.test_terminal_starts(), 0)

    def test_output_probe_does_not_own_or_drain_a_session(self):
        self.assertEqual(self.exchange(request('/api/nsh/output'))[0], 200)
        self.authenticate()
        self.setUp()
        self.exchange(request('/api/nsh/exec', 'POST', b'ls'))
        self.assertEqual(self.exchange(request('/api/nsh/output', 'HEAD'))[2], b'')
        data = json.loads(self.exchange(request('/api/nsh/output'))[2])
        self.assertEqual(data['output'], 'nsh> ')

    def test_meminfo_escaping_preserves_json_suffix(self):
        self.c.test_set_escaped_meminfo(1)
        data = json.loads(self.exchange(request('/api/system/status'))[2])
        self.assertTrue(data['meminfo'])
        self.assertEqual(set(data['meminfo']), {'\x01'})
        self.assertIsNone(data['storage'])

    def test_reboot_does_not_preempt_its_pending_reply(self):
        self.c.test_set_send_limit(0)
        self.connect(request('/api/system/reboot', 'POST'))
        self.c.test_advance(2000)
        self.poll()
        self.assertEqual(self.c.test_reboots(), 0)
        self.c.test_set_send_limit(300)
        self.poll(10)
        self.assertIn(b'200 OK', self.output())
        self.assertEqual(self.c.test_reboots(), 0)
        self.c.test_advance(1500)
        self.poll()
        self.assertEqual(self.c.test_reboots(), 1)


if __name__ == '__main__':
    unittest.main(verbosity=2)
