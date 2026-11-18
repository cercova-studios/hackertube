//! Separate process: never link upstream and fork extension symbols together.
use rusqlite::Connection;
use std::error::Error;
use std::io::{self, Read};

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os().nth(1).ok_or("expected database path")?;
    // SAFETY: 0.1.9 declares an untyped symbol; its C implementation has SQLite's
    // extension entrypoint ABI. Both crates link the same bundled SQLite runtime.
    let code = unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> std::os::raw::c_int,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )))
    };
    if code != rusqlite::ffi::SQLITE_OK {
        return Err(format!("upstream registration failed: {code}").into());
    }
    let conn = Connection::open(path)?;
    let mut sql = String::new();
    io::stdin().read_to_string(&mut sql)?;
    conn.execute_batch(&sql)?;
    Ok(())
}
