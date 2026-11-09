package app.ultratasks.system;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.concurrent.Callable;
import java.util.concurrent.Executors;

/** Runs the real JNI bridge in the host JVM against temporary SQLite files. */
public final class BridgeSmoke {
    public static void main(String[] arguments) throws Exception {
        Path directory = Files.createTempDirectory("ultratasks-jni-");
        String path = directory.resolve("test.sqlite3").toString();
        String command = create("jni-smoke-1", "JNI 日本語");
        String first = NativeBridge.dispatch(path, command);
        require(ok(first) && first.contains("JNI 日本語"), "create through JNI");
        require(NativeBridge.dispatch(path, command).equals(first), "persistent idempotency through JNI");
        require(NativeBridge.dispatch(path, "{\"type\":\"snapshot\"}").contains("JNI 日本語"), "read persisted data");
        require(ok(NativeBridge.dispatch(path, "{\"type\":\"undo\"}")), "undo through JNI");
        require(NativeBridge.dispatch(path, "{\"type\":\"snapshot\"}").contains("\"deleted\":true"), "undo persisted tombstone");
        require(ok(NativeBridge.dispatch(path, "{\"type\":\"redo\"}")), "redo through JNI");
        require(NativeBridge.dispatch(path, "{\"type\":\"snapshot\"}").contains("\"deleted\":false"), "redo restored record");
        require(NativeBridge.dispatch(path, "{\"type\":\"history\"}").contains("\"can_undo\":true"), "history availability");
        require(NativeBridge.dispatch(path, "{\"type\":\"plan\",\"timezone\":\"Asia/Tokyo\",\"enabled\":true}").contains("\"notifications\":[]"), "planner integration");
        require(!ok(NativeBridge.dispatch(path, "{\"type\":\"execute\",\"request_id\":\"jni-smoke-1\",\"commands\":[]}")), "request ID collision rejection");
        require(!ok(NativeBridge.dispatch(path, "invalid JSON")), "malformed requests become structured errors");

        String exported = NativeBridge.dispatch(path, "{\"type\":\"export\"}");
        require(ok(exported) && exported.contains("\"format\":\"ultratasks\""), "versioned JSON export");
        String data = exported.substring(exported.indexOf("\"data\":") + 7, exported.lastIndexOf(",\"ok\":true}"));
        String importedPath = directory.resolve("imported.sqlite3").toString();
        String imported = NativeBridge.dispatch(importedPath, "{\"type\":\"import\",\"require_empty\":true,\"data\":" + data + "}");
        require(ok(imported) && imported.contains("\"inserted\":1"), "versioned JSON import into independent DB");
        require(NativeBridge.dispatch(importedPath, "{\"type\":\"export\"}").equals(exported), "IDs and revisions preserved through JNI JSON transfer");
        require(NativeBridge.dispatch(path, "{\"type\":\"export_ics\"}").contains("BEGIN:VTODO"), "ICS export through JNI");
        String ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//JNI//EN\r\nBEGIN:VEVENT\r\nUID:jni@example.test\r\nSUMMARY:ICS 予定\r\nDTSTART;VALUE=DATE:20261109\r\nDTEND;VALUE=DATE:20261110\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        require(ok(NativeBridge.dispatch(importedPath, "{\"type\":\"import_ics\",\"data\":" + quote(ics) + "}")), "ICS import through JNI");

        require(ok(NativeBridge.dispatch(path, "{\"type\":\"backup\",\"filename\":\"backup.sqlite3\"}")), "online backup through JNI");
        require(!ok(NativeBridge.dispatch(path, "{\"type\":\"backup\",\"filename\":\"backup.sqlite3\"}")), "backup never overwrites");
        require(ok(NativeBridge.dispatch(path, "{\"type\":\"restore\",\"filename\":\"backup.sqlite3\",\"destination\":\"restored.sqlite3\"}")), "restore to confined new database");
        require(!ok(NativeBridge.dispatch(path, "{\"type\":\"restore\",\"filename\":\"backup.sqlite3\",\"destination\":\"restored.sqlite3\"}")), "restore never overwrites");
        require(NativeBridge.dispatch(directory.resolve("restored.sqlite3").toString(), "{\"type\":\"snapshot\"}").contains("JNI 日本語"), "restored state opens through JNI");
        for (String name : new String[]{"../escape.sqlite3", "/tmp/escape.sqlite3", "..\\escape.sqlite3", "backup/child.sqlite3"}) {
            require(!ok(NativeBridge.dispatch(path, "{\"type\":\"backup\",\"filename\":" + quote(name) + "}")), "reject path traversal: " + name);
        }
        Path link = directory.resolve("link.sqlite3");
        try {
            Files.createSymbolicLink(link, directory.resolve("backup.sqlite3"));
            require(!ok(NativeBridge.dispatch(path, "{\"type\":\"restore\",\"filename\":\"link.sqlite3\",\"destination\":\"link-copy.sqlite3\"}")), "reject symlink backup source");
        } catch (UnsupportedOperationException | java.nio.file.FileSystemException unsupported) {
            // Windows without symlink privileges may skip only this filesystem probe.
        }

        require(ok(NativeBridge.dispatchReadOnly(path, "{\"type\":\"snapshot\"}")), "read-only query succeeds");
        for (String mutation : new String[]{command, "{\"type\":\"undo\"}", "{\"type\":\"redo\"}", "{\"type\":\"backup\",\"filename\":\"blocked.sqlite3\"}", "{\"type\":\"import\",\"data\":" + data + "}", "{\"type\":\"restore\",\"filename\":\"backup.sqlite3\",\"destination\":\"blocked.sqlite3\"}"}) {
            require(!ok(NativeBridge.dispatchReadOnly(path, mutation)), "read-only entry rejects mutation");
        }
        require(!Files.exists(directory.resolve("blocked.sqlite3")), "read-only entry has no file side effect");
        String absent = directory.resolve("absent.sqlite3").toString();
        require(!ok(NativeBridge.dispatchReadOnly(absent, "{\"type\":\"snapshot\"}")) && !Files.exists(Path.of(absent)), "read-only query never creates a missing DB");
        require(NativeBridge.dispatch(path, "x".repeat(32 * 1024 * 1024 + 64 * 1024 + 1)).contains("request exceeds"), "request byte limit");

        require(!ok(NativeBridge.dispatchReadOnly(path, "{\"type\":\"network\",\"request\":{\"operation\":\"sync_status\"}}")), "read-only entry rejects network operations");
        String insecureSync = "{\"type\":\"network\",\"request\":{\"operation\":\"sync\",\"endpoint\":\"http://example.invalid/\",\"token\":\"" + "a".repeat(64) + "\"}}";
        require(NativeBridge.dispatch(path, insecureSync).contains("requires HTTPS"), "insecure sync endpoint rejected without a request");
        String unapprovedAi = "{\"type\":\"network\",\"request\":{\"operation\":\"ai_apply\",\"approved\":false,\"plan\":{\"format_version\":1,\"request_id\":\"jni-ai\",\"summary\":\"proposal\",\"clock\":{\"now\":\"2026-11-09T00:00:00Z\",\"timezone\":\"UTC\"},\"scope\":[],\"commands\":[]}}}";
        require(NativeBridge.dispatch(path, unapprovedAi).contains("explicit approval"), "AI apply requires approval before changing data");

        var executor = Executors.newFixedThreadPool(4);
        try {
            var requests = new ArrayList<Callable<String>>();
            for (int i = 0; i < 12; i++) {
                final int index = i;
                requests.add(() -> NativeBridge.dispatch(path, create("jni-concurrent-" + index, "parallel-" + index)));
            }
            for (var result : executor.invokeAll(requests)) require(ok(result.get()), "serialized concurrent JNI writes");
        } finally { executor.shutdown(); }
        String concurrent = NativeBridge.dispatch(path, "{\"type\":\"snapshot\"}");
        for (int i = 0; i < 12; i++) require(concurrent.contains("\"title\":\"parallel-" + i + "\""), "concurrent write durable " + i);
        System.out.println("JNI commands/Undo/JSON/ICS/backups/readonly/bounds/concurrency smoke passed");
    }
    private static String create(String id, String title) {
        return "{\"type\":\"execute\",\"request_id\":" + quote(id) + ",\"commands\":[{\"CreateTask\":{\"title\":" + quote(title) + ",\"due\":null,\"list_id\":null}}]}";
    }
    private static String quote(String value) { return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"").replace("\r", "\\r").replace("\n", "\\n") + "\""; }
    private static boolean ok(String value) { return value.contains("\"ok\":true"); }
    private static void require(boolean value, String description) { if (!value) throw new AssertionError(description); }
}
