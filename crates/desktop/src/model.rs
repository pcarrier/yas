//! Protocol-neutral desktop backend records.
//!
//! These values describe the private D-Bus services owned by this crate. The
//! YAS server maps their process-local identifiers to boot-scoped public
//! handles; no wire framing or public-protocol identifier lives here.

pub const TRAY_STATUS_PASSIVE: u8 = 0;
pub const TRAY_STATUS_ACTIVE: u8 = 1;
pub const TRAY_STATUS_NEEDS_ATTENTION: u8 = 2;

pub const TRAY_CATEGORY_APPLICATION_STATUS: u8 = 0;
pub const TRAY_CATEGORY_COMMUNICATIONS: u8 = 1;
pub const TRAY_CATEGORY_SYSTEM_SERVICE: u8 = 2;
pub const TRAY_CATEGORY_HARDWARE: u8 = 3;
pub const TRAY_CATEGORY_UNKNOWN: u8 = 255;

pub const TRAY_HAS_MENU: u8 = 1 << 0;
pub const TRAY_ITEM_IS_MENU: u8 = 1 << 1;

pub const TRAY_MENU_OK: u8 = 0;
pub const TRAY_MENU_NONE: u8 = 1;
pub const TRAY_MENU_UNAVAILABLE: u8 = 2;
pub const TRAY_MENU_STALE: u8 = 3;

pub const MENU_NODE_VISIBLE: u16 = 1 << 0;
pub const MENU_NODE_ENABLED: u16 = 1 << 1;
pub const MENU_NODE_SEPARATOR: u16 = 1 << 2;
pub const MENU_NODE_SUBMENU: u16 = 1 << 3;
pub const MENU_NODE_CHECKMARK: u16 = 1 << 4;
pub const MENU_NODE_RADIO: u16 = 1 << 5;

pub const NOTIFICATION_RESIDENT: u8 = 1 << 0;
pub const NOTIFICATION_TRANSIENT: u8 = 1 << 1;
pub const NOTIFICATION_URGENCY_NORMAL: u8 = 1;
pub const NOTIFICATION_URGENCY_CRITICAL: u8 = 2;

pub const NOTIFICATION_CLOSED_EXPIRED: u8 = 1;
pub const NOTIFICATION_CLOSED_DISMISSED: u8 = 2;
pub const NOTIFICATION_CLOSED_BY_CALLER: u8 = 3;
pub const NOTIFICATION_CLOSED_UNDEFINED: u8 = 4;

pub const MPRIS_PLAYER_MAX: usize = 32;
pub const MPRIS_ARTIST_MAX: usize = 16;
pub const MPRIS_STRING_MAX: usize = 4 * 1024;
pub const MPRIS_ARTWORK_MAX: usize = 512 * 1024;

pub const MPRIS_CAN_CONTROL: u16 = 1 << 0;
pub const MPRIS_CAN_PLAY: u16 = 1 << 1;
pub const MPRIS_CAN_PAUSE: u16 = 1 << 2;
pub const MPRIS_CAN_GO_NEXT: u16 = 1 << 3;
pub const MPRIS_CAN_GO_PREVIOUS: u16 = 1 << 4;
pub const MPRIS_CAN_SEEK: u16 = 1 << 5;
pub const MPRIS_CAN_RAISE: u16 = 1 << 6;
pub const MPRIS_CAN_SET_VOLUME: u16 = 1 << 7;
pub const MPRIS_CAN_SET_SHUFFLE: u16 = 1 << 8;
pub const MPRIS_CAN_SET_LOOP_STATUS: u16 = 1 << 9;
pub const MPRIS_CAN_SET_RATE: u16 = 1 << 10;

