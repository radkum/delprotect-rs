#![no_std]
#![allow(non_snake_case)]
#![allow(static_mut_ref)]
extern crate alloc;

mod cleaner;

/// kernel-init deliver a few elements (eg. panic implementation) necessary to run code in kernel
#[allow(unused_imports)]
use kernel_init;
use kernel_macros::{NT_SUCCESS, PAGED_CODE};

use common::ioctl_codes;

use kernel_string::{PUNICODE_STRING, UNICODE_STRING};
use km_api_sys::{
    flt_kernel::*,
    ntddk::{PFILE_DISPOSITION_INFORMATION, PROCESSINFOCLASS},
    ntifs::{ObOpenObjectByPointer, PsGetThreadProcess},
    ntoskrnl::{ExAllocatePool2, ExFreePoolWithTag, POOL_FLAG_PAGED},
    wmd::{NtCurrentProcess, ZwClose, ZwQueryInformationProcess, FILE_DELETE_ON_CLOSE},
};

use kernel_log::KernelLogger;
use log::LevelFilter;
use winapi::{
    km::wdm::{DEVICE_TYPE, DRIVER_OBJECT, KPROCESSOR_MODE},
    shared::{
        ntdef::{FALSE, HANDLE, NTSTATUS, OBJ_KERNEL_HANDLE, PVOID, ULONG, USHORT},
        ntstatus::{STATUS_ACCESS_DENIED, STATUS_INSUFFICIENT_RESOURCES, STATUS_SUCCESS},
    },
};

use crate::cleaner::Cleaner;
use winapi::{
    km::wdm::{
        IoCompleteRequest, IoCreateDevice, IoCreateSymbolicLink, IoDeleteDevice,
        IoDeleteSymbolicLink, IoGetCurrentIrpStackLocation, DEVICE_OBJECT, IRP, IRP_MJ,
        PDEVICE_OBJECT,
    },
    shared::ntstatus::STATUS_INVALID_DEVICE_REQUEST,
};

use alloc::{collections::VecDeque, string::String};
use core::ptr::null_mut;
use kernel_fast_mutex::{auto_lock::AutoLock, fast_mutex::FastMutex, locker::Locker};

const POOL_TAG: u32 = u32::from_ne_bytes(*b"RDER");
const MAX_ITEM_COUNT: usize = 32;

const DEVICE_NAME: &str = "\\Device\\DelProtect";
const SYM_LINK_NAME: &str = "\\??\\DelProtect";

static mut G_PROCESS_NAMES: Option<VecDeque<String>> = None;
static mut G_MUTEX: FastMutex = FastMutex::new();
static mut G_FILTER_HANDLE: PFLT_FILTER = null_mut();

const CALLBACKS: &'static [FLT_OPERATION_REGISTRATION] = {
    &[
        FLT_OPERATION_REGISTRATION::new()
            .set_major_function(FLT_OPERATION_REGISTRATION::IRP_MJ_CREATE)
            .set_preop(DelProtectPreCreate),
        FLT_OPERATION_REGISTRATION::new()
            .set_major_function(FLT_OPERATION_REGISTRATION::IRP_MJ_SET_INFORMATION)
            .set_preop(DelProtectPreSetInformation),
        FLT_OPERATION_REGISTRATION::new()
            .set_major_function(FLT_OPERATION_REGISTRATION::IRP_MJ_OPERATION_END),
    ]
};

const FILTER_REGISTRATION: FLT_REGISTRATION = FLT_REGISTRATION {
    Size: ::core::mem::size_of::<FLT_REGISTRATION>() as USHORT, /*sizeof*/
    Version: FLT_REGISTRATION_VERSION,
    Flags: 0,
    ContextRegistration: null_mut(),
    OperationRegistration: CALLBACKS.as_ptr(),
    FilterUnloadCallback: DelProtectUnload,
    InstanceSetupCallback: DelProtectInstanceSetup,
    InstanceQueryTeardownCallback: DelProtectInstanceQueryTeardown,
    InstanceTeardownStartCallback: DelProtectInstanceTeardownStart,
    InstanceTeardownCompleteCallback: DelProtectInstanceTeardownComplete,
    GenerateFileNameCallback: null_mut(),
    NormalizeNameComponentCallback: null_mut(),
    NormalizeContextCleanupCallback: null_mut(),
    TransactionNotificationCallback: null_mut(),
    NormalizeNameComponentExCallback: null_mut(),
    SectionNotificationCallback: null_mut(),
};

