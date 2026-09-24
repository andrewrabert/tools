//! Lossless JPEG re-encoding through mozjpeg: the DCT coefficients are kept and
//! only the entropy coding is redone with mozjpeg's progressive scan search.
//! Markers (EXIF, ICC, comments) are not carried over.

use std::cell::UnsafeCell;
use std::ffi::{c_ulong, c_void};
use std::mem;
use std::panic::{self, AssertUnwindSafe};
use std::ptr;
use std::slice;

use anyhow::{Result, anyhow};
use mozjpeg_sys::{
    jpeg_common_struct, jpeg_compress_struct, jpeg_copy_critical_parameters, jpeg_create_compress,
    jpeg_create_decompress, jpeg_decompress_struct, jpeg_destroy_compress, jpeg_destroy_decompress,
    jpeg_error_mgr, jpeg_finish_compress, jpeg_finish_decompress, jpeg_mem_dest, jpeg_mem_src,
    jpeg_read_coefficients, jpeg_read_header, jpeg_std_error, jpeg_write_coefficients,
};

const MESSAGE_LEN: usize = 80;

unsafe extern "C" {
    fn free(ptr: *mut c_void);
}

/// The libjpeg error message raised out of a callback.
struct Failure(String);

pub fn optimize(input: &[u8]) -> Result<Vec<u8>> {
    // SAFETY: the library is driven in the documented order on structs that
    // outlive every pointer handed to it; errors unwind out of the callbacks
    // (mozjpeg-sys is built with `unwinding`) and are caught right here.
    match panic::catch_unwind(AssertUnwindSafe(|| unsafe { transcode(input) })) {
        Ok(data) => Ok(data),
        Err(payload) => match payload.downcast::<Failure>() {
            Ok(failure) => Err(anyhow!("mozjpeg: {}", failure.0)),
            Err(payload) => panic::resume_unwind(payload),
        },
    }
}

struct Decompressor(jpeg_decompress_struct);

impl Drop for Decompressor {
    fn drop(&mut self) {
        // SAFETY: created by `jpeg_create_decompress` and destroyed once.
        unsafe { jpeg_destroy_decompress(&mut self.0) };
    }
}

struct Compressor(jpeg_compress_struct);

impl Drop for Compressor {
    fn drop(&mut self) {
        // SAFETY: created by `jpeg_create_compress` and destroyed once.
        unsafe { jpeg_destroy_compress(&mut self.0) };
    }
}

/// The buffer `jpeg_mem_dest` allocates with malloc.
struct Output {
    ptr: *mut u8,
    len: c_ulong,
}

impl Drop for Output {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: allocated by libjpeg with malloc and freed once.
            unsafe { free(self.ptr.cast()) };
        }
    }
}

unsafe fn transcode(input: &[u8]) -> Vec<u8> {
    // SAFETY: every struct is zeroed and then initialized by the library;
    // `error` outlives both codec structs that point at it.
    unsafe {
        let mut error: jpeg_error_mgr = mem::zeroed();
        jpeg_std_error(&mut error);
        error.error_exit = Some(error_exit);
        error.output_message = Some(output_message);

        let mut source = Decompressor(mem::zeroed());
        source.0.common.err = &mut error;
        jpeg_create_decompress(&mut source.0);
        let mut target = Compressor(mem::zeroed());
        target.0.common.err = &mut error;
        jpeg_create_compress(&mut target.0);

        jpeg_mem_src(&mut source.0, input.as_ptr(), input.len() as c_ulong);
        jpeg_read_header(&mut source.0, 1);
        let coefficients = jpeg_read_coefficients(&mut source.0);
        jpeg_copy_critical_parameters(&source.0, &mut target.0);

        let mut output = Output {
            ptr: ptr::null_mut(),
            len: 0,
        };
        jpeg_mem_dest(&mut target.0, &mut output.ptr, &mut output.len);
        jpeg_write_coefficients(&mut target.0, coefficients);
        jpeg_finish_compress(&mut target.0);
        jpeg_finish_decompress(&mut source.0);

        slice::from_raw_parts(output.ptr, output.len as usize).to_vec()
    }
}

unsafe extern "C-unwind" fn error_exit(cinfo: &mut jpeg_common_struct) {
    // SAFETY: `err` points at the error manager installed in `transcode`.
    let message = unsafe { message_of(cinfo) };
    panic::resume_unwind(Box::new(Failure(message)));
}

unsafe extern "C-unwind" fn output_message(_cinfo: &mut jpeg_common_struct) {}

unsafe fn message_of(cinfo: &mut jpeg_common_struct) -> String {
    // SAFETY: the error manager is valid for the life of `cinfo`, and
    // `format_message` writes at most MESSAGE_LEN bytes into the cell.
    unsafe {
        let error = &*cinfo.err;
        let Some(format_message) = error.format_message else {
            return format!("libjpeg error {}", error.msg_code);
        };
        let buffer = UnsafeCell::new([0u8; MESSAGE_LEN]);
        format_message(cinfo, &*buffer.get());
        let bytes = &*buffer.get();
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }
}
