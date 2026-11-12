CREATE TABLE meta (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    database_id BLOB NOT NULL CHECK(length(database_id) = 16),
    actor_id BLOB NOT NULL CHECK(length(actor_id) = 16),
    default_list BLOB NOT NULL, default_calendar BLOB NOT NULL,
    generation INTEGER NOT NULL CHECK(generation >= 0),
    epoch INTEGER NOT NULL CHECK(epoch >= 0),
    sequence INTEGER NOT NULL CHECK(sequence >= 0)
) STRICT;
CREATE TABLE lists (
    id BLOB PRIMARY KEY CHECK(length(id) = 16), revision INTEGER NOT NULL CHECK(revision > 0),
    created_seq INTEGER NOT NULL, position BLOB, deleted INTEGER NOT NULL CHECK(deleted IN (0,1)), title TEXT,
    CHECK(deleted = 1 OR title IS NOT NULL)
) STRICT;
CREATE TABLE calendars (
    id BLOB PRIMARY KEY CHECK(length(id) = 16), revision INTEGER NOT NULL CHECK(revision > 0),
    created_seq INTEGER NOT NULL, position BLOB, deleted INTEGER NOT NULL CHECK(deleted IN (0,1)), title TEXT,
    CHECK(deleted = 1 OR title IS NOT NULL)
) STRICT;
CREATE TABLE tasks (
    id BLOB PRIMARY KEY CHECK(length(id) = 16), revision INTEGER NOT NULL CHECK(revision > 0),
    created_seq INTEGER NOT NULL, position BLOB, deleted INTEGER NOT NULL CHECK(deleted IN (0,1)),
    title TEXT, list_id BLOB REFERENCES lists(id) DEFERRABLE INITIALLY DEFERRED,
    status INTEGER CHECK(status IN (0,1)), due_date TEXT, due_us INTEGER, due_zone TEXT,
    CHECK(deleted = 1 OR (title IS NOT NULL AND list_id IS NOT NULL AND status IS NOT NULL)),
    CHECK(due_date IS NULL OR due_us IS NULL), CHECK((due_us IS NULL) = (due_zone IS NULL))
) STRICT;
CREATE TABLE events (
    id BLOB PRIMARY KEY CHECK(length(id) = 16), revision INTEGER NOT NULL CHECK(revision > 0),
    created_seq INTEGER NOT NULL, position BLOB, deleted INTEGER NOT NULL CHECK(deleted IN (0,1)),
    title TEXT, calendar_id BLOB REFERENCES calendars(id) DEFERRABLE INITIALLY DEFERRED,
    date_start TEXT, date_end TEXT, start_us INTEGER, end_us INTEGER, zone TEXT,
    CHECK(deleted = 1 OR (title IS NOT NULL AND calendar_id IS NOT NULL AND
        ((date_start IS NOT NULL AND date_start < date_end AND start_us IS NULL AND end_us IS NULL AND zone IS NULL) OR
         (date_start IS NULL AND date_end IS NULL AND start_us IS NOT NULL AND start_us < end_us AND zone IS NOT NULL))))
) STRICT;
CREATE TABLE blocks (
    id BLOB PRIMARY KEY CHECK(length(id) = 16), revision INTEGER NOT NULL CHECK(revision > 0),
    created_seq INTEGER NOT NULL, position BLOB, deleted INTEGER NOT NULL CHECK(deleted IN (0,1)),
    task_id BLOB REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
    calendar_id BLOB REFERENCES calendars(id) DEFERRABLE INITIALLY DEFERRED,
    start_us INTEGER, end_us INTEGER, zone TEXT,
    CHECK(deleted = 1 OR (task_id IS NOT NULL AND calendar_id IS NOT NULL AND start_us IS NOT NULL AND start_us < end_us AND zone IS NOT NULL))
) STRICT;
CREATE INDEX tasks_list_status_created ON tasks(list_id, status, created_seq, id) WHERE deleted = 0;
CREATE INDEX tasks_status_created ON tasks(status, created_seq, id) WHERE deleted = 0;
CREATE INDEX tasks_created ON tasks(created_seq, id) WHERE deleted = 0;
CREATE INDEX tasks_due_date ON tasks(due_date, created_seq, id) WHERE deleted = 0 AND due_date IS NOT NULL;
CREATE INDEX tasks_due_us ON tasks(due_us, created_seq, id) WHERE deleted = 0 AND due_us IS NOT NULL;
CREATE INDEX blocks_task_start ON blocks(task_id, start_us, id) WHERE deleted = 0;
CREATE INDEX blocks_start ON blocks(start_us, id, end_us) WHERE deleted = 0;
CREATE INDEX blocks_end ON blocks(end_us, id) WHERE deleted = 0;
CREATE INDEX blocks_calendar_start ON blocks(calendar_id, start_us, id, end_us) WHERE deleted = 0;
CREATE INDEX events_day_start ON events(date_start, id, date_end) WHERE deleted = 0 AND date_start IS NOT NULL;
CREATE INDEX events_day_end ON events(date_end, id) WHERE deleted = 0 AND date_start IS NOT NULL;
CREATE INDEX events_start ON events(start_us, id, end_us) WHERE deleted = 0 AND start_us IS NOT NULL;
CREATE INDEX events_end ON events(end_us, id) WHERE deleted = 0 AND start_us IS NOT NULL;
CREATE INDEX events_calendar_day ON events(calendar_id, date_start, id, date_end) WHERE deleted = 0 AND date_start IS NOT NULL;
CREATE INDEX events_calendar_start ON events(calendar_id, start_us, id, end_us) WHERE deleted = 0 AND start_us IS NOT NULL;
CREATE INDEX lists_live ON lists(id) WHERE deleted = 0;
CREATE INDEX calendars_live ON calendars(id) WHERE deleted = 0;
CREATE TABLE history (
    seq INTEGER PRIMARY KEY AUTOINCREMENT, operation_id BLOB NOT NULL UNIQUE CHECK(length(operation_id) = 16),
    actor_id BLOB NOT NULL CHECK(length(actor_id) = 16), created_us INTEGER NOT NULL,
    kind INTEGER NOT NULL, focus_kind INTEGER NOT NULL, focus_id BLOB NOT NULL,
    affected INTEGER NOT NULL CHECK(affected BETWEEN 1 AND 129), undone INTEGER NOT NULL CHECK(undone IN (0,1)),
    images BLOB NOT NULL, image_bytes INTEGER NOT NULL CHECK(image_bytes = length(images))
) STRICT;
CREATE INDEX history_actor_state ON history(actor_id, undone, seq);
CREATE INDEX history_created ON history(created_us);
PRAGMA user_version = 1;
