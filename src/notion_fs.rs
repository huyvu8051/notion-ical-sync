//! Notion-backed CalDAV filesystem.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::TimeZone;
use dav_server::{
    davpath::DavPath,
    fs::{self, DavFileSystem, DavMetaData, DavProp, FsError, FsFuture, FsResult, FsStream},
};
use futures_util::{future, FutureExt, StreamExt};
use icalendar::Component;
use parking_lot::Mutex;

use crate::notion::{CalendarInfo, NotionCalendarEvent};

// ─────────────────────────────────────────────
// Tree Store: cached Notion calendars/events
// ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NotionFsTree(pub Arc<Mutex<Vec<(u64, HashMap<String, CalendarInfo>)>>>);

impl NotionFsTree {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }

    pub fn push(&self, map: HashMap<String, CalendarInfo>) -> u64 {
        let mut snap = self.0.lock();
        let idx = snap.len() as u64;
        snap.push((idx, map));
        idx
    }

    pub fn update_last(&self, map: HashMap<String, CalendarInfo>) {
        let mut snap = self.0.lock();
        if snap.is_empty() {
            snap.push((0, map));
        } else {
            let last = snap.len() - 1;
            snap[last] = (snap[last].0, map);
        }
    }

    pub fn latest_cache(&self) -> HashMap<String, CalendarInfo> {
        let snap = self.0.lock();
        snap.last().map(|(_, m)| m.clone()).unwrap_or_default()
    }

    pub fn get_calendar_by_slug(&self, slug: &str) -> Option<(u64, CalendarInfo)> {
        let snap = self.0.lock();
        snap.iter().find_map(|(i, m)| {
            m.values().find(|c| {
                let s = db_id_to_slug(&c.db_id);
                s == slug
            }).map(|c| (*i, c.clone()))
        })
    }

    pub fn get_event_by_slugs(
        &self,
        db_slug: &str,
        event_slug: &str,
    ) -> Option<(u64, CalendarInfo, NotionCalendarEvent)> {
        let snap = self.0.lock();
        snap.iter().find_map(|(i, m)| {
            m.values().find_map(|cal| {
                if db_id_to_slug(&cal.db_id) != db_slug {
                    return None;
                }
                cal.events.values()
                    .find(|ev| event_id_to_slug(&ev.page_id_str) == event_slug)
                    .map(|ev| (*i, cal.clone(), ev.clone()))
            })
        })
    }
}

// ─────────────────────────────────────────────
// Path helpers
// ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum CalPath<'a> {
    CalendarsRoot,
    CalendarRoot { db_slug: std::borrow::Cow<'a, [u8]> },
    Event { db_slug: std::borrow::Cow<'a, [u8]>, event_slug: std::borrow::Cow<'a, [u8]> },
}

fn split_path(path: &DavPath) -> Option<CalPath<'static>> {
    let bytes = path.as_bytes();
    let parts: Vec<&[u8]> = bytes.split(|&b| b == b'/').filter(|s| !s.is_empty()).collect();

    match parts.as_slice() {
        [b"calendars"] => Some(CalPath::CalendarsRoot),
        [b"calendars", db] => Some(CalPath::CalendarRoot {
            db_slug: std::borrow::Cow::Owned(db.to_vec()),
        }),
        [b"calendars", db, ev] => Some(CalPath::Event {
            db_slug: std::borrow::Cow::Owned(db.to_vec()),
            event_slug: std::borrow::Cow::Owned(ev.to_vec()),
        }),
        _ => None,
    }
}

// ─────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────

pub fn db_id_to_slug(db_id: &str) -> String {
    db_id.replace('-', "").to_lowercase()
}

pub fn event_id_to_slug(page_id: &str) -> String {
    page_id.replace('-', "").to_lowercase()
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_secs()
}

pub fn datetime_to_icaldatetime(ts: u64) -> icalendar::CalendarDateTime {
    let sys: SystemTime = UNIX_EPOCH + Duration::from_secs(ts);
    let dt: chrono::DateTime<chrono::Utc> = sys.into();
    icalendar::CalendarDateTime::Utc(dt)
}

