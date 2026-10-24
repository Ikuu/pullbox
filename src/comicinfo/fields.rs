//! The simple (text-valued) ComicInfo.xml elements, in the order ComicRack
//! writes them. Taken from ComicRackCE's `ComicRack.Engine/ComicInfo.cs`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    /// Long free text, edited in a multi-line box.
    MultiLine,
    Integer,
    Decimal,
    /// One of a fixed set of values.
    Choice(&'static [&'static str]),
}

impl FieldKind {
    /// Whether `value` (already trimmed) is acceptable for this field.
    /// An empty value is always valid and means "not set".
    pub fn is_valid(self, value: &str) -> bool {
        if value.is_empty() {
            return true;
        }
        match self {
            FieldKind::Text | FieldKind::MultiLine => true,
            FieldKind::Integer => value.parse::<i32>().is_ok(),
            FieldKind::Decimal => value.parse::<f32>().is_ok(),
            FieldKind::Choice(options) => options.contains(&value),
        }
    }
}

pub struct FieldDef {
    /// XML element name.
    pub name: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
}

const YES_NO: &[&str] = &["Unknown", "No", "Yes"];
const MANGA: &[&str] = &["Unknown", "No", "Yes", "YesAndRightToLeft"];
const AGE_RATING: &[&str] = &[
    "Unknown",
    "Adults Only 18+",
    "Early Childhood",
    "Everyone",
    "Everyone 10+",
    "G",
    "Kids to Adults",
    "M",
    "MA15+",
    "Mature 17+",
    "PG",
    "R18+",
    "Rating Pending",
    "Teen",
    "X18+",
];

const fn field(name: &'static str, label: &'static str, kind: FieldKind) -> FieldDef {
    FieldDef { name, label, kind }
}

use FieldKind::*;

pub const FIELDS: &[FieldDef] = &[
    field("Title", "Title", Text),
    field("Series", "Series", Text),
    field("LocalizedSeries", "Localized Series", Text),
    field("Number", "Number", Text),
    field("Count", "of", Integer),
    field("Volume", "Volume", Integer),
    field("AlternateSeries", "Alternate Series", Text),
    field("AlternateNumber", "Alt. Number", Text),
    field("StoryArc", "Story Arc", Text),
    field("SeriesGroup", "Series Group", Text),
    field("AlternateCount", "of", Integer),
    field("Summary", "Summary", MultiLine),
    field("Notes", "Notes", MultiLine),
    field("Review", "Review", MultiLine),
    field("Year", "Year", Integer),
    field("Month", "Month", Integer),
    field("Day", "Day", Integer),
    field("Writer", "Writer", Text),
    field("Penciller", "Penciller", Text),
    field("Inker", "Inker", Text),
    field("Colorist", "Colorist", Text),
    field("Letterer", "Letterer", Text),
    field("CoverArtist", "Cover Artist", Text),
    field("Editor", "Editor", Text),
    field("Translator", "Translator", Text),
    field("Publisher", "Publisher", Text),
    field("Imprint", "Imprint", Text),
    field("Genre", "Genre", Text),
    field("Web", "Web", Text),
    field("PageCount", "Pages", Integer),
    field("LanguageISO", "Language (ISO)", Text),
    field("Format", "Format", Text),
    field("AgeRating", "Age Rating", Choice(AGE_RATING)),
    field("BlackAndWhite", "Black & White", Choice(YES_NO)),
    field("Manga", "Manga", Choice(MANGA)),
    field("Characters", "Characters", Text),
    field("Teams", "Teams", Text),
    field("MainCharacterOrTeam", "Main Character/Team", Text),
    field("Locations", "Locations", Text),
    field("CommunityRating", "Community Rating", Decimal),
    field("ScanInformation", "Scan Information", Text),
    field("Tags", "Tags", Text),
];

/// Position of `name` in the canonical write order.
pub fn order_of(name: &str) -> Option<usize> {
    FIELDS.iter().position(|f| f.name == name)
}
