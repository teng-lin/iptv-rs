//! JNI lifetime adapter for the same transport used by desktop mpv.
use iptv_net::playback::PlaybackRelay;
use jni::{
    JNIEnv,
    objects::{JObject, JString},
    sys::{jboolean, jstring},
};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

fn relays() -> &'static Mutex<HashMap<String, PlaybackRelay>> {
    static RELAYS: OnceLock<Mutex<HashMap<String, PlaybackRelay>>> = OnceLock::new();
    RELAYS.get_or_init(Mutex::default)
}
fn string(env: &mut JNIEnv, value: &JString) -> Option<String> {
    env.get_string(value).ok().map(String::from)
}
fn headers(env: &mut JNIEnv, value: &JString) -> Option<Vec<(String, String)>> {
    let value = string(env, value)?;
    serde_json::from_str::<HashMap<String, String>>(&value)
        .ok()
        .map(|h| h.into_iter().collect())
}
fn failed(env: &mut JNIEnv) {
    let _ = env.throw_new("java/io/IOException", "Playback transport unavailable");
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hantekllc_iptvrs_host_PlaybackTransport_create(
    mut env: JNIEnv,
    _object: JObject,
    url: JString,
    fields: JString,
    public_only: jboolean,
) -> jstring {
    let result = (|| {
        let url = string(&mut env, &url)?;
        let fields = headers(&mut env, &fields)?;
        let relay = PlaybackRelay::start_with_policy(
            &url,
            &fields,
            crate::transport::policy(public_only != 0),
        )
        .ok()?;
        let local = relay.url().to_owned();
        let value = env.new_string(&local).ok()?;
        relays().lock().ok()?.insert(local, relay);
        Some(value.into_raw())
    })();
    result.unwrap_or_else(|| {
        failed(&mut env);
        std::ptr::null_mut()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hantekllc_iptvrs_host_PlaybackTransport_updateHeaders(
    mut env: JNIEnv,
    _object: JObject,
    local: JString,
    fields: JString,
) {
    let result = (|| {
        let local = string(&mut env, &local)?;
        let fields = headers(&mut env, &fields)?;
        relays()
            .lock()
            .ok()?
            .get(&local)?
            .update_headers(&fields)
            .ok()
    })();
    if result.is_none() {
        failed(&mut env);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hantekllc_iptvrs_host_PlaybackTransport_release(
    mut env: JNIEnv,
    _object: JObject,
    local: JString,
) {
    if let Some(local) = string(&mut env, &local) {
        if let Ok(mut relays) = relays().lock() {
            relays.remove(&local);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hantekllc_iptvrs_host_PlaybackTransport_takeErrorCode(
    mut env: JNIEnv,
    _object: JObject,
    local: JString,
) -> jstring {
    let result = (|| {
        let local = string(&mut env, &local)?;
        let code = relays().lock().ok()?.get(&local)?.take_error_code()?;
        env.new_string(code).ok().map(|value| value.into_raw())
    })();
    result.unwrap_or(std::ptr::null_mut())
}
