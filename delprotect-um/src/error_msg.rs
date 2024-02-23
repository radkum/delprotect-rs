use std::{os::raw::c_void, ptr::null_mut};
use windows::core::imp::{
    FormatMessageW, GetLastError, FORMAT_MESSAGE_ALLOCATE_BUFFER, FORMAT_MESSAGE_FROM_SYSTEM,
    FORMAT_MESSAGE_IGNORE_INSERTS,
};
use windows_sys::Win32::Foundation::LocalFree;

pub(crate) fn print_last_error(msg: &str) {
    let error_code = unsafe { GetLastError() };
    let error_msg = get_error_as_string(error_code).unwrap_or("Failed to get msg".to_string());

    println!(
        "{msg}, ErrorCode: 0x{:08x}, ErrorMsg: \"{}\"",
        error_code,
        error_msg.trim_end()
    );
}

pub(crate) fn get_error_as_string(error_msg_id: u32) -> Option<String> {
    unsafe {
        let mut message_buffer = null_mut();
        let chars = FormatMessageW(
            FORMAT_MESSAGE_ALLOCATE_BUFFER
                | FORMAT_MESSAGE_FROM_SYSTEM
                | FORMAT_MESSAGE_IGNORE_INSERTS,
            null_mut(),
            error_msg_id,
            0,
            &mut message_buffer as *mut *mut u16 as *mut u16,
            0,
            null_mut(),
        );

        let msg = if chars > 0 {
            let parts = std::slice::from_raw_parts(message_buffer, chars as _);
            String::from_utf16(parts).ok()
        } else {
            None
        };

        LocalFree(message_buffer as *mut c_void);

        msg
    }
}

#[allow(dead_code)]
fn get_last_error_as_string() -> Option<String> {
    unsafe {
        //Get the error message, if any.
        let error_msg_id = GetLastError();
        if error_msg_id == 0 {
            return Some(String::from("STATUS_SUCCESS"));
        }

        let mut message_buffer = null_mut();
        let chars = FormatMessageW(
            FORMAT_MESSAGE_ALLOCATE_BUFFER
                | FORMAT_MESSAGE_FROM_SYSTEM
                | FORMAT_MESSAGE_IGNORE_INSERTS,
            null_mut(),
            error_msg_id,
            0,
            &mut message_buffer as *mut *mut u16 as *mut u16,
            0,
            null_mut(),
        );

        let msg = if chars > 0 {
            let parts = std::slice::from_raw_parts(message_buffer, chars as _);
            String::from_utf16(parts).ok()
        } else {
            None
        };

        LocalFree(message_buffer as *mut c_void);

        msg
    }
}
