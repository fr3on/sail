//! The tool row: firmware setup, restart, power off.

use alloc::string::String;
use alloc::vec::Vec;
use uefi::cstr16;
use uefi::prelude::*;
use uefi::runtime::{self, ResetType, VariableAttributes, VariableVendor};

#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Firmware,
    Restart,
    PowerOff,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Firmware => "Firmware setup",
            Tool::Restart => "Restart",
            Tool::PowerOff => "Power off",
        }
    }

    pub fn icon_index(self) -> usize {
        self as usize
    }
}

const OS_INDICATIONS: &uefi::CStr16 = cstr16!("OsIndications");
const BOOT_TO_FW_UI: u8 = 1;

fn firmware_setup_supported() -> bool {
    runtime::get_variable_boxed(cstr16!("OsIndicationsSupported"), &VariableVendor::GLOBAL_VARIABLE)
        .map(|(d, _)| d.first().is_some_and(|b| b & BOOT_TO_FW_UI != 0))
        .unwrap_or(false)
}

pub fn available() -> Vec<Tool> {
    let mut v = Vec::new();
    if firmware_setup_supported() {
        v.push(Tool::Firmware);
    }
    v.push(Tool::Restart);
    v.push(Tool::PowerOff);
    v
}

/// Runs a tool. Restart and power off never return; the string is an error message.
pub fn run(tool: Tool) -> String {
    match tool {
        Tool::Firmware => {
            if !firmware_setup_supported() {
                return String::from("This firmware cannot open its setup from software.");
            }
            let vendor = VariableVendor::GLOBAL_VARIABLE;
            let mut cur = [0u8; 8];
            if let Ok((data, _)) = runtime::get_variable_boxed(OS_INDICATIONS, &vendor) {
                for (d, s) in cur.iter_mut().zip(data.iter()) {
                    *d = *s;
                }
            }
            cur[0] |= BOOT_TO_FW_UI;
            let attrs = VariableAttributes::NON_VOLATILE
                | VariableAttributes::BOOTSERVICE_ACCESS
                | VariableAttributes::RUNTIME_ACCESS;
            if let Err(e) = runtime::set_variable(OS_INDICATIONS, &vendor, attrs, &cur) {
                return alloc::format!("Could not ask the firmware to open its setup: {:?}", e.status());
            }
            runtime::reset(ResetType::COLD, Status::SUCCESS, None)
        }
        Tool::Restart => runtime::reset(ResetType::COLD, Status::SUCCESS, None),
        Tool::PowerOff => runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None),
    }
}
