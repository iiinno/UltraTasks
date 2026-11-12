use gpui_kit::{ListOffset, ListState};
use std::collections::HashSet;
use ultratasks_core::*;

#[derive(Clone, Copy)]
pub enum PageMode {
    Replace,
    Next,
    Previous,
}
#[derive(Default)]
pub struct TaskCache {
    pub rows: Vec<TaskRow>,
    pub total: u64,
    pub previous: Option<TaskCursor>,
    pub next: Option<TaskCursor>,
    pub earlier: bool,
    pub later: bool,
    pub generation: u64,
    pub held_done: HashSet<TaskId>,
    pub loaded: bool,
}
impl TaskCache {
    pub fn visible_selection(&self, selected: Option<TaskId>) -> Option<TaskId> {
        selected.filter(|id| self.loaded && self.rows.iter().any(|row| row.task.key.id == *id))
    }
    pub fn invalidate(&mut self) {
        self.loaded = false;
        self.previous = None;
        self.next = None;
        self.earlier = false;
        self.later = false;
        self.held_done.clear();
    }
    pub fn anchor(&self, list: &ListState) -> Option<(TaskId, ListOffset)> {
        let offset = list.logical_scroll_top();
        self.rows
            .get(offset.item_ix)
            .map(|row| (row.task.key.id, offset))
    }
    pub fn accept(&mut self, page: TaskPage, mode: PageMode, list: &ListState) {
        let anchor = self.anchor(list);
        let old = self.rows.clone();
        self.total = page.total;
        self.generation = page.generation.get();
        self.loaded = true;
        match mode {
            PageMode::Replace => {
                self.rows = page.items;
                self.previous = page.previous;
                self.next = page.next;
                self.earlier = self.previous.is_some();
                self.later = self.next.is_some();
            }
            PageMode::Next => {
                self.next = page.next;
                self.later = self.next.is_some();
                for row in page.items {
                    if !self
                        .rows
                        .iter()
                        .any(|old| old.task.key.id == row.task.key.id)
                    {
                        self.rows.push(row);
                    }
                }
                if self.rows.len() > 600 {
                    let count = self.rows.len() - 600;
                    self.rows.drain(..count);
                    self.previous = None;
                    self.earlier = true;
                }
            }
            PageMode::Previous => {
                self.previous = page.previous;
                self.earlier = self.previous.is_some();
                let mut rows = page.items;
                rows.retain(|row| {
                    !self
                        .rows
                        .iter()
                        .any(|old| old.task.key.id == row.task.key.id)
                });
                rows.append(&mut self.rows);
                self.rows = rows;
                if self.rows.len() > 600 {
                    self.rows.truncate(600);
                    self.next = None;
                    self.later = true;
                }
            }
        }
        reconcile(&old, &self.rows, list);
        self.restore(anchor, list);
    }
    pub fn refresh(&mut self, refresh: TaskRefresh, list: &ListState) {
        if !self.loaded {
            return;
        }
        let anchor = self.anchor(list);
        let old = self.rows.clone();
        self.total = refresh.total;
        self.generation = refresh.generation.get();
        for row in refresh.rows {
            let id = row.task.key.id;
            let matching = refresh.matching_ids.contains(&id);
            if row.task.data.status == TaskStatus::Open {
                self.held_done.remove(&id);
            }
            if let Some(index) = self.rows.iter().position(|r| r.task.key.id == id) {
                if row.task.data.status == TaskStatus::Done
                    && self.rows[index].task.data.status == TaskStatus::Open
                {
                    self.held_done.insert(id);
                }
                if matching || self.held_done.contains(&id) {
                    self.rows[index] = row;
                } else {
                    self.rows.remove(index);
                }
            } else if matching && !self.later && self.rows.len() < 600 {
                self.rows.push(row);
            }
        }
        self.previous = None;
        self.next = None;
        reconcile(&old, &self.rows, list);
        self.restore(anchor, list);
    }
    pub fn delete(&mut self, id: TaskId, list: &ListState) {
        if let Some(index) = self.rows.iter().position(|r| r.task.key.id == id) {
            self.rows.remove(index);
            list.splice(index..index + 1, 0);
        }
        self.held_done.remove(&id);
    }
    fn restore(&self, anchor: Option<(TaskId, ListOffset)>, list: &ListState) {
        if let Some((id, mut offset)) = anchor
            && let Some(index) = self.rows.iter().position(|r| r.task.key.id == id)
        {
            offset.item_ix = index;
            list.scroll_to(offset);
        }
    }
}
fn reconcile(old: &[TaskRow], new: &[TaskRow], list: &ListState) {
    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(a, b)| a.task.key.id == b.task.key.id)
        .count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a.task.key.id == b.task.key.id)
        .count();
    if old.len() - prefix - suffix > 0 || new.len() - prefix - suffix > 0 {
        list.splice(prefix..old.len() - suffix, new.len() - prefix - suffix);
    }
    for (index, (a, b)) in old.iter().zip(new).enumerate() {
        if a.task.key.id == b.task.key.id && a != b {
            list.splice(index..index + 1, 1);
        }
    }
}
pub fn subtitle(row: &TaskRow, zone: &ZoneId) -> String {
    if row.blocks.in_window > 0 {
        if row.blocks.in_window == 1
            && let Some(span) = &row.blocks.first_in_window
        {
            return format!(
                "{} · {}",
                super::agenda::span_label(span, zone),
                row.list_title.as_str()
            );
        }
        format!(
            "作業枠{}件 · 計{}分 · {}",
            row.blocks.in_window,
            row.blocks.allocated_micros_in_window / 60_000_000,
            row.list_title.as_str()
        )
    } else {
        let due = match &row.task.data.due {
            None => "期限なし".into(),
            Some(Due::Date(d)) => format!("{}まで", d.iso()),
            Some(Due::At(z)) => format!("{}まで", super::agenda::instant_label(z.instant, zone)),
        };
        format!("{} · {due}", row.list_title.as_str())
    }
}
