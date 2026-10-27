#include <jni.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

extern void tc_reset(bool demo);
extern bool tc_snapshot(const uint8_t *bytes, size_t len);
extern void tc_render(uint32_t *pixels, size_t len, uint32_t hour, uint32_t minute, uint32_t second);
extern void tc_touch(int32_t x, int32_t y, bool scroll);

JNIEXPORT void JNICALL Java_cafe_token_android_NativeCafe_reset(JNIEnv *env, jclass cls, jboolean demo) {
    (void)env; (void)cls;
    tc_reset(demo == JNI_TRUE);
}
JNIEXPORT jboolean JNICALL Java_cafe_token_android_NativeCafe_snapshot(JNIEnv *env, jclass cls, jbyteArray data) {
    (void)cls;
    jsize len = (*env)->GetArrayLength(env, data);
    jbyte *bytes = (*env)->GetByteArrayElements(env, data, NULL);
    if (!bytes) return JNI_FALSE;
    bool ok = tc_snapshot((const uint8_t *)bytes, (size_t)len);
    (*env)->ReleaseByteArrayElements(env, data, bytes, JNI_ABORT);
    return ok ? JNI_TRUE : JNI_FALSE;
}
JNIEXPORT void JNICALL Java_cafe_token_android_NativeCafe_render(JNIEnv *env, jclass cls, jintArray frame, jint hour, jint minute, jint second) {
    (void)cls;
    if ((*env)->GetArrayLength(env, frame) != 480 * 272) return;
    jint *pixels = (*env)->GetIntArrayElements(env, frame, NULL);
    if (!pixels) return;
    tc_render((uint32_t *)pixels, 480 * 272, (uint32_t)hour, (uint32_t)minute, (uint32_t)second);
    (*env)->ReleaseIntArrayElements(env, frame, pixels, 0);
}
JNIEXPORT void JNICALL Java_cafe_token_android_NativeCafe_touch(JNIEnv *env, jclass cls, jint x, jint y, jboolean scroll) {
    (void)env; (void)cls;
    tc_touch(x, y, scroll == JNI_TRUE);
}
