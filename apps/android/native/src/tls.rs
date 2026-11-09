use jni::{
    EnvUnowned,
    objects::{JClass, JObject},
};

// SAFETY: Java's static initialize(Object) supplies an attached JNI thread and
// a live application Context. with_env catches panics before crossing JNI.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_ultratasks_system_NativeBridge_initialize<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) {
    unowned
        .with_env(|env| rustls_platform_verifier::android::init_with_env(env, context))
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}
