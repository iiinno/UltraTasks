//! JNI system bridge; no renderer, WebView or alternate data store.
mod network;
mod operations;
#[cfg(target_os = "android")]
mod tls;
use jni::{
    EnvUnowned,
    objects::{JClass, JString},
};
use operations::Access;

// SAFETY: These uniquely named symbols match NativeBridge's static
// (String,String)->String JNI signatures. The VM supplies an attached-thread
// EnvUnowned and live local references. with_env catches panics and its error
// policy converts them to Java exceptions; no unwind crosses the ABI boundary.
// Export attributes are the only unsafe-code exception in this JNI host.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_ultratasks_system_NativeBridge_dispatch<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    path: JString<'caller>,
    request: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<_, jni::errors::Error> {
            JString::from_str(
                env,
                operations::dispatch(&path.to_string(), &request.to_string(), Access::ReadWrite),
            )
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_ultratasks_system_NativeBridge_dispatchReadOnly<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    path: JString<'caller>,
    request: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<_, jni::errors::Error> {
            JString::from_str(
                env,
                operations::dispatch(&path.to_string(), &request.to_string(), Access::ReadOnly),
            )
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
