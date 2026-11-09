package app.ultratasks.system;

import java.util.concurrent.CompletableFuture;

/** Actual production lifecycle gate, with deterministic late-completion races. */
public final class JobRunStateTest {
    public static void main(String[] arguments) {
        JobRunState state = new JobRunState();
        int originalJobId = 1000;
        int stoppedJobId = 1000;
        JobRunState.Run stopped = state.start(originalJobId);
        CompletableFuture<Void> stoppedFuture = new CompletableFuture<>();
        state.attach(stopped, stoppedFuture);
        state.stop(stoppedJobId);
        require(stoppedFuture.isCancelled(), "stop matches equivalent job ID from another parameters parcel");
        require(!state.complete(stopped), "stopped run cannot finish or change status");

        JobRunState.Run old = state.start(2000);
        CompletableFuture<Void> oldFuture = new CompletableFuture<>();
        state.attach(old, oldFuture);
        JobRunState.Run current = state.start(2000);
        require(oldFuture.isCancelled(), "replacement cancels old queued future");
        require(!state.complete(old), "old callback cannot finish replacement with same job ID");
        CompletableFuture<Void> currentFuture = new CompletableFuture<>();
        state.attach(current, currentFuture);
        state.stop(9999);
        require(!currentFuture.isCancelled(), "stopping another job cannot cancel active work");
        require(state.complete(current), "active run completes exactly once");
        require(!state.complete(current), "duplicate callback is suppressed");

        JobRunState.Run lateAttach = state.start(3000);
        state.stop(3000);
        CompletableFuture<Void> lateFuture = new CompletableFuture<>();
        state.attach(lateAttach, lateFuture);
        require(lateFuture.isCancelled(), "future attached after stop is immediately canceled");
        JobRunState.Run destroyed = state.start(4000);
        CompletableFuture<Void> destroyedFuture = new CompletableFuture<>();
        state.attach(destroyed, destroyedFuture);
        state.stopAll();
        require(destroyedFuture.isCancelled() && !state.complete(destroyed), "service destruction cancels future and rejects completion");
        System.out.println("Job lifecycle stop/replacement/late-callback/identity tests passed");
    }
    private static void require(boolean condition, String description) {
        if (!condition) throw new AssertionError(description);
    }
}
