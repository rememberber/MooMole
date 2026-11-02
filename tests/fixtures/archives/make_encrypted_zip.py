"""Deterministic, inert WinZip AE-2 fixtures using Python + Apple's CommonCrypto.

This test generator is independent of the Rust zip reader/writer. Its public
password and deterministic salt are test data, never an encryption utility.
"""
import ctypes
import hashlib
import hmac
from pathlib import Path
import struct
import sys
import zlib

PASSWORD = "MooMole owned secret 8fd1 密码"


def aes_block(key, block):
    lib = ctypes.CDLL("/usr/lib/system/libcommonCrypto.dylib")
    crypt = lib.CCCrypt
    crypt.argtypes = [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint,
                      ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p,
                      ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p,
                      ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
    crypt.restype = ctypes.c_int
    result = ctypes.create_string_buffer(16)
    written = ctypes.c_size_t()
    assert crypt(0, 0, 2, key, len(key), None, block, len(block),
                 result, 16, ctypes.byref(written)) == 0
    assert written.value == 16
    return result.raw


def create(path):
    local, central = bytearray(), bytearray()
    entries = [("Trip/秘密.txt", "owned 独立 AES payload".encode(), True),
               ("Trip/empty.txt", b"", True),
               ("Trip/public.txt", b"owned unencrypted companion", False)]
    for index, (name, body, encrypted) in enumerate(entries):
        name = name.encode()
        extra = b""
        method, needed, crc = 0, 20, zlib.crc32(body)
        payload = body
        if encrypted:
            method, needed, crc = 99, 51, 0
            extra = struct.pack("<HHH2sBH", 0x9901, 7, 2, b"AE", 3, 8)
            salt = bytes(range(index, index + 16))
            keys = hashlib.pbkdf2_hmac("sha1", PASSWORD.encode(), salt, 1000, 66)
            compressor = zlib.compressobj(wbits=-15)
            compressed = compressor.compress(body) + compressor.flush()
            cipher = bytearray()
            for start in range(0, len(compressed), 16):
                counter = (start // 16 + 1).to_bytes(16, "little")
                stream = aes_block(keys[:32], counter)
                cipher.extend(a ^ b for a, b in zip(compressed[start:start + 16], stream))
            tag = hmac.new(keys[32:64], cipher, hashlib.sha1).digest()[:10]
            payload = salt + keys[64:] + cipher + tag
        flags = 0x800 | int(encrypted)
        offset = len(local)
        local.extend(struct.pack("<4s5H3I2H", b"PK\x03\x04", needed, flags,
                                 method, 0, 33, crc, len(payload), len(body), len(name), len(extra)))
        local.extend(name + extra + payload)
        central.extend(struct.pack("<4s6H3I5H2I", b"PK\x01\x02", (3 << 8) | needed,
                                   needed, flags, method, 0, 33, crc, len(payload), len(body),
                                   len(name), len(extra), 0, 0, 0, 0o100600 << 16, offset))
        central.extend(name + extra)
    end = struct.pack("<4s4H2IH", b"PK\x05\x06", 0, 0, len(entries), len(entries),
                      len(central), len(local), 0)
    Path(path).write_bytes(local + central + end)


if __name__ == "__main__":
    create(sys.argv[1])
