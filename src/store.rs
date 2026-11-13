//! NVRAM: the "last used" entry and the Secure Boot state.

use crate::entries::Entry;
use uefi::runtime::{self, VariableAttributes, VariableVendor};
use uefi::{cstr16, guid};

const SAIL_VENDOR: VariableVendor = VariableVendor(guid!("5a11e000-0000-4000-8000-000000000001"));

/// A new name: the old `SailLast` held a display name and was written with runtime access,
/// and firmware refuses to rewrite a variable with different attributes.
const LAST: &uefi::CStr16 = cstr16!("SailLastId");

/// Boot-time only: the running OS cannot read or write it through the normal path.
fn private_attrs() -> VariableAttributes {
    VariableAttributes::NON_VOLATILE | VariableAttributes::BOOTSERVICE_ACCESS
}

fn read_id() -> Option<u64> {
    let (data, _) = runtime::get_variable_boxed(LAST, &SAIL_VENDOR).ok()?;
    // Validate the stored value instead of trusting it.
    let bytes: [u8; 8] = data.as_ref().try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

pub fn load_last(items: &[Entry]) -> Option<usize> {
    let id = read_id()?;
    items.iter().position(|e| e.id == id)
}

/// Writes only when the value changes, which spares the flash.
pub fn save_last(id: u64) {
    if read_id() == Some(id) {
        return;
    }
    let _ = runtime::set_variable(LAST, &SAIL_VENDOR, private_attrs(), &id.to_le_bytes());
}

/// `Some(true)` when the firmware reports Secure Boot enabled, `Some(false)` when it is off,
/// `None` when the variable is missing (a firmware without Secure Boot).
pub fn secure_boot() -> Option<bool> {
    let (d, _) = runtime::get_variable_boxed(cstr16!("SecureBoot"), &VariableVendor::GLOBAL_VARIABLE).ok()?;
    d.first().map(|b| *b == 1)
}
