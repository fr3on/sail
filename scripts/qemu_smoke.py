#!/usr/bin/env python3
"""Boot sail.efi in QEMU + OVMF with stand-in entries and check the menu is drawn.

Usage: scripts/qemu_smoke.py [path/to/sail.efi] [--shot out.ppm]
Env:   OVMF=/path/to/OVMF_CODE.fd to override firmware detection.

Passes when the screenshot shows the accent-blue selection border and light title text,
which only happens if the image booted, found entries, and painted the menu.
"""
import os, shutil, socket, subprocess, sys, tempfile, time

ACCENT = (0x3B, 0x82, 0xF6)
FIRMWARE = [
    "/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_CODE.fd", "/usr/share/edk2/ovmf/OVMF_CODE.fd",
    "/usr/share/qemu/edk2-x86_64-code.fd", "/opt/homebrew/share/qemu/edk2-x86_64-code.fd",
]
STAND_INS = ["fedora/shimx64.efi", "ubuntu/shimx64.efi", "debian/shimx64.efi",
             "Microsoft/Boot/bootmgfw.efi", "systemd/systemd-bootx64.efi"]


def find_firmware():
    cands = [os.environ["OVMF"]] if os.environ.get("OVMF") else FIRMWARE
    for p in cands:
        p = os.path.realpath(p)
        if os.path.exists(p):
            return p
    sys.exit("no OVMF firmware found; set OVMF=/path/to/OVMF_CODE.fd")


def read_ppm(path):
    data = open(path, "rb").read()
    parts, i = [], 0
    while len(parts) < 4:  # magic, width, height, maxval
        while data[i:i + 1].isspace():
            i += 1
        j = i
        while not data[j:j + 1].isspace():
            j += 1
        parts.append(data[i:j]); i = j
    i += 1
    assert parts[0] == b"P6", "unexpected screendump format"
    return int(parts[1]), int(parts[2]), data[i:]


def judge(path):
    w, h, px = read_ppm(path)
    accent = light = 0
    for k in range(0, len(px) - 2, 3):
        r, g, b = px[k], px[k + 1], px[k + 2]
        if abs(r - ACCENT[0]) < 24 and abs(g - ACCENT[1]) < 24 and abs(b - ACCENT[2]) < 24:
            accent += 1
        elif r > 200 and g > 200 and b > 200:
            light += 1
    return w, h, accent, light


def monitor(sock_path, cmd):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(5)
    s.connect(sock_path)
    s.recv(4096)
    s.sendall(cmd.encode() + b"\n")
    time.sleep(1.5)
    s.close()


def main():
    args = sys.argv[1:]
    shot_out = None
    if "--shot" in args:
        k = args.index("--shot"); shot_out = args[k + 1]; del args[k:k + 2]
    efi = os.path.abspath(args[0] if args else "target/x86_64-unknown-uefi/release/sail.efi")
    if not os.path.exists(efi):
        sys.exit(f"{efi} not found; run cargo build --release first")
    fw = find_firmware()
    # Short path on purpose: unix socket paths are limited to ~104 bytes.
    work = tempfile.mkdtemp(prefix="sail-smoke.", dir="/tmp")
    esp = os.path.join(work, "esp")
    os.makedirs(os.path.join(esp, "EFI", "BOOT"))
    shutil.copy(efi, os.path.join(esp, "EFI", "BOOT", "BOOTX64.EFI"))
    for rel in STAND_INS:
        p = os.path.join(esp, "EFI", *rel.split("/"))
        os.makedirs(os.path.dirname(p), exist_ok=True)
        open(p, "w").write("stand-in\n")
    qemu = subprocess.Popen(
        ["qemu-system-x86_64", "-machine", "q35", "-m", "512",
         "-drive", f"if=pflash,format=raw,readonly=on,file={fw}",
         "-drive", f"file=fat:rw:{esp},format=raw", "-vga", "std", "-display", "none",
         "-monitor", f"unix:{work}/mon.sock,server,nowait", "-serial", "none"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    shot = os.path.join(work, "shot.ppm")
    ok, last = False, None
    try:
        deadline = time.time() + int(os.environ.get("SMOKE_TIMEOUT", "180"))
        while time.time() < deadline and qemu.poll() is None:
            time.sleep(8)
            try:
                if os.path.exists(shot):
                    os.remove(shot)
                monitor(f"{work}/mon.sock", f"screendump {shot}")
                last = judge(shot)
            except (OSError, AssertionError):
                continue
            if last[2] > 500 and last[3] > 1000:
                ok = True
                break
    finally:
        qemu.kill()
        if shot_out and os.path.exists(shot):
            shutil.copy(shot, shot_out)
        shutil.rmtree(work, ignore_errors=True)
    if ok:
        print(f"PASS: menu drawn ({last[0]}x{last[1]}, accent={last[2]}, light={last[3]})")
    else:
        sys.exit(f"FAIL: menu not drawn (last check: {last})")


main()
