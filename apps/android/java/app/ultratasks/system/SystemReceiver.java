package app.ultratasks.system;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

public final class SystemReceiver extends BroadcastReceiver {
    @Override public void onReceive(Context context, Intent intent) {
        ReminderRuntime.enqueue(context, null, goAsync());
    }
}
