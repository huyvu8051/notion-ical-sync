//! Which Notion property backs each calendar field.
//!
//! Before this existed the names were hardcoded English literals scattered
//! through `caldav.rs` (`"Location"`, `"Notes"`, `"Show As"`, …), matched
//! case-insensitively. That silently dropped every field for the majority of
//! users, whose columns are named in German, Korean, Chinese or Spanish, and
//! it made writes fail outright whenever the title column was not literally
//! `"Name"`.
//!
//! A calendar whose mapping has never been configured keeps the old guesses —
//! see [`ColumnMapping::name_for`] — so rows predating the feature behave
//! exactly as before until the backfill configures them.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Every `Priority` option this app creates, paired with the iCalendar
/// PRIORITY value it reads back as.
///
/// One list instead of two, because the schema we hand Notion and the parser
/// that reads it back are the same fact stated twice. An option the parser
/// does not recognise is a column that looks configured and syncs nothing,
/// and nothing would have caught the drift.
pub const PRIORITY_OPTIONS: [(&str, u8); 3] = [("High", 1), ("Medium", 5), ("Low", 9)];

/// The same bargain for `Repeat`: option name paired with the RRULE it means.
pub const REPEAT_OPTIONS: [(&str, &str); 4] = [
    ("Daily", "FREQ=DAILY"),
    ("Weekly", "FREQ=WEEKLY"),
    ("Monthly", "FREQ=MONTHLY"),
    ("Yearly", "FREQ=YEARLY"),
];

/// A calendar field that can be backed by a Notion property.
///
/// The date property is deliberately absent: it is mandatory, lives in its own
/// `calendars.date_property` column, and is interpolated into the `filter` and
/// `sorts` of every Notion query rather than read off a returned page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Title,
    Location,
    Notes,
    Priority,
    Busy,
    Reminder,
    TravelMinutes,
    Repeat,
    Attendees,
}

impl Field {
    pub const ALL: [Field; 9] = [
        Field::Title,
        Field::Location,
        Field::Notes,
        Field::Priority,
        Field::Busy,
        Field::Reminder,
        Field::TravelMinutes,
        Field::Repeat,
        Field::Attendees,
    ];

    /// Stable identifier used in form field names and the stored JSON.
    pub fn key(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Location => "location",
            Field::Notes => "notes",
            Field::Priority => "priority",
            Field::Busy => "busy",
            Field::Reminder => "reminder",
            Field::TravelMinutes => "travel_minutes",
            Field::Repeat => "repeat",
            Field::Attendees => "attendees",
        }
    }

    pub fn from_key(key: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|f| f.key() == key)
    }

    /// Label shown next to the field's dropdown.
    pub fn label(self) -> &'static str {
        match self {
            Field::Title => "Event title",
            Field::Location => "Location",
            Field::Notes => "Description",
            Field::Priority => "Priority",
            Field::Busy => "Busy / free",
            Field::Reminder => "Reminder (minutes)",
            Field::TravelMinutes => "Travel time (minutes)",
            Field::Repeat => "Repeat",
            Field::Attendees => "Attendees",
        }
    }

    /// The English name this field was hardcoded to before mapping existed.
    ///
    /// `Busy` has two, because it accepts either a `checkbox` named "Busy" or a
    /// `select` named "Show As" whose value is "busy".
    pub fn legacy_names(self) -> &'static [&'static str] {
        match self {
            Field::Title => &["Name"],
            Field::Location => &["Location"],
            Field::Notes => &["Notes"],
            Field::Priority => &["Priority"],
            Field::Busy => &["Busy", "Show As"],
            Field::Reminder => &["Reminder"],
            Field::TravelMinutes => &["Travel time"],
            Field::Repeat => &["Repeat"],
            Field::Attendees => &["Attendees"],
        }
    }

    /// What the user needs to know about the values this field expects —
    /// shown under its dropdown, because picking a correctly-typed column is
    /// not enough when the values themselves have to match.
    pub fn type_hint(self) -> &'static str {
        match self {
            Field::Title => "Text shown as the event name",
            Field::Location => "Text or URL",
            Field::Notes => "Text, shown as the event description",
            Field::Priority => "Select with options High, Medium or Low",
            Field::Busy => "Checkbox, or a select with options Busy and Free",
            Field::Reminder => "Number of minutes before the event",
            Field::TravelMinutes => "Number of minutes of travel time",
            Field::Repeat => "Select with options Daily, Weekly, Monthly or Yearly",
            Field::Attendees => "People, or text with emails separated by commas",
        }
    }

    /// The Notion property type to create for this field when the app builds
    /// the database itself.
    ///
    /// Deliberately narrower than [`Field::accepts_type`]: that one says what
    /// an existing column of the user's may be, which is a question with
    /// several right answers. This one is the single type we pick when there
    /// is no column yet and nobody to ask.
    pub fn create_type(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Location | Field::Notes => "rich_text",
            Field::Priority | Field::Repeat => "select",
            Field::Busy => "checkbox",
            Field::Reminder | Field::TravelMinutes => "number",
            Field::Attendees => "people",
        }
    }

    /// The column name the app gives this field in a database it creates.
    ///
    /// The legacy English name on purpose, and not only for familiarity: a
    /// database named this way is read correctly by the pre-mapping guess in
    /// [`ColumnMapping::name_for`], so it still syncs if its mapping row is
    /// ever lost. Localising the names would be friendlier and would give that
    /// up, so it waits for someone to decide the trade is worth it.
    pub fn created_name(self) -> &'static str {
        self.legacy_names()[0]
    }

    /// Notion property types this field can read from, used both to filter the
    /// dropdown and to reject a mapping that points at an unusable column.
    pub fn accepts_type(self, notion_type: &str) -> bool {
        match self {
            Field::Title => notion_type == "title",
            Field::Location => matches!(notion_type, "rich_text" | "url"),
            Field::Notes => notion_type == "rich_text",
            Field::Priority | Field::Repeat => notion_type == "select",
            Field::Busy => matches!(notion_type, "checkbox" | "select"),
            Field::Reminder | Field::TravelMinutes => notion_type == "number",
            Field::Attendees => matches!(notion_type, "people" | "rich_text"),
        }
    }
}

