package app.ultratasks.system;

import android.Manifest;
import android.app.AlarmManager;
import android.app.Activity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.provider.Settings;
import android.util.AtomicFile;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.time.ZoneId;
import java.util.HashSet;
import java.util.Iterator;
import java.util.Set;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** One process-local owner. Alarm/boot work never blocks the Android main thread. */
public final class ReminderRuntime {
    private static final ExecutorService WORKER = Executors.newSingleThreadExecutor();
    private static final String CHANNEL = "ultratasks.reminders";
    private static final String MAINTENANCE = "_maintenance";
    private ReminderRuntime() {}

    public static void enqueue(Context source, String triggeredId, BroadcastReceiver.PendingResult pending) {
        final Context context = source.getApplicationContext();
        WORKER.execute(() -> {
            try {
                reconcile(context, triggeredId);
                context.getSharedPreferences("device", 0).edit().remove("last_error").apply();
            } catch (Exception | LinkageError error) {
                // Never log task contents. Expose status for the eventual GPUI host.
                context.getSharedPreferences("device", 0).edit().putString("last_error", error.getClass().getSimpleName() + ": " + error.getMessage()).apply();
            } finally {
                if (pending != null) pending.finish();
            }
        });
    }

    /** Call from a background task. All mutations still use Rust commands/Undo. */
    public static String execute(Context context, String commandJson) {
        String result = NativeBridge.dispatch(database(context), commandJson);
        enqueue(context, null, null);
        return result;
    }

    /** Explicit user action from the future native screen; never auto-prompt. */
    public static void requestExactAlarms(Activity activity) {
        if (Build.VERSION.SDK_INT >= 31 && !activity.getSystemService(AlarmManager.class).canScheduleExactAlarms()) {
            activity.startActivity(new Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.parse("package:" + activity.getPackageName())));
        }
    }

    public static void setEnabled(Context context, boolean enabled) {
        context.getSharedPreferences("device", 0).edit().putBoolean("reminders_enabled", enabled).apply();
        enqueue(context, null, null);
    }

    private static String database(Context context) {
        File file = context.getDatabasePath("ultratasks.db");
        File parent = file.getParentFile();
        if (parent != null && !parent.isDirectory() && !parent.mkdirs()) throw new IllegalStateException("Cannot create database directory");
        return file.getAbsolutePath();
    }

    private static void reconcile(Context context, String triggeredId) throws Exception {
        boolean enabled = context.getSharedPreferences("device", 0).getBoolean("reminders_enabled", true);
        String timezone = context.getSharedPreferences("device", 0).getString("timezone", ZoneId.systemDefault().getId());
        boolean delayedAlarm = triggeredId != null && !MAINTENANCE.equals(triggeredId);
        JSONObject request = new JSONObject().put("type", "plan").put("timezone", timezone).put("enabled", enabled).put("missed_grace_minutes", delayedAlarm ? 1440 : 15);
        JSONObject result = new JSONObject(NativeBridge.dispatch(database(context), request.toString()));
        if (!result.getBoolean("ok")) throw new IllegalStateException(result.getString("error"));
        JSONArray planned = result.getJSONArray("notifications");
        JSONArray desired = new JSONArray();
        long now = System.currentTimeMillis();
        for (int i = 0; i < planned.length(); i++) {
            JSONObject value = planned.getJSONObject(i);
            // A real OS alarm delayed by Doze gets up to 24 hours of grace.
            // Other newly found reminders retain the normal 15 minute policy.
            if (Instant.parse(value.getString("fire_at")).toEpochMilli() >= now - 15 * 60 * 1000L || value.getString("id").equals(triggeredId)) desired.put(value);
        }
        if (desired.length() > 4000) throw new IllegalStateException("Notification plan exceeds limit");

        NotificationManager manager = context.getSystemService(NotificationManager.class);
        AlarmManager alarms = context.getSystemService(AlarmManager.class);
        manager.createNotificationChannel(new NotificationChannel(CHANNEL, "UltraTasks reminders", NotificationManager.IMPORTANCE_DEFAULT));
        if (enabled) {
            // Re-check permission/settings and extend the horizon after regrant,
            // including when the activity is never opened again.
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, now + 12 * 60 * 60 * 1000L, alarmIntent(context, MAINTENANCE));
        } else {
            alarms.cancel(alarmIntent(context, MAINTENANCE));
        }
        AtomicFile stateFile = new AtomicFile(new File(context.getFilesDir(), "notification-state.json"));
        JSONObject previous = stateFile.getBaseFile().exists() ? new JSONObject(new String(stateFile.readFully(), StandardCharsets.UTF_8)) : new JSONObject();
        Set<String> desiredIds = new HashSet<>();
        for (int i = 0; i < desired.length(); i++) {
            String id = desired.getJSONObject(i).getString("id");
            if (!desiredIds.add(id)) throw new IllegalStateException("Duplicate reminder identity");
        }
        for (Iterator<String> keys = previous.keys(); keys.hasNext();) {
            String id = keys.next();
            if (!desiredIds.contains(id)) cancel(context, alarms, manager, id);
        }
        if (enabled && (Build.VERSION.SDK_INT >= 33 && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED || !manager.areNotificationsEnabled() || manager.getNotificationChannel(CHANNEL).getImportance() == NotificationManager.IMPORTANCE_NONE)) {
            for (String id : desiredIds) cancel(context, alarms, manager, id);
            throw new SecurityException("Notification permission or channel is disabled");
        }
        JSONObject next = new JSONObject();
        boolean exact = Build.VERSION.SDK_INT < 31 || alarms.canScheduleExactAlarms();
        for (int i = 0; i < desired.length(); i++) {
            JSONObject value = desired.getJSONObject(i);
            String id = value.getString("id");
            String instant = value.getString("fire_at");
            long at = Instant.parse(instant).toEpochMilli();
            JSONObject old = previous.optJSONObject(id);
            boolean shown = old != null && old.optString("fire_at").equals(instant) && old.optBoolean("shown");
            if (at > now) {
                PendingIntent intent = alarmIntent(context, id);
                if (exact) alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, intent);
                else alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, intent);
            } else if (!shown) {
                Notification notification = new Notification.Builder(context, CHANNEL)
                    .setSmallIcon(android.R.drawable.ic_popup_reminder)
                    .setContentTitle(value.getString("title")).setContentText(value.getString("body"))
                    .setAutoCancel(true).setOnlyAlertOnce(true).setCategory(Notification.CATEGORY_REMINDER).build();
                manager.notify(id, 0, notification);
                shown = true;
            }
            next.put(id, new JSONObject().put("fire_at", instant).put("shown", shown));
        }
        FileOutputStream output = null;
        try {
            output = stateFile.startWrite();
            output.write(next.toString().getBytes(StandardCharsets.UTF_8));
            stateFile.finishWrite(output);
        } catch (Exception error) {
            if (output != null) stateFile.failWrite(output);
            throw error;
        }
        context.getSharedPreferences("device", 0).edit().putString("delivery_precision", exact ? "exact_permission_granted" : "inexact_os_alarm").apply();
    }

    private static PendingIntent alarmIntent(Context context, String id) {
        Intent intent = new Intent(context, ReminderReceiver.class).setData(Uri.parse("ultratasks://reminders/" + Uri.encode(id))).putExtra("id", id);
        return PendingIntent.getBroadcast(context, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
    }

    private static void cancel(Context context, AlarmManager alarms, NotificationManager manager, String id) {
        alarms.cancel(alarmIntent(context, id));
        // Already posted notifications stay visible until the user dismisses
        // them; the planner's catch-up window is not an inbox retention policy.
    }
}
