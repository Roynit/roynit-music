use anyhow::Result;
use serde_json::Value;
use std::path::Path;

pub const SUPPORTED: bool = cfg!(windows);
#[cfg(windows)]
fn crypt(bytes: &[u8], protect: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    anyhow::ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "Credential payload too large"
    );
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI reads the input for this call and allocates output with LocalAlloc.
    // No machine-wide flag: ciphertext belongs to the current Windows user.
    let ok = unsafe {
        if protect {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}
pub fn save(path: &Path, value: &Value) -> Result<()> {
    #[cfg(windows)]
    {
        use std::io::Write;
        let bytes = crypt(&serde_json::to_vec(value)?, true)?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid vault path"))?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
    }
    #[cfg(not(windows))]
    {
        let _ = (path, value);
    }
    Ok(())
}
pub fn load(path: &Path) -> Result<Option<Value>> {
    if !path.is_file() {
        return Ok(None);
    }
    #[cfg(windows)]
    {
        Ok(Some(serde_json::from_slice(&crypt(
            &std::fs::read(path)?,
            false,
        )?)?))
    }
    #[cfg(not(windows))]
    {
        Ok(None)
    }
}
