//! Windows DPAPI protection for secrets at rest (cookies, site passwords). Data is bound to
//! the current Windows user; it is never written to disk or logs in plain text.
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

const ENTROPY: &[u8] = b"SwiftHarryDM/secrets/v1";

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
}

pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    let input = blob(plain);
    let entropy = blob(ENTROPY);
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out).ok()?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out.pbData as *mut core::ffi::c_void));
        Some(v)
    }
}

pub fn unprotect(cipher: &[u8]) -> Option<Vec<u8>> {
    let input = blob(cipher);
    let entropy = blob(ENTROPY);
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out).ok()?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out.pbData as *mut core::ffi::c_void));
        Some(v)
    }
}

pub fn protect_str(s: &str) -> Option<String> {
    protect(s.as_bytes()).map(hex::encode)
}

pub fn unprotect_str(h: &str) -> Option<String> {
    unprotect(&hex::decode(h).ok()?).and_then(|v| String::from_utf8(v).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_tamper() {
        let c = protect(b"SID=abc; HSID=def").unwrap();
        assert!(!c.windows(3).any(|w| w == b"abc"));
        assert_eq!(unprotect(&c).unwrap(), b"SID=abc; HSID=def");
        let mut bad = c.clone();
        let n = bad.len() / 2;
        bad[n] ^= 0xFF;
        assert!(unprotect(&bad).is_none());
        assert_eq!(unprotect_str(&protect_str("pw").unwrap()).unwrap(), "pw");
    }
}
