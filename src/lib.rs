use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::jstring;

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_meta_wearable_dat_externalsampleapps_cameraaccess_hive_HiveCore_processInput(
    mut env: JNIEnv,
    _class: JClass,
    input: JString,
) -> jstring {
    let input_str: String = env.get_string(&input).expect("Couldn't get java string!").into();
    
    // For now, testing the bridge. We will integrate the ReAct loop next.
    let response = format!("HIVE Core (Rust) received: {}. JNI Bridge Online.", input_str);
    
    let output = env.new_string(response).expect("Couldn't create java string!");
    output.into_raw()
}
