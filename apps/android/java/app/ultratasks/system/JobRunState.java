package app.ultratasks.system;

import java.util.concurrent.CompletableFuture;

/** Job identity and completion gate, independent of Android for deterministic tests. */
final class JobRunState {
    static final class Run {
        final int jobId;
        private CompletableFuture<?> future;
        Run(int jobId) { this.jobId = jobId; }
    }
    private Run active;

    synchronized Run start(int jobId) {
        cancelActive();
        active = new Run(jobId);
        return active;
    }

    synchronized void attach(Run run, CompletableFuture<?> future) {
        if (active == run) run.future = future;
        else future.cancel(false);
    }

    synchronized boolean complete(Run run) {
        if (active != run) return false;
        active = null;
        return true;
    }

    synchronized void stop(int jobId) {
        if (active != null && active.jobId == jobId) cancelActive();
    }

    synchronized void stopAll() { cancelActive(); }

    private void cancelActive() {
        Run previous = active;
        active = null;
        if (previous != null && previous.future != null) previous.future.cancel(false);
    }
}
