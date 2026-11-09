package app.ultratasks.system;

import android.app.job.JobInfo;
import android.app.job.JobScheduler;
import android.content.ComponentName;
import android.content.Context;
import android.content.SharedPreferences;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.Iterator;
import java.util.Map;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.Callable;
import java.util.concurrent.CancellationException;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.RejectedExecutionException;
import java.util.concurrent.Semaphore;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** System-only entry points for the future GPUI host. No network call runs on UI. */
public final class SystemController {
    private static final int SYNC_JOB = 0x555453;
    private static final int MAX_NETWORK_JSON = 1024 * 1024;
    private static final Object SYNC_CONFIGURATION = new Object();
    private static final Semaphore SYNC_REQUEST = new Semaphore(1);
    private static final ThreadPoolExecutor NETWORK = new ThreadPoolExecutor(2, 2, 0L,
        TimeUnit.MILLISECONDS, new ArrayBlockingQueue<Runnable>(8), new ThreadPoolExecutor.AbortPolicy());
    private SystemController() {}

    /** Call from a setup worker after user configuration; credentials are stored separately. */
    public static void configureSync(Context context, String endpoint, String credentialName) throws Exception {
        if (endpoint == null || endpoint.length() > 2048) throw new IllegalArgumentException("Invalid sync endpoint length");
        URI uri = new URI(endpoint);
        if (!"https".equals(uri.getScheme()) || uri.getHost() == null || uri.getUserInfo() != null ||
            uri.getRawQuery() != null || uri.getRawFragment() != null) {
            throw new IllegalArgumentException("Sync endpoint must use HTTPS without credentials or query");
        }
        CredentialStore.get(context, credentialName);
        synchronized (SYNC_CONFIGURATION) {
            SharedPreferences preferences = context.getSharedPreferences("sync", Context.MODE_PRIVATE);
            long next = nextGeneration(preferences.getAll());
            if (!preferences.edit().putString("endpoint", endpoint).putString("credential", credentialName)
                .putLong("generation", next).commit()) {
                throw new IllegalStateException("Sync settings could not be saved");
            }
            JobInfo job = new JobInfo.Builder(SYNC_JOB, new ComponentName(context, SyncJobService.class))
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY).setPersisted(true)
                .setPeriodic(15 * 60 * 1000L).setBackoffCriteria(30 * 1000L, JobInfo.BACKOFF_POLICY_EXPONENTIAL).build();
            try {
                if (context.getSystemService(JobScheduler.class).schedule(job) != JobScheduler.RESULT_SUCCESS) {
                    throw new IllegalStateException("JobScheduler rejected the job");
                }
                context.getSharedPreferences("device", Context.MODE_PRIVATE).edit().putString("last_sync_status", "scheduled").apply();
            } catch (RuntimeException error) {
                // The saved configuration remains usable for explicit sync. Do not
                // claim background registration succeeded or silently lose settings.
                context.getSharedPreferences("device", Context.MODE_PRIVATE).edit().putString("last_sync_status", "background_schedule_failed").apply();
                throw new IllegalStateException("Sync settings saved, but background scheduling failed; manual sync remains available", error);
            }
        }
    }

    /** Stops queued configured sync. A native request already in flight may finish. */
    public static void disableSync(Context context) {
        synchronized (SYNC_CONFIGURATION) {
            SharedPreferences preferences = context.getSharedPreferences("sync", Context.MODE_PRIVATE);
            long next = nextGeneration(preferences.getAll());
            if (!preferences.edit().clear().putLong("generation", next).commit()) {
                throw new IllegalStateException("Sync settings could not be removed");
            }
            context.getSystemService(JobScheduler.class).cancel(SYNC_JOB);
            context.getSharedPreferences("device", Context.MODE_PRIVATE).edit().putString("last_sync_status", "disabled").apply();
        }
    }

    public static CompletableFuture<JSONObject> syncNow(Context source) {
        Context context = source.getApplicationContext();
        final SyncConfiguration configuration;
        try {
            synchronized (SYNC_CONFIGURATION) { configuration = readConfiguration(context); }
        } catch (Exception error) { return failed(error); }
        return submit(() -> {
            synchronized (SYNC_CONFIGURATION) { requireCurrent(context, configuration); }
            JSONObject request = new JSONObject().put("operation", "sync").put("endpoint", configuration.endpoint)
                .put("token", CredentialStore.get(context, configuration.credential));
            return dispatch(context, request, configuration);
        });
    }

    /** Only the wrapping AES key stays in KeyStore; credentials are decrypted for this call. */
    public static CompletableFuture<JSONObject> proposeAi(Context source, JSONObject request, String credentialName) {
        Context context = source.getApplicationContext();
        final JSONObject copy;
        try { copy = copyRequest(request); } catch (Exception error) { return failed(error); }
        return submit(() -> {
            copy.put("operation", "ai_propose");
            copy.put("api_key", credentialName == null ? JSONObject.NULL : CredentialStore.get(context, credentialName));
            return dispatch(context, copy, null);
        });
    }

    /** Scope/approval flags remain enforced by Rust; remote responses never apply themselves. */
    public static CompletableFuture<JSONObject> network(Context source, JSONObject request) {
        Context context = source.getApplicationContext();
        final JSONObject copy;
        try { copy = copyRequest(request); } catch (Exception error) { return failed(error); }
        return submit(() -> dispatch(context, copy, null));
    }

    static long syncGeneration(Context context) {
        synchronized (SYNC_CONFIGURATION) { return generation(context.getSharedPreferences("sync", Context.MODE_PRIVATE).getAll()); }
    }

    static void recordSyncStatus(Context context, long expectedGeneration, String status) {
        synchronized (SYNC_CONFIGURATION) {
            if (generation(context.getSharedPreferences("sync", Context.MODE_PRIVATE).getAll()) == expectedGeneration) {
                context.getSharedPreferences("device", Context.MODE_PRIVATE).edit().putString("last_sync_status", status).apply();
            }
        }
    }

    private static CompletableFuture<JSONObject> submit(Callable<JSONObject> operation) {
        CompletableFuture<JSONObject> result = new CompletableFuture<>();
        try {
            NETWORK.execute(() -> {
                if (result.isCancelled()) return;
                try { result.complete(operation.call()); }
                catch (Exception | LinkageError error) { result.completeExceptionally(error); }
            });
        } catch (RejectedExecutionException busy) {
            result.completeExceptionally(new IllegalStateException("Network request queue is full; retry after an active request completes"));
        }
        return result;
    }

    private static CompletableFuture<JSONObject> failed(Exception error) {
        CompletableFuture<JSONObject> result = new CompletableFuture<>();
        result.completeExceptionally(error);
        return result;
    }

    private static JSONObject dispatch(Context context, JSONObject request, SyncConfiguration expected) throws Exception {
        String operation = request.getString("operation");
        boolean serialSync = operation.equals("sync") || operation.equals("recover_server") || operation.equals("resolve_conflict");
        if (serialSync && !SYNC_REQUEST.tryAcquire()) throw new IllegalStateException("A synchronization operation is already running");
        try {
            if (expected != null) {
                // Read endpoint and token-name from one immutable preference
                // snapshot, then recheck its generation immediately before dispatch.
                synchronized (SYNC_CONFIGURATION) { requireCurrent(context, expected); }
            }
            JSONObject envelope = new JSONObject().put("type", "network").put("request", request);
            String encoded = envelope.toString();
            if (encoded == null || encoded.length() > MAX_NETWORK_JSON + 16 * 1024 || encoded.getBytes(StandardCharsets.UTF_8).length > MAX_NETWORK_JSON + 16 * 1024) throw new IllegalArgumentException("Network envelope is too large");
            return new JSONObject(ReminderRuntime.execute(context, encoded));
        } finally { if (serialSync) SYNC_REQUEST.release(); }
    }

    private static SyncConfiguration readConfiguration(Context context) {
        // SharedPreferences.getAll returns a coherent copy. Separate getString
        // calls can pair an old endpoint with a newly configured credential.
        Map<String, ?> snapshot = context.getSharedPreferences("sync", Context.MODE_PRIVATE).getAll();
        Object endpoint = snapshot.get("endpoint");
        Object credential = snapshot.get("credential");
        if (!(endpoint instanceof String) || !(credential instanceof String)) throw new IllegalStateException("Sync is not configured");
        return new SyncConfiguration((String) endpoint, (String) credential, generation(snapshot));
    }

    private static void requireCurrent(Context context, SyncConfiguration expected) {
        SyncConfiguration current = readConfiguration(context);
        if (current.generation != expected.generation || !current.endpoint.equals(expected.endpoint) || !current.credential.equals(expected.credential)) {
            throw new CancellationException("Sync configuration changed before the request started");
        }
    }

    private static long generation(Map<String, ?> snapshot) {
        Object value = snapshot.get("generation");
        if (value == null) return 0;
        if (!(value instanceof Long) || ((Long) value) < 0) throw new IllegalStateException("Invalid sync configuration generation");
        return (Long) value;
    }

    private static long nextGeneration(Map<String, ?> snapshot) {
        long value = generation(snapshot);
        if (value == Long.MAX_VALUE) throw new IllegalStateException("Sync configuration generation exhausted");
        return value + 1;
    }

    private static final class SyncConfiguration {
        final String endpoint;
        final String credential;
        final long generation;
        SyncConfiguration(String endpoint, String credential, long generation) {
            this.endpoint = endpoint; this.credential = credential; this.generation = generation;
        }
    }

    private static JSONObject copyRequest(JSONObject input) throws JSONException {
        if (input == null) throw new IllegalArgumentException("Missing network request");
        return (JSONObject) copyJson(input, new JsonBudget(), 0);
    }

    /** Bound the walk and copy before serialization or queueing caller-owned data. */
    private static Object copyJson(Object value, JsonBudget budget, int depth) throws JSONException {
        if (depth > 32 || ++budget.nodes > 20000) throw new IllegalArgumentException("Network JSON is too deeply nested or complex");
        if (value == JSONObject.NULL || value == null) { budget.take(4); return JSONObject.NULL; }
        if (value instanceof String) { budget.string((String) value); return value; }
        if (value instanceof Boolean) { budget.take(5); return value; }
        if (value instanceof Number) {
            if (!(value instanceof Byte || value instanceof Short || value instanceof Integer || value instanceof Long || value instanceof Float || value instanceof Double)) {
                throw new IllegalArgumentException("Unsupported network JSON number");
            }
            double number = ((Number) value).doubleValue();
            if (Double.isNaN(number) || Double.isInfinite(number)) throw new IllegalArgumentException("Invalid network JSON number");
            budget.take(value.toString().length()); return value;
        }
        if (value instanceof JSONObject) {
            JSONObject source = (JSONObject) value;
            JSONObject copy = new JSONObject();
            budget.take(2);
            for (Iterator<String> keys = source.keys(); keys.hasNext();) {
                String key = keys.next(); budget.string(key); budget.take(2);
                copy.put(key, copyJson(source.get(key), budget, depth + 1));
            }
            return copy;
        }
        if (value instanceof JSONArray) {
            JSONArray source = (JSONArray) value;
            if (source.length() > 20000) throw new IllegalArgumentException("Network array is too large");
            JSONArray copy = new JSONArray(); budget.take(2);
            for (int i = 0; i < source.length(); i++) { budget.take(1); copy.put(copyJson(source.get(i), budget, depth + 1)); }
            return copy;
        }
        throw new IllegalArgumentException("Unsupported network JSON value");
    }

    private static final class JsonBudget {
        int remaining = MAX_NETWORK_JSON;
        int nodes;
        void take(int bytes) {
            if (bytes < 0 || bytes > remaining) throw new IllegalArgumentException("Network JSON exceeds 1 MiB");
            remaining -= bytes;
        }
        void string(String text) {
            if (text.length() > remaining) throw new IllegalArgumentException("Network JSON exceeds 1 MiB");
            take(2);
            for (int index = 0; index < text.length(); index++) {
                char ch = text.charAt(index);
                if (ch == '"' || ch == '\\') take(2);
                else if (ch < 0x20) take(6);
                else if (ch < 0x80) take(1);
                else if (ch < 0x800) take(2);
                else if (Character.isHighSurrogate(ch)) {
                    if (++index >= text.length() || !Character.isLowSurrogate(text.charAt(index))) throw new IllegalArgumentException("Unpaired Unicode surrogate");
                    take(4);
                } else if (Character.isLowSurrogate(ch)) throw new IllegalArgumentException("Unpaired Unicode surrogate");
                else take(3);
            }
        }
    }
}
