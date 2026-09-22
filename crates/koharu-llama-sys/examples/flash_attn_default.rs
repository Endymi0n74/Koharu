//! Reads the `flash_attn_type` default from the real llama.cpp binary.
//!
//! Run with the packaged DLL on `PATH`, e.g. on Windows:
//! ```text
//! $env:PATH = "$env:LOCALAPPDATA\koharu\packages\llama\b10903\windows-cuda;$env:PATH"
//! cargo run -p koharu-llama-sys --example flash_attn_default
//! ```
//! Values: -1 = AUTO, 0 = DISABLED, 1 = ENABLED.

fn main() {
    unsafe {
        let params = koharu_llama_sys::llama_context_default_params();
        let policy = params.flash_attn_type;
        let name = koharu_llama_sys::llama_flash_attn_type_name(policy);
        let name = if name.is_null() {
            "<null>".to_owned()
        } else {
            std::ffi::CStr::from_ptr(name)
                .to_string_lossy()
                .into_owned()
        };
        println!("flash_attn_type default = {policy} ({name})");
        match policy {
            -1 => println!("=> AUTO: llama.cpp decides per backend/model"),
            0 => println!("=> DISABLED: flash attention off by default"),
            1 => println!("=> ENABLED: flash attention on by default"),
            other => println!("=> unexpected value {other}"),
        }
    }
}
