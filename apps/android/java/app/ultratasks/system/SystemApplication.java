package app.ultratasks.system;

import android.app.Application;

/** Initialize Android's certificate verifier before receivers or UI can network. */
public final class SystemApplication extends Application {
    @Override public void onCreate() {
        super.onCreate();
        NativeBridge.initialize(getApplicationContext());
    }
}