/// Build the schema shape the mapping helpers expect — keyed by lowercased
/// property name — from a plain `(name, type)` list.
///
/// `AppState::get_data_source_properties` already returns this shape; this is
/// for callers that carry the properties as an ordered list because they also
/// need to render them in a dropdown.
pub fn schema_from_pairs(pairs: &[(String, String)]) -> HashMap<String, (String, String)> {
    pairs
        .iter()
        .map(|(name, notion_type)| (name.to_lowercase(), (name.clone(), notion_type.clone())))
        .collect()
}

/// The date column of a database the app creates.
///
/// Not a [`Field`] -- the enum's doc explains why the date property is kept
/// out -- but a calendar without it syncs nothing, so a created database is
/// not finished until this exists.
pub const CREATED_DATE_PROPERTY: &str = "Date";

/// `initial_data_source.properties` for a database the app creates, built from
/// [`Field::ALL`] rather than written out by hand.
///
/// The point of generating it: a field added to the enum shows up here with no
/// further edit, so the schema cannot fall behind the mapping UI that reads
/// it. A hand-written copy would drift the first time someone adds a field and
/// nothing would fail loudly enough to notice.
pub fn notion_property_schema() -> serde_json::Value {
    fn select_with(options: impl Iterator<Item = &'static str>) -> serde_json::Value {
        let options: Vec<_> = options
            .map(|name| serde_json::json!({ "name": name }))
            .collect();
        serde_json::json!({ "select": { "options": options } })
    }

    let mut props = serde_json::Map::new();
    props.insert(
        CREATED_DATE_PROPERTY.to_string(),
        serde_json::json!({ "date": {} }),
    );
    for field in Field::ALL {
        let body = match field {
            Field::Priority => select_with(PRIORITY_OPTIONS.iter().map(|(name, _)| *name)),
            Field::Repeat => select_with(REPEAT_OPTIONS.iter().map(|(name, _)| *name)),
            other => {
                let mut body = serde_json::Map::new();
                body.insert(other.create_type().to_string(), serde_json::json!({}));
                serde_json::Value::Object(body)
            }
        };
        props.insert(field.created_name().to_string(), body);
    }
    serde_json::Value::Object(props)
}

