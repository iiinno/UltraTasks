package app.ultratasks.system;

import android.app.job.JobParameters;
import android.app.job.JobService;
import android.os.Handler;
import android.os.Looper;
import java.util.concurrent.CompletableFuture;
import org.json.JSONObject;

/** Android decides execution time under Doze; this is eventual, not exact, sync. */
public final class SyncJobService extends JobService {
    private final JobRunState runs = new JobRunState();
    private final Handler main = new Handler(Looper.getMainLooper());

    @Override public boolean onStartJob(JobParameters parameters) {
        final JobRunState.Run run = runs.start(parameters.getJobId());
        final long generation = SystemController.syncGeneration(this);
        CompletableFuture<JSONObject> future = SystemController.syncNow(this);
        runs.attach(run, future);
        future.whenComplete((result, error) -> main.post(() -> {
            if (!runs.complete(run)) return;
            boolean success = error == null && result != null && result.optBoolean("ok", false);
            SystemController.recordSyncStatus(this, generation, success ? "ok" : "retry_required");
            jobFinished(parameters, !success);
        }));
        return true;
    }

    @Override public boolean onStopJob(JobParameters parameters) {
        // Cancels work still queued in Java and suppresses stale callbacks. Java
        // cancellation does not interrupt running work: an active request may
        // finish within the native time budget. Durable IDs make retry safe.
        runs.stop(parameters.getJobId());
        return true;
    }

    @Override public void onDestroy() {
        runs.stopAll();
        super.onDestroy();
    }
}
