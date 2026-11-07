# Sail

A graphical UEFI boot menu with big tiles, written in Rust. It finds the operating systems on your machine, shows them as tiles with their logos, and starts the one you pick. It works with Secure Boot on (through shim).

> **Experimental.** Sail is a boot loader. A mistake can leave a machine that does not boot. It was tested on one machine (a Lenovo ThinkStation P620 with Fedora and Windows 11). Read this page fully, keep a recovery USB, and do not use it on a machine you cannot fix.

## What it does

- Scans every disk's EFI partition and lists what it knows: Fedora, Windows, Ubuntu, Debian, Arch, openSUSE, Linux Mint, and any other disk with `\EFI\BOOT\BOOTX64.EFI`.
- Scrolling row of four tiles, arrow keys or number keys, Enter to start, 8 second countdown on the first tile.
- Starts the choice by chainloading its EFI file. It does not replace or edit any other boot loader.
- Runs at up to 1920x1200, even on a 4K screen, so it stays fast.

It does not read GRUB menus, pass kernel options, or manage boot entries. Linux distributions are started through their own shim and GRUB.

## Build

You need Rust with the UEFI target:

```bash
rustup target add x86_64-unknown-uefi
cargo build --release
```

The result is `target/x86_64-unknown-uefi/release/sail.efi`.

## Install on a real machine

Use a one-time boot first, and keep your normal boot entry first in the boot order.

**Secure Boot off:**

1. Copy `sail.efi` to `<EFI partition>/EFI/sail/sailx64.efi`.
2. Add an entry: `sudo efibootmgr --create --disk /dev/nvme0n1 --part 1 --label Sail --loader '\EFI\sail\sailx64.efi'` (change the disk and partition to your EFI partition).
3. Boot it once with `sudo efibootmgr --bootnext <number>` and reboot.

**Secure Boot on (Fedora and other shim systems):**

Firmware does not trust Sail's signature, but shim can trust a key you enroll (MOK):

1. Make a signing key and sign Sail: `openssl req -new -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes -subj "/CN=Sail/" -keyout sail.key -out sail.crt`, then `sbsign --key sail.key --cert sail.crt --output grubx64.efi sail.efi`. Sail already carries a `.sbat` section, which shim requires.
2. Enroll the key: `openssl x509 -in sail.crt -outform DER -out sail.der`, `sudo mokutil --import sail.der`, reboot, and finish the blue MOK screen (Enroll MOK, Continue, Yes, password).
3. Copy your distribution's `shimx64.efi` and `mmx64.efi` to `EFI/sail/`, and the signed file there as `grubx64.efi` (shim starts the file with that name).
4. Add an entry that points at `\EFI\sail\shimx64.efi`, at the end of the boot order.

Keep `sail.key` private and out of git; whoever has it can sign code that your machine will trust.

## Recovery

- The firmware boot menu (often F12) starts any entry directly, so your other systems still boot if Sail fails.
- To remove Sail: `sudo efibootmgr -B -b <number>` and delete `EFI/sail` on the EFI partition.

## Layout

| Path | What |
|---|---|
| `src/main.rs` | the whole boot menu |
| `assets/` | DM Sans font, logo masks (see `NOTICE.md`) |

## License

MIT for Sail's code (`LICENSE`). The font and logos have their own terms: see `NOTICE.md`. Distribution logos are trademarks of their owners.