/*************************************************************************
    MiniFilter initialization and unload routines.
*************************************************************************/
#[link_section = "INIT"]
#[no_mangle]
pub unsafe extern "system" fn DriverEntry(
    driver: &mut DRIVER_OBJECT,
    _path: *const UNICODE_STRING,
) -> NTSTATUS {
    KernelLogger::init(LevelFilter::Info).expect("Failed to initialize logger");

    log::info!("START DelProtect");

    let hello_world = UNICODE_STRING::create("Hello World!");
    log::info!("{}", hello_world.as_rust_string().unwrap_or_default());

    //--------------------GLOBALS-----------------------
    G_MUTEX.Init();

    //init processes vector
    let mut events = VecDeque::new();
    if let Err(e) = events.try_reserve_exact(MAX_ITEM_COUNT) {
        log::info!(
            "fail to reserve a {} bytes of memory. Err: {:?}",
            ::core::mem::size_of::<String>() * MAX_ITEM_COUNT,
            e
        );
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    G_PROCESS_NAMES = Some(events);

    //--------------------INIT VARIABLES-----------------------
    #[allow(unused_assignments)]
    let mut status = STATUS_SUCCESS;

    let dev_name = UNICODE_STRING::from(DEVICE_NAME);
    let sym_link = UNICODE_STRING::from(SYM_LINK_NAME);

    let mut cleaner = Cleaner::new();
    let mut device_object: PDEVICE_OBJECT = null_mut();

    loop {
        //--------------------DEVICE-----------------------
        status = IoCreateDevice(
            driver,
            0,
            dev_name.as_ptr(),
            DEVICE_TYPE::FILE_DEVICE_UNKNOWN,
            0,
            FALSE,
            &mut device_object,
        );

        if NT_SUCCESS!(status) {
            cleaner.init_device(device_object);
        } else {
            log::info!("failed to create device 0x{:08x}", status);
            break;
        }

        //--------------------SYMLINK-----------------------
        status = IoCreateSymbolicLink(&sym_link.as_ntdef_unicode(), &dev_name.as_ntdef_unicode());

        if NT_SUCCESS!(status) {
            cleaner.init_symlink(&sym_link);
        } else {
            log::info!("failed to create sym_link 0x{:08x}", status);
            break;
        }

        //--------------------FILTER_HANDLE-----------------------
        status = FltRegisterFilter(driver, &FILTER_REGISTRATION, &mut G_FILTER_HANDLE);

        if NT_SUCCESS!(status) {
            cleaner.init_filter_handle(G_FILTER_HANDLE);
        } else {
            log::info!("failed to create sym_link 0x{:08x}", status);
            break;
        }

        //--------------------DISPATCH_ROUTINES-----------------------
        driver.DriverUnload = Some(DelProtectUnloadDriver);
        driver.MajorFunction[IRP_MJ::CREATE as usize] = Some(DispatchCreateClose);
        driver.MajorFunction[IRP_MJ::CLOSE as usize] = Some(DispatchCreateClose);
        driver.MajorFunction[IRP_MJ::DEVICE_CONTROL as usize] = Some(DispatchDeviceControl);

        status = FltStartFiltering(G_FILTER_HANDLE);
        break;
    }

    if NT_SUCCESS!(status) {
        log::info!("SUCCESS");
    } else {
        cleaner.clean();
    }

    log::info!("SUCCESS: {}", status);
    status
}

extern "system" fn DelProtectUnload(_flags: FLT_REGISTRATION_FLAGS) -> NTSTATUS {
    log::info!("delprotect_unload");

    PAGED_CODE!();
    unsafe {
        FltUnregisterFilter(G_FILTER_HANDLE);
    }

    STATUS_SUCCESS
}

#[link_section = "PAGE"]
extern "system" fn DelProtectInstanceSetup(
    _flt_objects: PFLT_RELATED_OBJECTS,
    _flags: FLT_INSTANCE_SETUP_FLAGS,
    _volume_device_type: DEVICE_TYPE,
    _volume_filesystem_type: FLT_FILESYSTEM_TYPE,
) -> NTSTATUS {
    //log::info!("DelProtectInstanceSetup");
    PAGED_CODE!();
    STATUS_SUCCESS
}

#[link_section = "PAGE"]
extern "system" fn DelProtectInstanceQueryTeardown(
    _flt_objects: PFLT_RELATED_OBJECTS,
    _flags: FLT_INSTANCE_QUERY_TEARDOWN_FLAGS,
) -> NTSTATUS {
    //log::info!("DelProtectInstanceQueryTeardown");

    PAGED_CODE!();
    unsafe {
        FltUnregisterFilter(G_FILTER_HANDLE);
    }
    //log::info!("DelProtectInstanceQueryTeardown SUCCESS");
    STATUS_SUCCESS
}

#[link_section = "PAGE"]
extern "system" fn DelProtectInstanceTeardownStart(
    _flt_objects: PFLT_RELATED_OBJECTS,
    _flags: FLT_INSTANCE_TEARDOWN_FLAGS,
) -> NTSTATUS {
    //log::info!("DelProtectInstanceTeardownStart");

    PAGED_CODE!();
    //log::info!("DelProtectInstanceTeardownStart SUCCESS");
    STATUS_SUCCESS
}

#[link_section = "PAGE"]
extern "system" fn DelProtectInstanceTeardownComplete(
    _flt_objects: PFLT_RELATED_OBJECTS,
    _flags: FLT_INSTANCE_TEARDOWN_FLAGS,
) -> NTSTATUS {
    //log::info!("DelProtectInstanceTeardownComplete");

    PAGED_CODE!();
    //log::info!("DelProtectInstanceTeardownComplete SUCCESS");
    STATUS_SUCCESS
}

/*************************************************************************
    MiniFilter callback routines.
*************************************************************************/
extern "system" fn DelProtectPreCreate(
    data: &mut FLT_CALLBACK_DATA,
    _flt_objects: &mut FLT_RELATED_OBJECTS,
    _reserved: *mut PVOID,
) -> FLT_PREOP_CALLBACK_STATUS {
    let mut status = FLT_PREOP_CALLBACK_STATUS::FLT_PREOP_SUCCESS_NO_CALLBACK;

    //let mut data = data as &mut FLT_CALLBACK_DATA;
    if let KPROCESSOR_MODE::KernelMode = data.RequestorMode {
        return status;
    }
    //log::info!("DelProtectPreCreate not in kernel");

    unsafe {
        let params = &(*data.Iopb).Parameters.Create;

        if (params.Options & FILE_DELETE_ON_CLOSE) > 0 {
            log::info!("Delete on close");
            if !IsDeleteAllowed(NtCurrentProcess()) {
                *data.IoStatus.__bindgen_anon_1.Status_mut() = STATUS_ACCESS_DENIED;
                status = FLT_PREOP_CALLBACK_STATUS::FLT_PREOP_COMPLETE;
                log::info!("Prevent delete by cmd.exe");
            }
        }
    }

    status
}

extern "system" fn DelProtectPreSetInformation(
    data: &mut FLT_CALLBACK_DATA,
    _flt_objects: &mut FLT_RELATED_OBJECTS,
    _reserved: *mut PVOID,
) -> FLT_PREOP_CALLBACK_STATUS {
    //log::info!("DelProtectPreSetInformation");
    let mut status = FLT_PREOP_CALLBACK_STATUS::FLT_PREOP_SUCCESS_NO_CALLBACK;

    let params = unsafe { &(*data.Iopb).Parameters.SetFileInformation };

    match params.FileInformationClass {
        FILE_INFORMATION_CLASS::FileDispositionInformation
        | FILE_INFORMATION_CLASS::FileDispositionInformationEx => {},
        _ => return status,
    }

    let info = params.InfoBuffer as PFILE_DISPOSITION_INFORMATION;
    unsafe {
        if (*info).DeleteFile == 0 {
            return status;
        }

        let process = PsGetThreadProcess(data.Thread);
        if process.is_null() {
            //something is wrong
            return status;
        }

        let mut h_process: HANDLE = usize::MAX as HANDLE;
        let ret = ObOpenObjectByPointer(
            process,
            OBJ_KERNEL_HANDLE,
            null_mut(),
            0,
            null_mut(),
            KPROCESSOR_MODE::KernelMode,
            &mut h_process,
        );
        if !NT_SUCCESS!(ret) {
            return status;
        }

        if !IsDeleteAllowed(h_process) {
            *data.IoStatus.__bindgen_anon_1.Status_mut() = STATUS_ACCESS_DENIED;
            status = FLT_PREOP_CALLBACK_STATUS::FLT_PREOP_COMPLETE;
            log::info!("Prevent delete by cmd.exe");
        }
        ZwClose(h_process);
    }
    status
}

unsafe fn IsDeleteAllowed(h_process: HANDLE) -> bool {
    let process_name_size = 300;
    let process_name =
        ExAllocatePool2(POOL_FLAG_PAGED, process_name_size, POOL_TAG) as PUNICODE_STRING;

    if process_name.is_null() {
        log::info!("fail to reserve a {} bytes of memory", process_name_size);
        return true;
    }

    let mut delete_allowed = true;
    let mut return_length: ULONG = 0;
    let status = ZwQueryInformationProcess(
        h_process,
        PROCESSINFOCLASS::ProcessImageFileName,
        process_name as PVOID,
        (process_name_size - 2) as u32,
        &mut return_length,
    );

    // log::info!(
    //     "ZwQueryInformationProcess - status: {}, returnLength: {}",
    //     status,
    //     return_length
    // );

    if NT_SUCCESS!(status) {
        let process_name = &*process_name;

        if process_name.Length != 0 {
            let rust_process_name = process_name.as_rust_string().unwrap_or_default();
            log::info!("Delete operation from {}", rust_process_name);
            let _locker = AutoLock::new(&mut G_MUTEX);
            if let Some(process_names) = &G_PROCESS_NAMES {
                for name in process_names {
                    log::info!("name (from list) in bytes: {:?}", name.as_bytes());
                    log::info!("name (from list): \"{}\"", name);
                    log::info!("name to delete: \"{}\"", rust_process_name);
                    log::info!(
                        "name to delete in bytes: {:?}",
                        rust_process_name.as_bytes()
                    );
                    if rust_process_name.contains(name) {
                        delete_allowed = false;
                        log::info!("DELETE BLOCK ");
                        break;
                    }
                }
            }
        }
    }

    ExFreePoolWithTag(process_name as PVOID, POOL_TAG);

    delete_allowed
}

/*************************************************************************
                    Dispatch  routines.
*************************************************************************/
extern "system" fn DelProtectUnloadDriver(driver: &mut DRIVER_OBJECT) {
    log::info!("rust_unload");
    unsafe {
        IoDeleteDevice(driver.DeviceObject);

        let sym_link = UNICODE_STRING::create(SYM_LINK_NAME);
        IoDeleteSymbolicLink(&sym_link.as_ntdef_unicode());
    }
}

extern "system" fn DispatchCreateClose(_driver: &mut DEVICE_OBJECT, irp: &mut IRP) -> NTSTATUS {
    complete_irp_success(irp)
}

extern "system" fn DispatchDeviceControl(_driver: &mut DEVICE_OBJECT, irp: &mut IRP) -> NTSTATUS {
    unsafe {
        let stack = IoGetCurrentIrpStackLocation(irp);
        let device_io = (*stack).Parameters.DeviceIoControl();

        log::info!("device_io.IoControlCode: {} ", device_io.IoControlCode);
        match device_io.IoControlCode {
            ioctl_codes::IOCTL_DELPROTECT_ADD_EXE_UTF8 => {
                log::info!("IOCTL_DELPROTECT_ADD_EXE_UTF8 ");
                let proc_name = get_rust_name_from_system_buffer_UTF8(
                    *irp.AssociatedIrp.SystemBuffer() as PVOID as *mut u8,
                    device_io.InputBufferLength as usize,
                );

                log::info!("proc_name: {}", proc_name);

                push_item_thread_safe(&proc_name);
            },
            ioctl_codes::IOCTL_DELPROTECT_ADD_EXE_UTF16 => {
                log::info!("IOCTL_DELPROTECT_ADD_EXE_UTF16 ");
                let proc_name = get_rust_name_from_system_buffer_UTF16(
                    *irp.AssociatedIrp.SystemBuffer() as PVOID as *mut u16,
                    device_io.InputBufferLength as usize,
                );

                log::info!("proc_name: {}", proc_name);

                push_item_thread_safe(&proc_name);
            },
            ioctl_codes::IOCTL_DELPROTECT_CLEAR => {
                log::info!("before lock ");
                let _locker = AutoLock::new(&mut G_MUTEX);
                log::info!("after lock");
                if let Some(events) = &mut G_PROCESS_NAMES {
                    log::info!("before clear ");
                    events.clear();
                    log::info!("after clear ");
                }
            },
            _ => {
                log::info!("IOCTL_ other ");
                return complete_irp_with_status(irp, STATUS_INVALID_DEVICE_REQUEST);
            },
        }
    }

    complete_irp_success(irp)
}

unsafe fn get_rust_name_from_system_buffer_UTF8(name: *mut u8, name_len_in_bytes: usize) -> String {
    let name_len = {
        let name_len = name_len_in_bytes / ::core::mem::size_of::<u8>();

        if *name.offset(name_len as isize - 1) == 0 {
            name_len - 1
        } else {
            name_len
        }
    };

    let buffer = core::slice::from_raw_parts::<u8>(name, name_len);
    String::from_utf8_lossy(buffer).into()
}

unsafe fn get_rust_name_from_system_buffer_UTF16(
    name: *mut u16,
    name_len_in_bytes: usize,
) -> String {
    let name_len = {
        let name_len = name_len_in_bytes / ::core::mem::size_of::<u16>();

        if *name.offset(name_len as isize - 1) == 0 {
            name_len - 1
        } else {
            name_len
        }
    };

    let buffer = core::slice::from_raw_parts::<u16>(name, name_len);
    String::from_utf16_lossy(buffer)
}
/*************************************************************************
                    IRP functions
*************************************************************************/
fn complete_irp_with_status(irp: &mut IRP, status: NTSTATUS) -> NTSTATUS {
    complete_irp(irp, status, 0)
}

fn complete_irp_success(irp: &mut IRP) -> NTSTATUS {
    complete_irp_with_status(irp, STATUS_SUCCESS)
}

fn complete_irp(irp: &mut IRP, status: NTSTATUS, info: usize) -> NTSTATUS {
    unsafe {
        let s = irp.IoStatus.__bindgen_anon_1.Status_mut();
        *s = status;
        irp.IoStatus.Information = info;
        IoCompleteRequest(irp, 0);
    }

    status
}

/*************************************************************************
                    Thread safe operations.
*************************************************************************/
unsafe fn push_item_thread_safe(process_name: &str) {
    let mut p_name = String::new();
    if let Err(e) = p_name.try_reserve_exact(process_name.len()) {
        log::info!(
            "fail to reserve a {} bytes of memory. Err: {:?}",
            process_name.len(),
            e
        );
        return;
    }
    p_name.push_str(process_name);
    let _locker = AutoLock::new(&mut G_MUTEX);
    if let Some(process_names) = &mut G_PROCESS_NAMES {
        if process_names.len() >= MAX_ITEM_COUNT {
            process_names.pop_front();
        }
        process_names.push_back(p_name);
    }
}
