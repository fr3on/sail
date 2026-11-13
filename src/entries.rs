//! Finding the installed systems and chainloading one.
//!
//! Security model: Sail only ever starts files from the fixed allow-list below,
//! or the standard removable-media fallback path. `load_image` goes through the
//! firmware's own verification, so with Secure Boot on the firmware (or shim)
//! rejects anything it does not trust. Sail never reads, parses or edits another
//! loader's files.

use alloc::string::String;
use alloc::vec::Vec;
use sail::id::fnv1a;
use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol, SearchType};
use uefi::proto::ProtocolPointer;
use uefi::proto::device_path::DevicePath;
use uefi::proto::device_path::build::{DevicePathBuilder, media::FilePath};
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::{CStr16, Identify, cstr16};

pub const MAX_TILES: usize = 32;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Fedora,
    Windows,
    Ubuntu,
    Debian,
    Arch,
    Suse,
    Mint,
    /// A distribution or loader without a bundled logo; drawn as a letter badge.
    Generic(char),
    Other,
}

struct Known {
    kind: Kind,
    name: &'static str,
    note: &'static str,
    path: &'static CStr16,
}

macro_rules! known {
    ($kind:expr, $name:expr, $note:expr, $path:expr) => {
        Known {
            kind: $kind,
            name: $name,
            note: $note,
            path: cstr16!($path),
        }
    };
}

const KNOWN: &[Known] = &[
    known!(Kind::Fedora, "Fedora", "Linux", "\\EFI\\fedora\\shimx64.efi"),
    known!(Kind::Ubuntu, "Ubuntu", "Linux", "\\EFI\\ubuntu\\shimx64.efi"),
    known!(Kind::Debian, "Debian", "Linux", "\\EFI\\debian\\shimx64.efi"),
    known!(Kind::Arch, "Arch Linux", "Linux", "\\EFI\\arch\\grubx64.efi"),
    known!(Kind::Suse, "openSUSE", "Linux", "\\EFI\\opensuse\\grubx64.efi"),
    known!(Kind::Mint, "Linux Mint", "Linux", "\\EFI\\linuxmint\\shimx64.efi"),
    known!(Kind::Generic('R'), "Rocky Linux", "Linux", "\\EFI\\rocky\\shimx64.efi"),
    known!(
        Kind::Generic('A'),
        "AlmaLinux",
        "Linux",
        "\\EFI\\almalinux\\shimx64.efi"
    ),
    known!(Kind::Generic('M'), "Manjaro", "Linux", "\\EFI\\Manjaro\\grubx64.efi"),
    known!(
        Kind::Generic('S'),
        "systemd-boot",
        "Boot loader",
        "\\EFI\\systemd\\systemd-bootx64.efi"
    ),
    known!(
        Kind::Generic('r'),
        "rEFInd",
        "Boot loader",
        "\\EFI\\refind\\refind_x64.efi"
    ),
    known!(Kind::Generic('G'), "GRUB", "Boot loader", "\\EFI\\GRUB\\grubx64.efi"),
    known!(
        Kind::Windows,
        "Windows",
        "Windows",
        "\\EFI\\Microsoft\\Boot\\bootmgfw.efi"
    ),
];

const FALLBACK: &CStr16 = cstr16!("\\EFI\\BOOT\\BOOTX64.EFI");

pub struct Entry {
    pub kind: Kind,
    pub name: &'static str,
    pub note: &'static str,
    pub path: &'static CStr16,
    pub device: uefi::Handle,
    /// Stable across boots: hash of the partition's device path and the entry name.
    pub id: u64,
    /// Never started by the countdown: anything found only through the generic
    /// removable-media path (a USB stick someone plugged in) needs a deliberate Enter.
    pub manual_only: bool,
}

/// Open a protocol without taking it exclusively, so other loaders can still use it.
pub fn get_protocol<P: ProtocolPointer + ?Sized>(handle: uefi::Handle) -> Option<ScopedProtocol<P>> {
    // SAFETY: GetProtocol only reads the interface pointer; it takes no ownership of the
    // handle's protocol and disconnects nothing.
    unsafe {
        boot::open_protocol::<P>(
            OpenProtocolParams {
                handle,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
        .ok()
    }
}

pub fn discover() -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let me = boot::image_handle();
    let own_device = boot::open_protocol_exclusive::<LoadedImage>(me)
        .ok()
        .and_then(|li| li.device());
    let Ok(handles) = boot::locate_handle_buffer(SearchType::ByProtocol(&SimpleFileSystem::GUID)) else {
        return out;
    };
    for &h in handles.iter() {
        if out.len() >= MAX_TILES {
            break;
        }
        // Without a device path the entry could not be started anyway.
        let Some(dp) = get_protocol::<DevicePath>(h) else {
            continue;
        };
        let disk = dp.as_bytes().to_vec();
        drop(dp);
        let Some(proto) = get_protocol::<SimpleFileSystem>(h) else {
            continue;
        };
        let mut fs = uefi::fs::FileSystem::new(proto);
        let mut push =
            |kind: Kind, name: &'static str, note: &'static str, path: &'static CStr16, manual_only: bool| {
                let id = fnv1a(&[&disk, name.as_bytes()]);
                if out.len() < MAX_TILES && !out.iter().any(|e| e.id == id) {
                    out.push(Entry {
                        kind,
                        name,
                        note,
                        path,
                        device: h,
                        id,
                        manual_only,
                    });
                }
            };
        for k in KNOWN {
            if fs.try_exists(uefi::fs::Path::new(k.path)).unwrap_or(false) {
                push(k.kind, k.name, k.note, k.path, false);
            }
        }
        if Some(h) != own_device && fs.try_exists(uefi::fs::Path::new(FALLBACK)).unwrap_or(false) {
            push(Kind::Other, "Other disk", "USB or drive", FALLBACK, true);
        }
    }
    // Linux and loaders first, Windows next, unknown media last. The sort is stable.
    out.sort_by_key(|e| match e.kind {
        Kind::Other => 2,
        Kind::Windows => 1,
        _ => 0,
    });
    out
}

/// Load `entry`'s image. The firmware verifies it here (Secure Boot) and refuses untrusted files.
pub fn load(entry: &Entry) -> Result<uefi::Handle, String> {
    let me = boot::image_handle();
    let dp = get_protocol::<DevicePath>(entry.device).ok_or_else(|| String::from("no device path"))?;
    let mut storage = Vec::new();
    let mut b = DevicePathBuilder::with_vec(&mut storage);
    for node in dp.node_iter() {
        b = b.push(&node).map_err(|_| String::from("path"))?;
    }
    let full = b
        .push(&FilePath { path_name: entry.path })
        .and_then(|b| b.finalize())
        .map_err(|_| String::from("path"))?;
    boot::load_image(
        me,
        boot::LoadImageSource::FromDevicePath {
            device_path: full,
            boot_policy: uefi::proto::BootPolicy::ExactMatch,
        },
    )
    .map_err(|e| alloc::format!("{:?}", e.status()))
}

/// Start a loaded image. Returns only if it failed or came back to the menu.
pub fn run(image: uefi::Handle) -> Result<(), String> {
    let result = boot::start_image(image).map_err(|e| alloc::format!("{:?}", e.status()));
    if result.is_ok() {
        // It returned successfully, so it is still loaded: release it.
        // (After an error return the firmware has already unloaded it.)
        let _ = boot::unload_image(image);
    }
    result
}
