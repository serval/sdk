use anyhow::anyhow;
use std::mem::size_of;

// Declare all of the host functions we need
#[link(wasm_import_module = "serval")]
extern "C" {
    #[link_name = "invoke_raw"]
    fn invoke_raw(name_ptr: u32, name_len: u32, data_ptr: u32, data_len: u32) -> i32;
}

/// Invokes the extension with the give name, passing along an arbitrary blob of data. returns the
/// data returned by the extension.
pub fn invoke_extension(extension_name: String, data: &Vec<u8>) -> Result<Vec<u8>, anyhow::Error> {
    let extension_name_bytes = extension_name.into_bytes();
    let extension_name_ptr = extension_name_bytes.as_ptr() as u32;

    let data_ptr = data.as_ptr() as u32;

    let out_ptr = unsafe {
        invoke_raw(
            extension_name_ptr,
            extension_name_bytes.len() as u32,
            data_ptr,
            data.len() as u32,
        )
    };

    if out_ptr < 0 {
        // A return value of 0 is used to signal that an error occurred.
        // TODO: We should probably start returning a signed integer instead and use negative
        // numbers to signal specific errors.
        return Err(anyhow!(
            "invoke_capability failed with error code {out_ptr}"
        ));
    }

    get_bytes_from_host(out_ptr as usize)
}

/// Allocate memory into the module's linear memory and return the offset to the start of the block.
/// Source: https://radu-matei.com/blog/practical-guide-to-wasm-memory/#exchanging-strings-between-modules-and-runtimes
#[no_mangle]
pub fn alloc(len: usize) -> *mut u8 {
    // create a new mutable buffer with capacity `len`
    let mut buf = Vec::with_capacity(len);
    // take a mutable pointer to the buffer
    let ptr = buf.as_mut_ptr();
    // take ownership of the memory block and ensure that its destructor is not called when the
    // object goes out of scope at the end of the function
    std::mem::forget(buf);
    // todo: ensure the pointer doesn't happen to be at offset 0, since that is used to signal an error
    // return the pointer so the runtime can write data at this offset
    ptr
}

/// Deallocates a chunk of memory that was originally allocated with our `alloc` function.
/// Source: https://radu-matei.com/blog/practical-guide-to-wasm-memory/#exchanging-strings-between-modules-and-runtimes
/// # Safety
/// See the docs on [Vec#from_raw_parts](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.from_raw_parts)
#[no_mangle]
pub unsafe fn dealloc(ptr: *mut u8, size: usize) {
    let data = Vec::from_raw_parts(ptr, size, size);

    std::mem::drop(data);
}

/// Retrieves a blob of bytes that the host environment is trying to pass to us. Since we can only
/// communicate by passing around single numbers, the way the Serval host envioronment works is by
/// asking us (the guest) to allocate N + 4 bytes of memory, where N is the number of bytes of data
/// that they're trying to send us. The host writes N as a u32 into the first 4 bytes of the memory
/// range. When we receive a pointer, we read a u32 from it to figure out how many bytes of data to
/// read, read the data, and then clean up the entire memory allocation afterwards.
fn get_bytes_from_host(ptr: usize) -> Result<Vec<u8>, anyhow::Error> {
    // TODO: figure out how to make this unsafe stuff sufficiently safe to sleep at night.

    // ptr points to a u32, followed by N bytes of data intended for us. That first u32 tells us
    // what the value of N is.
    let mut len_buf = [0u8; size_of::<i32>()];
    let num_bytes = unsafe {
        let ptr = &*(ptr as *const u8);
        std::ptr::copy(ptr, len_buf.as_mut_ptr(), size_of::<u32>());
        u32::from_le_bytes(len_buf)
    };

    // Now that we know how many bytes of data there are, we can read 'em into a buffer
    let bytes: Vec<u8> = unsafe {
        let mut buf = vec![0; num_bytes as usize];
        let ptr = &*((ptr + size_of::<u32>()) as *const u8);
        std::ptr::copy(ptr, buf.as_mut_ptr(), num_bytes as usize);
        buf
    };

    // The block of memory at ptr was allocated by the host calling into our alloc function; now
    // that we have read the data they were trying to pass to us, we can clean up that temporary
    // allocation.
    let alloc_size = size_of::<u32>() + num_bytes as usize;
    unsafe {
        let ptr = ptr as *mut u8;
        dealloc(ptr, alloc_size);
    };

    Ok(bytes)
}