// Backend command outcomes. The server converts these semantic outcomes into
// the corresponding YAS Result status; they are not packet status bytes.
pub const STATUS_OK: u8 = 0;
pub const STATUS_UNKNOWN_ID: u8 = 1;
pub const STATUS_WRONG_TYPE: u8 = 3;
pub const STATUS_BUDGET: u8 = 6;
pub const STATUS_INVALID: u8 = 7;
pub const STATUS_OTHER: u8 = 9;
pub const STATUS_CONFLICT: u8 = 11;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PngImage {
    pub width: u16,
    pub height: u16,
    pub png: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayItem {
    pub tray_id: u32,
    pub revision: u32,
    pub status: u8,
    pub category: u8,
    pub flags: u8,
    pub app_id: String,
    pub title: String,
    pub tooltip_title: String,
    pub tooltip_body: String,
    pub icon: PngImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayRecord {
    Upsert(TrayItem),
    Delete { tray_id: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuNode {
    pub id: i32,
    pub parent_id: i32,
    pub position: u16,
    pub flags: u16,
    pub toggle_state: i8,
    pub label: String,
    pub icon: PngImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenu {
    pub tray_id: u32,
    pub tray_revision: u32,
    pub menu_revision: u32,
    pub status: u8,
    pub nodes: Vec<MenuNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationAction {
    pub key: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub notification_id: u32,
    pub revision: u32,
    pub urgency: u8,
    pub flags: u8,
    pub timeout_ms: u32,
    pub app_name: String,
    pub desktop_entry: String,
    pub summary: String,
    pub body: String,
    pub icon: PngImage,
    pub image: PngImage,
    pub actions: Vec<NotificationAction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationRecord {
    Upsert(Notification),
    Delete {
        notification_id: u32,
        revision: u32,
        reason: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PlaybackStatus {
    Stopped = 0,
    Paused = 1,
    Playing = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LoopStatus {
    None = 0,
    Track = 1,
    Playlist = 2,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MprisArtwork {
    #[default]
    None,
    Url(String),
    Png(Vec<u8>),
}

impl MprisArtwork {
    pub fn png_len(&self) -> usize {
        match self {
            Self::Png(png) => png.len(),
            Self::None | Self::Url(_) => 0,
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

pub fn artwork_url_allowed(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    !rest.is_empty()
        && url.len() <= MPRIS_STRING_MAX
        && (scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MprisPlayer {
    pub player_id: u32,
    pub revision: u32,
    pub track_revision: u32,
    pub active: bool,
    pub playback_status: PlaybackStatus,
    pub loop_status: LoopStatus,
    pub shuffle: bool,
    pub capability_flags: u16,
    pub rate_ppm: i32,
    pub minimum_rate_ppm: i32,
    pub maximum_rate_ppm: i32,
    pub volume_ppm: u32,
    pub position_us: i64,
    pub length_us: i64,
    pub identity: String,
    pub desktop_entry: String,
    pub title: String,
    pub album: String,
    pub artists: Vec<String>,
    pub artwork: MprisArtwork,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MprisRecord {
    Delete { player_id: u32 },
    Upsert(MprisPlayer),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MprisActionResult {
    pub nonce: u32,
    pub status: u8,
    pub player_id: u32,
    pub revision: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalChoiceValue {
    pub id: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalChoice {
    pub id: String,
    pub label: String,
    pub options: Vec<PortalChoiceValue>,
    pub initial_value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalAccessRequest {
    pub request_id: u32,
    pub deadline_ms: u32,
    pub parent_surface_id: Option<u16>,
    pub app_id: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub deny_label: String,
    pub grant_label: String,
    pub icon_name: String,
    pub choices: Vec<PortalChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenCastCandidate {
    pub surface_id: u16,
    pub width: u16,
    pub height: u16,
    pub title: String,
    pub app_id: String,
    pub thumbnail_png: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalScreenCastRequest {
    pub request_id: u32,
    pub deadline_ms: u32,
    pub parent_surface_id: Option<u16>,
    pub app_id: String,
    pub multiple: bool,
    pub candidates: Vec<ScreenCastCandidate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalRequest {
    Access(PortalAccessRequest),
    ScreenCast(PortalScreenCastRequest),
}
