package app.ultratasks.system;

/** The app's private database path is supplied by the host, never by an intent. */
public final class NativeBridge {
    static { System.loadLibrary("ultratasks_android"); }
    private NativeBridge() {}
    /** Pass the real Application Context once, before any TLS networking. */
    public static native void initialize(Object applicationContext);
    public static native String dispatch(String databasePath, String requestJson);
    /** The host chooses this entry point; JSON cannot grant itself write access. */
    public static native String dispatchReadOnly(String databasePath, String requestJson);
}
