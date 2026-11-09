-- Frozen schema from a16055c, before recurrence and sync migrations.
CREATE TABLE task_lists (
          id TEXT PRIMARY KEY, title TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), deleted INTEGER NOT NULL CHECK(deleted IN (0,1))
        );
        CREATE TABLE tasks (
          id TEXT PRIMARY KEY, title TEXT NOT NULL, completed INTEGER NOT NULL CHECK(completed IN (0,1)), due_json TEXT,
          list_id TEXT REFERENCES task_lists(id), revision INTEGER NOT NULL CHECK(revision>0), deleted INTEGER NOT NULL CHECK(deleted IN (0,1))
        );
        CREATE INDEX tasks_active ON tasks(deleted,completed,id);
        CREATE INDEX tasks_list ON tasks(list_id,deleted,id);
        CREATE TABLE events (
          id TEXT PRIMARY KEY, title TEXT NOT NULL, schedule_json TEXT NOT NULL,
          start_utc TEXT, end_utc TEXT, start_date TEXT, end_date TEXT,
          revision INTEGER NOT NULL CHECK(revision>0), deleted INTEGER NOT NULL CHECK(deleted IN (0,1)),
          CHECK((start_utc IS NOT NULL AND end_utc IS NOT NULL AND start_date IS NULL AND end_date IS NULL) OR
                (start_utc IS NULL AND end_utc IS NULL AND start_date IS NOT NULL AND end_date IS NOT NULL))
        );
        CREATE INDEX events_timed ON events(deleted,start_utc,end_utc);
        CREATE INDEX events_all_day ON events(deleted,start_date,end_date);
        CREATE TABLE time_blocks (
          id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id), start_utc TEXT NOT NULL, end_utc TEXT NOT NULL,
          timezone TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), deleted INTEGER NOT NULL CHECK(deleted IN (0,1)), CHECK(start_utc<end_utc)
        );
        CREATE INDEX blocks_window ON time_blocks(deleted,start_utc,end_utc);
        CREATE INDEX blocks_task ON time_blocks(task_id,deleted);
        CREATE TABLE operations (
          seq INTEGER PRIMARY KEY AUTOINCREMENT, actor TEXT NOT NULL, before_json TEXT NOT NULL, after_json TEXT NOT NULL,
          undone INTEGER NOT NULL DEFAULT 0 CHECK(undone IN (0,1))
        );
        CREATE INDEX operations_actor ON operations(actor,undone,seq);
        CREATE TABLE history_guards (
          actor TEXT NOT NULL, kind TEXT NOT NULL, entity_id TEXT NOT NULL, revision INTEGER NOT NULL,
          PRIMARY KEY(actor,kind,entity_id)
        );
        PRAGMA user_version=1;
CREATE INDEX tasks_order ON tasks(deleted,id);
PRAGMA user_version=2;
PRAGMA application_id=1431587667;
