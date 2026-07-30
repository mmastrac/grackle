//! The browser side of search (§6b): `grackle-search-core` compiled to
//! WebAssembly behind a deliberately tiny raw ABI — no wasm-bindgen, no
//! generated glue. `search.js` is the ~60-line loader that talks to this.
//!
//! Protocol (all pointers are offsets into the module's linear memory):
//!
//!   alloc(len)          -> ptr        caller-owned scratch to write into
//!   init(ptr, len)      -> 0 | -1     parse /search.bin (postcard) into state
//!   search(ptr, len)    -> u64        query in; (ptr << 32 | len) of a JSON
//!                                     array of [url, title, date] out
//!                                     (title/date read from the store map)
//!
//! Each search result buffer is leaked and reclaimed on the *next* call —
//! single-threaded, one query in flight, so the previous buffer is dead the
//! moment a new keystroke queries again.

use grackle_search_core::Index;
use std::sync::Mutex;

static INDEX: Mutex<Option<Index>> = Mutex::new(None);
static LAST_RESULT: Mutex<Option<(usize, usize)>> = Mutex::new(None);

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// # Safety
/// `ptr..ptr+len` must be a live allocation from `alloc` holding the index.
#[no_mangle]
pub unsafe extern "C" fn init(ptr: *const u8, len: usize) -> i32 {
    let bytes = std::slice::from_raw_parts(ptr, len);
    match Index::from_bytes(bytes) {
        Ok(idx) => {
            *INDEX.lock().unwrap() = Some(idx);
            0
        }
        Err(_) => -1,
    }
}

/// # Safety
/// `ptr..ptr+len` must be a live allocation from `alloc` holding UTF-8.
#[no_mangle]
pub unsafe extern "C" fn search(ptr: *const u8, len: usize) -> u64 {
    // Reclaim the previous result buffer (dead once a new query arrives).
    if let Some((p, l)) = LAST_RESULT.lock().unwrap().take() {
        drop(Vec::from_raw_parts(p as *mut u8, l, l));
    }
    let query = match std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) {
        Ok(q) => q,
        Err(_) => return 0,
    };
    let guard = INDEX.lock().unwrap();
    let Some(idx) = guard.as_ref() else { return 0 };
    let hits = idx.search(query, 12);

    let mut json = String::with_capacity(1024);
    json.push('[');
    for (i, (url, store)) in hits.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        // Overlay still hardcodes date + title from the store bag.
        let title = store.get("title").map(String::as_str).unwrap_or("");
        let date = store.get("date").map(String::as_str).unwrap_or("");
        json.push('[');
        json_str(url, &mut json);
        json.push(',');
        json_str(title, &mut json);
        json.push(',');
        json_str(date, &mut json);
        json.push(']');
    }
    json.push(']');

    let mut bytes = json.into_bytes();
    bytes.shrink_to_fit();
    let out_len = bytes.len();
    let out_ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    *LAST_RESULT.lock().unwrap() = Some((out_ptr as usize, out_len));
    ((out_ptr as u64) << 32) | out_len as u64
}

/// Minimal JSON string escaping — enough for titles that contain quotes,
/// backslashes or control characters.
fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
