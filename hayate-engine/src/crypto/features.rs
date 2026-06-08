//! CPU feature detection for hardware-accelerated cryptographic primitives.

/// Dynamically detects if the host CPU possesses hardware AES acceleration.
#[inline]
#[must_use]
pub fn is_aes_hw_accelerated() -> bool {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        std::is_x86_feature_detected!("aes")
    }
    #[cfg(target_arch = "aarch64")]
    {
        std::arch::is_aarch64_feature_detected!("aes")
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
    {
        false
    }
}
