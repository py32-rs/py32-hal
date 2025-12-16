//! Unique ID (UID)

// The following code is modified from embassy-stm32
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32
// Special thanks to the Embassy Project and its contributors for their work!

/// Get this device's unique 128-bit ID.
pub fn uid() -> [u8; 16] {
    unsafe { *crate::pac::UID.uid(0).as_ptr().cast::<[u8; 16]>() }
}

/// Get this device's unique 128-bit ID, encoded into a string of 32 hexadecimal ASCII digits.
pub fn uid_hex() -> &'static str {
    unsafe { core::str::from_utf8_unchecked(uid_hex_bytes()) }
}

/// Get this device's unique 128-bit ID, encoded into 32 hexadecimal ASCII bytes.
pub fn uid_hex_bytes() -> &'static [u8; 32] {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    static mut UID_HEX: [u8; 32] = [0; 32];
    static mut LOADED: bool = false;
    critical_section::with(|_| unsafe {
        if !LOADED {
            let uid = uid();
            for (idx, v) in uid.iter().enumerate() {
                let lo = v & 0x0f;
                let hi = (v & 0xf0) >> 4;
                UID_HEX[idx * 2] = HEX[hi as usize];
                UID_HEX[idx * 2 + 1] = HEX[lo as usize];
            }
            LOADED = true;
        }
    });
    unsafe { &*core::ptr::addr_of!(UID_HEX) }
}