pub fn ical_datetime_string(dt: icalendar::CalendarDateTime) -> String {
    match dt {
        icalendar::CalendarDateTime::Utc(dt) => dt.format("%Y%m%dT%H%M%SZ").to_string(),
        icalendar::CalendarDateTime::Floating(dt) => dt.format("%Y%m%dT%H%M%S").to_string(),
        icalendar::CalendarDateTime::WithTimezone { date_time, .. } => date_time.format("%Y%m%dT%H%M%S").to_string(),
    }
}

pub fn make_ics_bytes(cal: &CalendarInfo, ev: &NotionCalendarEvent, now: u64) -> Vec<u8> {
    use icalendar::{Calendar, Component, EventLike, Property};

    let uid = format!("{}-{}", cal.db_id, ev.page_id_str);

    let mut e = icalendar::Event::new();
    e.uid(&uid);
    e.summary(&ev.name);
    e.description(&ev.description);
    e.url(&format!("https://notion.so/{}", ev.page_id_str.replace('-', "")));
    e.append_property(Property::new("X-NOTION-URL", ev.notion_url.clone()));
    e.append_property(Property::new("DTSTAMP", ical_datetime_string(datetime_to_icaldatetime(now))));
    e.starts(datetime_to_icaldatetime(ev.start_timestamp));
    if let Some(end_ts) = ev.end_timestamp {
        e.ends(datetime_to_icaldatetime(end_ts));
    }
    e.status(icalendar::EventStatus::Confirmed);
    e.class(icalendar::Class::Public);

    let mut calendar = Calendar::new();
    calendar.push(e);
    calendar.to_string().into_bytes()
}

pub fn estimate_ics_size(ev: &NotionCalendarEvent) -> usize {
    let base = 400usize;
    base + ev.name.len() + ev.description.len() + ev.page_id_str.len()
}

// ─────────────────────────────────────────────
// MetaData
// ─────────────────────────────────────────────

#[derive(Debug, Clone)]
enum MetaKind {
    Root,
    Calendars,
    Calendar { db_id: String, event_count: usize },
    Ics { len: u64, mtime: SystemTime },
}

#[derive(Debug, Clone)]
pub struct NotionMetaData {
    kind: MetaKind,
}

impl NotionMetaData {
    pub fn root_dir() -> Self {
        Self { kind: MetaKind::Root }
    }
    pub fn calendars() -> Self {
        Self { kind: MetaKind::Calendars }
    }
    pub fn calendar_dir(cal: &CalendarInfo) -> Self {
        Self {
            kind: MetaKind::Calendar {
                db_id: cal.db_id.clone(),
                event_count: cal.events.len(),
            },
        }
    }
    pub fn ics_file(size: u64) -> Self {
        Self {
            kind: MetaKind::Ics { len: size, mtime: SystemTime::now() },
        }
    }
}

impl fs::DavMetaData for NotionMetaData {
    fn len(&self) -> u64 {
        match &self.kind {
            MetaKind::Root | MetaKind::Calendars => 4096,
            MetaKind::Calendar { .. } => 4096,
            MetaKind::Ics { len, .. } => *len,
        }
    }

    fn modified(&self) -> FsResult<SystemTime> {
        match &self.kind {
            MetaKind::Ics { mtime, .. } => Ok(*mtime),
            _ => Ok(SystemTime::now()),
        }
    }

    fn is_dir(&self) -> bool {
        matches!(self.kind, MetaKind::Root | MetaKind::Calendars | MetaKind::Calendar { .. })
    }

    fn is_calendar(&self, path: &DavPath) -> bool {
        let bytes = path.as_bytes();
        let cal_prefix = b"calendars/";
        if bytes.starts_with(cal_prefix) {
            let rest = &bytes[cal_prefix.len()..];
            !rest.contains(&b'/')
        } else {
            false
        }
    }

    fn is_file(&self) -> bool {
        !self.is_dir()
    }
}

// ─────────────────────────────────────────────
// DirEntry – returned from read_dir
// ─────────────────────────────────────────────

#[derive(Debug)]
struct NotionDirEntry {
    name: Vec<u8>,
    is_dir: bool,
    size: u64,
}

impl NotionDirEntry {
    fn new_dir(name: Vec<u8>) -> Self {
        Self { name, is_dir: true, size: 4096 }
    }
    fn new_file(name: Vec<u8>, size: u64) -> Self {
        Self { name, is_dir: false, size }
    }
}

impl fs::DavDirEntry for NotionDirEntry {
    fn name(&self) -> Vec<u8> {
        self.name.clone()
    }

