package cafe.token.android;

final class NativeCafe {
    static { System.loadLibrary("token_cafe_jni"); }
    static native void reset(boolean demo);
    static native boolean snapshot(byte[] json);
    static native void render(int[] frame, int hour, int minute, int second);
    static native void touch(int x, int y, boolean scroll);
    private NativeCafe() {}
}
