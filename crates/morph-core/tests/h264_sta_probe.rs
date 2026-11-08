// The GUI thread is a COM STA (drag-and-drop); the H.264 probe must still find
// Media Foundation from there.
#![cfg(all(windows, feature = "video"))]

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut core::ffi::c_void, mode: u32) -> i32;
}

#[test]
fn h264_probe_works_from_sta_thread() {
    unsafe { CoInitializeEx(std::ptr::null_mut(), 0x2) }; // COINIT_APARTMENTTHREADED
    assert!(morph_core::video::h264_available());
}