    fn metadata<'a>(&'a self) -> FsFuture<'a, Box<dyn fs::DavMetaData>> {
        let kind = if self.is_dir {
            MetaKind::Calendar { db_id: String::from_utf8_lossy(&self.name).into_owned(), event_count: 0 }
        } else {
            MetaKind::Ics { len: self.size, mtime: SystemTime::now() }
        };
        futures_util::future::ready(Ok(Box::new(NotionMetaData { kind }) as Box<dyn fs::DavMetaData>)).boxed()
    }
}

// ─────────────────────────────────────────────
// DavFile – wraps an in-memory .ics buffer
// ─────────────────────────────────────────────

#[derive(Debug)]
pub struct NotionDavFile {
    data: Vec<u8>,
    pos: usize,
}

impl NotionDavFile {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
}

impl fs::DavFile for NotionDavFile {
    fn metadata<'a>(
        &'a mut self,
    ) -> FsFuture<'a, Box<dyn fs::DavMetaData>> {
        futures_util::future::ready(Ok(Box::new(NotionMetaData::ics_file(self.data.len() as u64)) as Box<dyn fs::DavMetaData>)).boxed()
    }

    fn write_buf<'a>(
        &'a mut self,
        _buf: Box<dyn bytes::Buf + Send>,
    ) -> FsFuture<'a, ()> {
        futures_util::future::ready(Err(FsError::Forbidden)).boxed()
    }

    fn write_bytes<'a>(
        &'a mut self,
        _buf: bytes::Bytes,
    ) -> FsFuture<'a, ()> {
        futures_util::future::ready(Err(FsError::Forbidden)).boxed()
    }

    fn read_bytes<'a>(&'a mut self, count: usize) -> FsFuture<'a, bytes::Bytes> {
        let remaining = &self.data[self.pos..];
        let to_read = count.min(remaining.len());
        let chunk = remaining[..to_read].to_vec();
        self.pos += to_read;
        futures_util::future::ready(Ok(bytes::Bytes::from(chunk))).boxed()
    }

    fn seek<'a>(&'a mut self, pos: std::io::SeekFrom) -> FsFuture<'a, u64> {
        match pos {
            std::io::SeekFrom::Start(n) => self.pos = n as usize,
            std::io::SeekFrom::End(n) => {
                let end = self.data.len();
                self.pos = ((end as i64 + n) as usize).clamp(0, end);
            }
            std::io::SeekFrom::Current(n) => {
                self.pos = ((self.pos as i64 + n) as usize).clamp(0, self.data.len());
            }
        }
        futures_util::future::ready(Ok(self.pos as u64)).boxed()
    }

    fn flush<'a>(&'a mut self) -> FsFuture<'a, ()> {
        futures_util::future::ready(Ok(())).boxed()
    }
}

// ─────────────────────────────────────────────
// NotionDavFs implementation
// ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NotionDavFs {
    events: Arc<NotionFsTree>,
}

impl NotionDavFs {
    pub fn new(events: Arc<NotionFsTree>) -> Self {
        Self { events }
    }
}

