use anyhow::Result;
use libc::c_char;
use std::ffi::CStr;
use std::io;

// inspired by https://crates.io/crates/uname
// inlined because currently not packaged in Ubuntu Focal
#[inline]
fn to_cstr(buf: &[c_char]) -> &CStr {
    unsafe { CStr::from_ptr(buf.as_ptr()) }
}
pub fn uname_n() -> Result<String> {
    let mut n = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::uname(&mut n) };
    if r == 0 {
        Ok(to_cstr(&n.nodename[..]).to_string_lossy().into_owned())
    } else {
        Err(anyhow::anyhow!(io::Error::last_os_error()))
    }
}
