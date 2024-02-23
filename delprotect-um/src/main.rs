mod error_msg;

use crate::error_msg::print_last_error;

use common::ioctl_codes;
use std::{env, ffi::c_void, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{CloseHandle, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{CreateFileA, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    System::IO::DeviceIoControl,
};

fn main() {
    let args: Vec<String> = env::args().collect();
    //println!("{args:?}");
    if args.len() < 2 {
        print_usage();
        return;
    }

    let h_device = unsafe {
        CreateFileA(
            "\\\\.\\DelProtect\0".as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            0,
            0isize,
        ) as HANDLE
    };

    if h_device == INVALID_HANDLE_VALUE {
        print_last_error("Failed to open file");
        return;
    }
    println!("CreateFile success!");

    let status = match args[1].as_str() {
        "add" => {
            if args.len() == 3 {
                let name = args[2].clone() + "\0";
                let mut returned: u32 = 0;
                unsafe {
                    DeviceIoControl(
                        h_device,
                        ioctl_codes::IOCTL_DELPROTECT_ADD_EXE_UTF8,
                        name.as_ptr() as *const c_void,
                        name.len() as u32,
                        null_mut(),
                        0,
                        &mut returned as *mut u32,
                        null_mut(),
                    )
                }
            } else {
                print_usage();
                0
            }
        },
        "del" => {
            if args.len() == 3 {
                let name = args[2].clone() + "\0";
                let mut returned: u32 = 0;
                unsafe {
                    DeviceIoControl(
                        h_device,
                        ioctl_codes::IOCTL_DELPROTECT_REMOVE_EXE_UTF8,
                        name.as_ptr() as *const c_void,
                        name.len() as u32,
                        null_mut(),
                        0,
                        &mut returned as *mut u32,
                        null_mut(),
                    )
                }
            } else {
                print_usage();
                0
            }
        },
        "clear" => {
            let mut returned: u32 = 0;
            unsafe {
                DeviceIoControl(
                    h_device,
                    ioctl_codes::IOCTL_DELPROTECT_CLEAR,
                    null_mut(),
                    0,
                    null_mut(),
                    0,
                    &mut returned as *mut u32,
                    null_mut(),
                )
            }
        },
        _ => {
            print_usage();
            0
        },
    };

    if status == 0 {
        print_last_error("DeviceIoControl failed");
    }

    unsafe {
        CloseHandle(h_device);
    }
}

fn print_usage() {
    println!("Usage: DelProtectConfig <option> [exename]\n");
    println!("\tOption: add, remove or clear\n");
}