impl DavFileSystem for NotionDavFs {
    fn metadata<'a>(
        &'a self,
        path: &'a DavPath,
    ) -> FsFuture<'a, Box<dyn fs::DavMetaData>> {
        async move {
            let bytes = path.as_bytes();

            if bytes.is_empty() || bytes == b"/" {
                return Ok(Box::new(NotionMetaData::root_dir()) as Box<dyn fs::DavMetaData>);
            }

            let cpath = match split_path(path) {
                Some(c) => c,
                None => {
                    if bytes == b"calendars" || bytes == b"/calendars" {
                        return Ok(Box::new(NotionMetaData::calendars()) as Box<dyn fs::DavMetaData>);
                    }
                    return Err(FsError::NotFound);
                }
            };

            match cpath {
                CalPath::CalendarsRoot => {
                    Ok(Box::new(NotionMetaData::calendars()) as Box<dyn fs::DavMetaData>)
                }
                CalPath::CalendarRoot { db_slug } => {
                    let slug_str = String::from_utf8_lossy(&db_slug);
                    let res = self.events.get_calendar_by_slug(&slug_str);
                    let (_, cal) = res.ok_or(FsError::NotFound)?;
                    Ok(Box::new(NotionMetaData::calendar_dir(&cal)) as Box<dyn fs::DavMetaData>)
                }
                CalPath::Event { db_slug, event_slug } => {
                    let db_str = String::from_utf8_lossy(&db_slug);
                    let ev_str = String::from_utf8_lossy(&event_slug);
                    let res = self.events.get_event_by_slugs(&db_str, &ev_str);
                    let (_, cal, ev) = res.ok_or(FsError::NotFound)?;
                    let bytes = make_ics_bytes(&cal, &ev, now_unix());
                    Ok(Box::new(NotionMetaData::ics_file(bytes.len() as u64)) as Box<dyn fs::DavMetaData>)
                }
            }
        }.boxed()
    }

    fn read_dir<'a>(
        &'a self,
        path: &'a DavPath,
        _meta: fs::ReadDirMeta,
    ) -> FsFuture<'a, FsStream<Box<dyn fs::DavDirEntry>>> {
        async move {
            let bytes = path.as_bytes();

            if bytes.is_empty() || bytes == b"/" {
                let entries: Vec<Box<dyn fs::DavDirEntry>> =
                    vec![Box::new(NotionDirEntry::new_dir(b"calendars".to_vec()))];
                return Ok(Box::pin(futures_util::stream::iter(entries.into_iter().map(Ok)))
                    as FsStream<Box<dyn fs::DavDirEntry>>);
            }

            if bytes == b"calendars" || bytes == b"/calendars" {
                let map = self.events.latest_cache();
                let entries: Vec<Box<dyn fs::DavDirEntry>> = map
                    .values()
                    .map(|cal| {
                        Box::new(NotionDirEntry::new_dir(
                            db_id_to_slug(&cal.db_id).into_bytes(),
                        )) as Box<dyn fs::DavDirEntry>
                    })
                    .collect();
                return Ok(Box::pin(futures_util::stream::iter(entries.into_iter().map(Ok)))
                    as FsStream<Box<dyn fs::DavDirEntry>>);
            }

            let cpath = match split_path(path) {
                Some(c) => c,
                None => return Err(FsError::NotFound),
            };

            match cpath {
                CalPath::CalendarRoot { db_slug } => {
                    let slug_str = String::from_utf8_lossy(&db_slug);
                    let (_, cal) = self.events
                        .get_calendar_by_slug(&slug_str)
                        .ok_or(FsError::NotFound)?;

                    let entries: Vec<Box<dyn fs::DavDirEntry>> = cal
                        .events
                        .values()
                        .map(|ev| {
                            let fname = event_id_to_slug(&ev.page_id_str) + ".ics";
                            Box::new(NotionDirEntry::new_file(
                                fname.into_bytes(),
                                estimate_ics_size(ev) as u64,
                            )) as Box<dyn fs::DavDirEntry>
                        })
                        .collect();

                    Ok(Box::pin(futures_util::stream::iter(entries.into_iter().map(Ok)))
                        as FsStream<Box<dyn fs::DavDirEntry>>)
                }
                CalPath::Event { .. } => Err(FsError::Forbidden),
                CalPath::CalendarsRoot => Err(FsError::Forbidden),
            }
        }.boxed()
    }

    fn open<'a>(
        &'a self,
        path: &'a DavPath,
        options: fs::OpenOptions,
    ) -> FsFuture<'a, Box<dyn fs::DavFile>> {
        async move {
            if options.create || options.create_new || options.write || options.append {
                return Err(FsError::Forbidden);
            }
            if !options.read {
                return Err(FsError::Forbidden);
            }

            let cpath = match split_path(path) {
                Some(c) => c,
                None => return Err(FsError::NotFound),
            };

            match cpath {
                CalPath::Event { db_slug, event_slug } => {
                    let db_str = String::from_utf8_lossy(&db_slug);
                    let ev_str = String::from_utf8_lossy(&event_slug);
                    let res = self.events.get_event_by_slugs(&db_str, &ev_str);
                    let (_, cal, ev) = res.ok_or(FsError::NotFound)?;
                    let bytes = make_ics_bytes(&cal, &ev, now_unix());
                    Ok(Box::new(NotionDavFile::new(bytes)) as Box<dyn fs::DavFile>)
                }
                _ => Err(FsError::NotFound),
            }
        }.boxed()
    }
}
