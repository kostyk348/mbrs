#!/usr/bin/env python3
"""Minimal Modbus/TCP slave for demoing mbrs (no dependencies).

Usage:  python3 examples/mock_slave.py [port]     (default 1502)

Serves unit 1: holding/input registers 0..63 sweep a sine 0..4095 so the
32-colour ramp is visible; coils toggle. Supports FC 01,02,03,04,05,06,16.
"""
import socket, struct, sys, threading, time, math

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 1502
N = 64
regs = [0] * N
coils = [False] * N
lock = threading.Lock()
t0 = time.time()


def update():
    while True:
        t = time.time() - t0
        with lock:
            for i in range(N):
                regs[i] = int((math.sin(t * 0.5 + i * 0.3) * 0.5 + 0.5) * 4095)
                coils[i] = (int(t * 2) + i) % 3 == 0
        time.sleep(0.04)


def handle(conn):
    try:
        while True:
            hdr = conn.recv(7)
            if len(hdr) < 7:
                return
            tid, _pid, length, uid = struct.unpack(">HHHB", hdr)
            need = length - 1
            pdu = b""
            while len(pdu) < need:
                chunk = conn.recv(need - len(pdu))
                if not chunk:
                    return
                pdu += chunk
            fc = pdu[0]
            with lock:
                if 1 <= fc <= 4:
                    addr, qty = struct.unpack(">HH", pdu[1:5])
                    if fc <= 2:
                        data = bytearray((qty + 7) // 8)
                        for k, b in enumerate(coils[addr:addr + qty]):
                            if b:
                                data[k // 8] |= 1 << (k % 8)
                        resp = bytes([fc, len(data)]) + bytes(data)
                    else:
                        vals = regs[addr:addr + qty]
                        resp = bytes([fc, len(vals) * 2]) + b"".join(struct.pack(">H", v) for v in vals)
                elif fc == 6:
                    addr, val = struct.unpack(">HH", pdu[1:5])
                    regs[addr] = val
                    resp = pdu[:5]
                elif fc == 5:
                    addr, val = struct.unpack(">HH", pdu[1:5])
                    coils[addr] = val == 0xFF00
                    resp = pdu[:5]
                elif fc == 16:
                    addr, qty, _bc = struct.unpack(">HHB", pdu[1:6])
                    for k in range(qty):
                        regs[addr + k] = struct.unpack(">H", pdu[6 + 2 * k:8 + 2 * k])[0]
                    resp = struct.pack(">BHH", fc, addr, qty)
                else:
                    resp = bytes([fc | 0x80, 0x01])
            conn.sendall(struct.pack(">HHHB", tid, 0, len(resp) + 1, uid) + resp)
    except Exception:
        pass
    finally:
        conn.close()


threading.Thread(target=update, daemon=True).start()
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", PORT))
s.listen(8)
print(f"mock Modbus/TCP slave on 127.0.0.1:{PORT}", flush=True)
while True:
    c, _ = s.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