/// The mapping that matches [`notion_property_schema`] exactly.
///
/// Marked `configured`, because it is: the app chose every one of these names
/// when it created the columns, so there is nothing left to ask the user and
/// the mapping step can be skipped entirely for such a calendar.
pub fn mapping_for_created_schema() -> ColumnMapping {
    let mut mapping = ColumnMapping {
        configured: true,
        ..ColumnMapping::default()
    };
    for field in Field::ALL {
        mapping.set(field, Some(field.created_name().to_string()));
    }
    mapping
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMapping {
    /// False for every calendar created before the mapping UI shipped. While
    /// false, a field with no explicit name falls back to its legacy English
    /// guess; once true, an absent field means the user chose "none".
    #[serde(default)]
    pub configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub travel_minutes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attendees: Option<String>,
}

impl ColumnMapping {
    pub fn get(&self, field: Field) -> Option<&str> {
        let slot = match field {
            Field::Title => &self.title,
            Field::Location => &self.location,
            Field::Notes => &self.notes,
            Field::Priority => &self.priority,
            Field::Busy => &self.busy,
            Field::Reminder => &self.reminder,
            Field::TravelMinutes => &self.travel_minutes,
            Field::Repeat => &self.repeat,
            Field::Attendees => &self.attendees,
        };
        slot.as_deref()
    }

    pub fn set(&mut self, field: Field, name: Option<String>) {
        let slot = match field {
            Field::Title => &mut self.title,
            Field::Location => &mut self.location,
            Field::Notes => &mut self.notes,
            Field::Priority => &mut self.priority,
            Field::Busy => &mut self.busy,
            Field::Reminder => &mut self.reminder,
            Field::TravelMinutes => &mut self.travel_minutes,
            Field::Repeat => &mut self.repeat,
            Field::Attendees => &mut self.attendees,
        };
        *slot = name.filter(|n| !n.is_empty());
    }

    /// Property names to try for `field`, in order.
    ///
    /// An explicit mapping wins. An unconfigured calendar falls back to the
    /// legacy English names so its behaviour does not change the moment this
    /// column is added. A configured calendar with nothing set for the field
    /// yields nothing — the user chose "none" and we must not resurrect the
    /// guess behind their back.
    pub fn names_for(&self, field: Field) -> Vec<&str> {
        match self.get(field) {
            Some(name) => vec![name],
            None if !self.configured => field.legacy_names().to_vec(),
            None => Vec::new(),
        }
    }

    /// Single best name for `field`, for callers that cannot try alternatives
    /// (the write path, which must name exactly one property).
    pub fn name_for(&self, field: Field) -> Option<&str> {
        self.names_for(field).first().copied()
    }

    /// Reproduce the pre-mapping guesses against a real data source schema.
    ///
    /// `schema` is keyed by lowercased property name, as returned by
    /// `AppState::get_data_source_properties`: `lowercased -> (real name, type)`.
    ///
    /// Used to preselect the dropdowns so a user only corrects what is wrong,
    /// and to backfill calendars that predate the feature.
    pub fn guess_from_schema(schema: &HashMap<String, (String, String)>) -> ColumnMapping {
        let mut mapping = ColumnMapping {
            configured: true,
            ..Default::default()
        };

        for field in Field::ALL {
            let guess = field.legacy_names().iter().find_map(|legacy| {
                let (real_name, notion_type) = schema.get(&legacy.to_lowercase())?;
                field.accepts_type(notion_type).then(|| real_name.clone())
            });
            debug!(
                field = field.key(),
                guessed = guess.as_deref().unwrap_or("<none>"),
                "column mapping guess"
            );
            mapping.set(field, guess);
        }

        // Every Notion database has exactly one title property, and it is
        // frequently not called "Name" — that mismatch is what made writes
        // fail. Fall back to whichever property is actually the title.
        if mapping.title.is_none() {
            mapping.title = schema
                .values()
                .find(|(_, notion_type)| notion_type == "title")
                .map(|(real_name, _)| real_name.clone());
            debug!(
                resolved = mapping.title.as_deref().unwrap_or("<none>"),
                "title not named \"Name\", fell back to the title-typed property"
            );
        }

        debug!(
            schema_size = schema.len(),
            mapped = Field::ALL
                .iter()
                .filter(|f| mapping.get(**f).is_some())
                .count(),
            "column mapping guessed from schema"
        );
        mapping
    }

    /// Drop any mapping that names a property the data source does not have,
    /// or has with an incompatible type.
    ///
    /// Form input is not trusted: the posted names are checked against a
    /// freshly fetched schema before they reach the database.
    pub fn retain_valid(&mut self, schema: &HashMap<String, (String, String)>) {
        for field in Field::ALL {
            let Some(name) = self.get(field) else {
                continue;
            };
            let found = schema.get(&name.to_lowercase());
            let valid = found.is_some_and(|(_, notion_type)| field.accepts_type(notion_type));
            if !valid {
                warn!(
                    field = field.key(),
                    requested = name,
                    actual_type = found.map(|(_, t)| t.as_str()).unwrap_or("<missing>"),
                    "dropping column mapping: property missing or wrong type"
                );
                self.set(field, None);
            }
        }
    }
}
