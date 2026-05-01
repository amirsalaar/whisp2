//! CoreAudio input volume boost/restore for microphone recording sessions.
//! Uses AudioHardwareService property to get/set the default input device volume.

use std::mem;

/// CoreAudio property address for virtual master volume on input scope.
/// kAudioHardwareServiceDeviceProperty_VirtualMasterVolume = 'vmvc'
const VIRTUAL_MASTER_VOLUME: u32 = 0x766D7663;
/// kAudioObjectPropertyScopeInput = 'inpu'
const SCOPE_INPUT: u32 = 0x696E7075;
/// kAudioObjectPropertyElementMain = 0
const ELEMENT_MAIN: u32 = 0;
/// kAudioObjectSystemObject = 1
const SYSTEM_OBJECT: u32 = 1;
/// kAudioHardwarePropertyDefaultInputDevice = 'dIn '
const DEFAULT_INPUT_DEVICE: u32 = 0x6449_6E20;

#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyData(
        object_id: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const std::ffi::c_void,
        data_size: *mut u32,
        data: *mut std::ffi::c_void,
    ) -> i32;
}

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectSetPropertyData(
        object_id: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const std::ffi::c_void,
        data_size: u32,
        data: *const std::ffi::c_void,
    ) -> i32;
}

fn get_default_input_device() -> Option<u32> {
    let addr = AudioObjectPropertyAddress {
        selector: DEFAULT_INPUT_DEVICE,
        scope: 0x676C_6F62, // kAudioObjectPropertyScopeGlobal = 'glob'
        element: ELEMENT_MAIN,
    };
    let mut device_id: u32 = 0;
    let mut data_size = mem::size_of::<u32>() as u32;
    let result = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &addr,
            0,
            std::ptr::null(),
            &mut data_size,
            &mut device_id as *mut u32 as *mut std::ffi::c_void,
        )
    };
    if result == 0 { Some(device_id) } else { None }
}

fn get_volume(device_id: u32) -> Option<f32> {
    let addr = AudioObjectPropertyAddress {
        selector: VIRTUAL_MASTER_VOLUME,
        scope: SCOPE_INPUT,
        element: ELEMENT_MAIN,
    };
    let mut volume: f32 = 0.0;
    let mut data_size = mem::size_of::<f32>() as u32;
    let result = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &addr,
            0,
            std::ptr::null(),
            &mut data_size,
            &mut volume as *mut f32 as *mut std::ffi::c_void,
        )
    };
    if result == 0 { Some(volume) } else { None }
}

fn set_volume(device_id: u32, volume: f32) -> bool {
    let addr = AudioObjectPropertyAddress {
        selector: VIRTUAL_MASTER_VOLUME,
        scope: SCOPE_INPUT,
        element: ELEMENT_MAIN,
    };
    let result = unsafe {
        AudioObjectSetPropertyData(
            device_id,
            &addr,
            0,
            std::ptr::null(),
            mem::size_of::<f32>() as u32,
            &volume as *const f32 as *const std::ffi::c_void,
        )
    };
    result == 0
}

/// Boosts input volume by 1.5x (capped at 1.0). Returns the saved original volume.
/// Returns None if the device/property is not accessible.
pub fn boost() -> Option<f32> {
    let device_id = get_default_input_device()?;
    let current = get_volume(device_id)?;
    let boosted = (current * 1.5).min(1.0);
    if set_volume(device_id, boosted) {
        Some(current)
    } else {
        None
    }
}

/// Restores input volume to a previously saved value.
pub fn restore(saved: f32) {
    if let Some(device_id) = get_default_input_device() {
        set_volume(device_id, saved);
    }
}
