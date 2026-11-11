//! The icons the components draw, as the webview's (lucide's) paths, in
//! `currentColor`.

use dioxus_native::prelude::*;

/// An icon: lucide's 24×24 path data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icon(pub &'static [&'static str]);

pub const CHECK: Icon = Icon(&["M20 6 9 17l-5-5"]);
pub const MINUS: Icon = Icon(&["M5 12h14"]);
pub const CHEVRON_DOWN: Icon = Icon(&["m6 9 6 6 6-6"]);
pub const X: Icon = Icon(&["M18 6 6 18", "m6 6 12 12"]);
const CIRCLE: &str = "M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z";
pub const CIRCLE_CHECK: Icon = Icon(&[CIRCLE, "m9 12 2 2 4-4"]);
pub const CIRCLE_SLASH: Icon = Icon(&[CIRCLE, "M9 15 15 9"]);
pub const CIRCLE_ALERT: Icon = Icon(&[CIRCLE, "M12 8v4", "M12 16h.01"]);
pub const CIRCLE_X: Icon = Icon(&[CIRCLE, "m15 9-6 6", "m9 9 6 6"]);
pub const SHOPPING_CART: Icon = Icon(&[
    "M8 20a1 1 0 1 0 0 2a1 1 0 1 0 0-2z",
    "M19 20a1 1 0 1 0 0 2a1 1 0 1 0 0-2z",
    "M2.05 2.05h2l2.66 12.42a2 2 0 0 0 2 1.58h9.78a2 2 0 0 0 1.95-1.57l1.65-7.43H5.12",
]);
pub const SEARCH: Icon = Icon(&["M11 3a8 8 0 1 0 0 16a8 8 0 1 0 0-16z", "m21 21-4.3-4.3"]);
pub const HEART: Icon = Icon(&[
    "M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z",
]);
pub const CLOCK: Icon = Icon(&[CIRCLE, "M12 6v6l4 2"]);
pub const HARD_DRIVE: Icon = Icon(&[
    "M22 12H2",
    "M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z",
    "M6 16h.01",
    "M10 16h.01",
]);
pub const STORE: Icon = Icon(&[
    "m2 7 4.41-4.41A2 2 0 0 1 7.83 2h8.34a2 2 0 0 1 1.42.59L22 7",
    "M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8",
    "M15 22v-4a2 2 0 0 0-2-2h-2a2 2 0 0 0-2 2v4",
    "M2 7h20",
    "M22 7v3a2 2 0 0 1-2 2a2.7 2.7 0 0 1-1.59-.63.7.7 0 0 0-.82 0A2.7 2.7 0 0 1 16 12a2.7 2.7 0 0 1-1.59-.63.7.7 0 0 0-.82 0A2.7 2.7 0 0 1 12 12a2.7 2.7 0 0 1-1.59-.63.7.7 0 0 0-.82 0A2.7 2.7 0 0 1 8 12a2.7 2.7 0 0 1-1.59-.63.7.7 0 0 0-.82 0A2.7 2.7 0 0 1 4 12a2 2 0 0 1-2-2V7",
]);
pub const GAMEPAD: Icon = Icon(&[
    "M6 11h4",
    "M8 9v4",
    "M15 12h.01",
    "M18 10h.01",
    "M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0 0 17.32 5z",
]);

pub const PLAY: Icon = Icon(&["M6 3 20 12 6 21z"]);
pub const ELLIPSIS: Icon = Icon(&[
    "M12 11a1 1 0 1 0 0 2a1 1 0 1 0 0-2z",
    "M19 11a1 1 0 1 0 0 2a1 1 0 1 0 0-2z",
    "M5 11a1 1 0 1 0 0 2a1 1 0 1 0 0-2z",
]);
pub const ARROW_RIGHT: Icon = Icon(&["M5 12h14", "m12 5 7 7-7 7"]);

// The shell's (#96).
/// lucide's `house`.
pub const HOUSE: Icon = Icon(&[
    "M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8",
    "M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
]);
/// lucide's `layout-grid`.
pub const LAYOUT_GRID: Icon = Icon(&[
    "M4 3h5a1 1 0 0 1 1 1v5a1 1 0 0 1 -1 1h-5a1 1 0 0 1 -1 -1v-5a1 1 0 0 1 1 -1z",
    "M15 3h5a1 1 0 0 1 1 1v5a1 1 0 0 1 -1 1h-5a1 1 0 0 1 -1 -1v-5a1 1 0 0 1 1 -1z",
    "M15 14h5a1 1 0 0 1 1 1v5a1 1 0 0 1 -1 1h-5a1 1 0 0 1 -1 -1v-5a1 1 0 0 1 1 -1z",
    "M4 14h5a1 1 0 0 1 1 1v5a1 1 0 0 1 -1 1h-5a1 1 0 0 1 -1 -1v-5a1 1 0 0 1 1 -1z",
]);
/// lucide's `square-library`.
pub const SQUARE_LIBRARY: Icon = Icon(&[
    "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2z",
    "M7 7v10",
    "M11 7v10",
    "m15 7 2 10",
]);
/// lucide's `settings`.
pub const SETTINGS: Icon = Icon(&[
    "M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915",
    "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
]);
/// lucide's `bell`.
pub const BELL: Icon = Icon(&[
    "M10.268 21a2 2 0 0 0 3.464 0",
    "M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326",
]);
/// lucide's `loader-circle`.
pub const LOADER_CIRCLE: Icon = Icon(&["M21 12a9 9 0 1 1-6.219-8.56"]);
/// lucide's `circle-arrow-up`.
pub const CIRCLE_ARROW_UP: Icon = Icon(&[
    "M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z",
    "m16 12-4-4-4 4",
    "M12 16V8",
]);
/// lucide's `corner-down-left`.
pub const CORNER_DOWN_LEFT: Icon = Icon(&["M20 4v7a4 4 0 0 1-4 4H4", "m9 10-5 5 5 5"]);
/// lucide's `square`.
pub const SQUARE: Icon =
    Icon(&["M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2z"]);
/// lucide's `copy`.
pub const COPY: Icon = Icon(&[
    "M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1 -2 2h-10a2 2 0 0 1 -2 -2v-10a2 2 0 0 1 2 -2z",
    "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2",
]);

/// lucide's `check-check`.
pub const CHECK_CHECK: Icon = Icon(&["M18 6 7 17l-5-5", "m22 10-7.5 7.5L13 16"]);
/// lucide's `mail-open`.
pub const MAIL_OPEN: Icon = Icon(&[
    "M21.2 8.4c.5.38.8.97.8 1.6v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V10a2 2 0 0 1 .8-1.6l8-6a2 2 0 0 1 2.4 0l8 6Z",
    "m22 10-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 10",
]);
/// lucide's `mail`.
pub const MAIL: Icon = Icon(&[
    "m22 7-8.991 5.727a2 2 0 0 1-2.009 0L2 7",
    "M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-16a2 2 0 0 1 -2 -2v-12a2 2 0 0 1 2 -2z",
]);
/// lucide's `bell-dot`.
pub const BELL_DOT: Icon = Icon(&[
    "M10.268 21a2 2 0 0 0 3.464 0",
    "M11.68 2.009A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673c-.824-.85-1.678-1.731-2.21-3.348",
    "M18 2a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
]);

/// lucide's `share-2`.
pub const SHARE_2: Icon = Icon(&[
    "M18 2a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
    "M6 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
    "M18 16a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
    "M8.59 13.51 15.42 17.49",
    "M15.41 6.51 8.59 10.49",
]);

/// lucide's `send`.
pub const SEND: Icon = Icon(&[
    "M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z",
    "m21.854 2.147-10.94 10.939",
]);

/// lucide's `external-link`.
pub const EXTERNAL_LINK: Icon = Icon(&[
    "M15 3h6v6",
    "M10 14 21 3",
    "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6",
]);

// The library's (#99).
/// lucide's `square-check-big`.
pub const SQUARE_CHECK_BIG: Icon = Icon(&[
    "M21 10.656V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h12.344",
    "m9 11 3 3L22 4",
]);
/// lucide's `grid-2x2`.
pub const GRID_2X2: Icon = Icon(&[
    "M12 3v18",
    "M3 12h18",
    "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2z",
]);
/// lucide's `list`.
pub const LIST: Icon = Icon(&[
    "M3 5h.01",
    "M3 12h.01",
    "M3 19h.01",
    "M8 5h13",
    "M8 12h13",
    "M8 19h13",
]);
/// lucide's `search-x`.
pub const SEARCH_X: Icon = Icon(&[
    "m13.5 8.5-5 5",
    "m8.5 8.5 5 5",
    "M11 3a8 8 0 1 0 0 16a8 8 0 1 0 0-16z",
    "m21 21-4.3-4.3",
]);
/// lucide's `list-plus`.
pub const LIST_PLUS: Icon = Icon(&["M16 5H3", "M11 12H3", "M16 19H3", "M18 9v6", "M21 12h-6"]);
/// lucide's `arrow-left`.
pub const ARROW_LEFT: Icon = Icon(&["m12 19-7-7 7-7", "M19 12H5"]);
/// lucide's `cloud`.
pub const CLOUD: Icon = Icon(&["M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z"]);
/// lucide's `download`.
pub const DOWNLOAD: Icon = Icon(&[
    "M12 15V3",
    "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4",
    "m7 10 5 5 5-5",
]);
/// lucide's `folder-open`.
pub const FOLDER_OPEN: Icon = Icon(&[
    "m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2",
]);
/// lucide's `gamepad-2`.
pub const GAMEPAD_2: Icon = Icon(&[
    "M6 11L10 11",
    "M8 9L8 13",
    "M15 12L15.01 12",
    "M18 10L18.01 10",
    "M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0 0 17.32 5z",
]);
/// lucide's `globe`.
pub const GLOBE: Icon = Icon(&[
    "M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z",
    "M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20",
    "M2 12h20",
]);
/// lucide's `image-down`.
pub const IMAGE_DOWN: Icon = Icon(&[
    "M10.3 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v10l-3.1-3.1a2 2 0 0 0-2.814.014L6 21",
    "m14 19 3 3v-5.5",
    "m17 22 3-3",
    "M9 7a2 2 0 1 0 0 4a2 2 0 1 0 0-4z",
]);
/// lucide's `languages`.
pub const LANGUAGES: Icon = Icon(&[
    "m5 8 6 6",
    "m4 14 6-6 2-3",
    "M2 5h12",
    "M7 2h1",
    "m22 22-5-10-5 10",
    "M14 18h6",
]);
/// lucide's `log-out`.
pub const LOG_OUT: Icon = Icon(&[
    "m16 17 5-5-5-5",
    "M21 12H9",
    "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4",
]);
/// lucide's `message-square-warning`.
pub const MESSAGE_SQUARE_WARNING: Icon = Icon(&[
    "M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z",
    "M12 15h.01",
    "M12 7v4",
]);
/// lucide's `monitor-play`.
pub const MONITOR_PLAY: Icon = Icon(&[
    "M15.033 9.44a.647.647 0 0 1 0 1.12l-4.065 2.352a.645.645 0 0 1-.968-.56V7.648a.645.645 0 0 1 .967-.56z",
    "M12 17v4",
    "M8 21h8",
    "M4 3h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-16a2 2 0 0 1-2-2v-10a2 2 0 0 1 2-2z",
]);
/// lucide's `monitor-up`.
pub const MONITOR_UP: Icon = Icon(&[
    "m9 10 3-3 3 3",
    "M12 13V7",
    "M4 3h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-16a2 2 0 0 1-2-2v-10a2 2 0 0 1 2-2z",
    "M12 17v4",
    "M8 21h8",
]);
/// lucide's `power`.
pub const POWER: Icon = Icon(&["M12 2v10", "M18.4 6.6a9 9 0 1 1-12.77.04"]);
/// lucide's `rotate-cw`.
pub const ROTATE_CW: Icon = Icon(&[
    "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8",
    "M21 3v5h-5",
]);
/// lucide's `sparkles`.
pub const SPARKLES: Icon = Icon(&[
    "M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z",
    "M20 2v4",
    "M22 4h-4",
    "M4 18a2 2 0 1 0 0 4a2 2 0 1 0 0-4z",
]);
/// lucide's `rows-3`.
pub const ROWS_3: Icon = Icon(&[
    "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2z",
    "M21 9H3",
    "M21 15H3",
]);
/// lucide's `scaling`.
pub const SCALING: Icon = Icon(&[
    "M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7",
    "M14 15H9v-5",
    "M16 3h5v5",
    "M21 3 9 15",
]);
/// lucide's `droplet`.
pub const DROPLET: Icon = Icon(&[
    "M12 22a7 7 0 0 0 7-7c0-2-1-3.9-3-5.5s-3.5-4-4-6.5c-.5 2.5-2 4.9-4 6.5C6 11.1 5 13 5 15a7 7 0 0 0 7 7z",
]);
/// lucide's `image`.
pub const IMAGE: Icon = Icon(&[
    "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2z",
    "M7 9a2 2 0 1 0 4 0a2 2 0 1 0 -4 0z",
    "m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21",
]);
/// lucide's `rectangle-horizontal`.
pub const RECTANGLE_HORIZONTAL: Icon =
    Icon(&["M4 6h16a2 2 0 0 1 2 2v8a2 2 0 0 1 -2 2h-16a2 2 0 0 1 -2 -2v-8a2 2 0 0 1 2 -2z"]);
/// lucide's `captions`.
pub const CAPTIONS: Icon = Icon(&[
    "M5 5h14a2 2 0 0 1 2 2v10a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-10a2 2 0 0 1 2 -2z",
    "M7 15h4M15 15h2M7 11h2M13 11h4",
]);
/// lucide's `contrast`.
pub const CONTRAST: Icon = Icon(&[
    "M2 12a10 10 0 1 0 20 0a10 10 0 1 0 -20 0z",
    "M12 18a6 6 0 0 0 0-12v12z",
]);
/// lucide's `code`.
pub const CODE: Icon = Icon(&["m16 18 6-6-6-6", "m8 6-6 6 6 6"]);
/// lucide's `power-off`.
pub const POWER_OFF: Icon = Icon(&[
    "M18.36 6.64A9 9 0 0 1 20.77 15",
    "M6.16 6.16a9 9 0 1 0 12.68 12.68",
    "M12 2v4",
    "m2 2 20 20",
]);
/// lucide's `book-open`.
pub const BOOK_OPEN: Icon = Icon(&[
    "M12 5v16",
    "M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z",
]);
/// lucide's `magnet`.
pub const MAGNET: Icon = Icon(&[
    "m12 15 4 4",
    "M2.352 10.648a1.205 1.205 0 0 0 0 1.704l2.296 2.296a1.205 1.205 0 0 0 1.704 0l6.029-6.029a1 1 0 1 1 3 3l-6.029 6.029a1.205 1.205 0 0 0 0 1.704l2.296 2.296a1.205 1.205 0 0 0 1.704 0l6.365-6.367A1 1 0 0 0 8.716 4.282z",
    "m5 8 4 4",
]);
/// lucide's `wrench`.
pub const WRENCH: Icon = Icon(&[
    "M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.106-3.105c.32-.322.863-.22.983.218a6 6 0 0 1-8.259 7.057l-7.91 7.91a1 1 0 0 1-2.999-3l7.91-7.91a6 6 0 0 1 7.057-8.259c.438.12.54.662.219.984z",
]);

/// lucide's `rotate-ccw`.
pub const ROTATE_CCW: Icon = Icon(&[
    "M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8",
    "M3 3v5h5",
]);

// Settings' sections (#103).
/// lucide's `settings-2`.
pub const SETTINGS_2: Icon = Icon(&[
    "M14 17H5",
    "M19 7h-9",
    "M17 14a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
    "M7 4a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
]);
/// lucide's `palette`.
pub const PALETTE: Icon = Icon(&[
    "M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z",
    "M13.5 6a0.5 0.5 0 1 0 0 1a0.5 0.5 0 1 0 0-1z",
    "M17.5 10a0.5 0.5 0 1 0 0 1a0.5 0.5 0 1 0 0-1z",
    "M6.5 12a0.5 0.5 0 1 0 0 1a0.5 0.5 0 1 0 0-1z",
    "M8.5 7a0.5 0.5 0 1 0 0 1a0.5 0.5 0 1 0 0-1z",
]);
/// lucide's `puzzle`.
pub const PUZZLE: Icon = Icon(&[
    "M15.39 4.39a1 1 0 0 0 1.68-.474 2.5 2.5 0 1 1 3.014 3.015 1 1 0 0 0-.474 1.68l1.683 1.682a2.414 2.414 0 0 1 0 3.414L19.61 15.39a1 1 0 0 1-1.68-.474 2.5 2.5 0 1 0-3.014 3.015 1 1 0 0 1 .474 1.68l-1.683 1.682a2.414 2.414 0 0 1-3.414 0L8.61 19.61a1 1 0 0 0-1.68.474 2.5 2.5 0 1 1-3.014-3.015 1 1 0 0 0 .474-1.68l-1.683-1.682a2.414 2.414 0 0 1 0-3.414L4.39 8.61a1 1 0 0 1 1.68.474 2.5 2.5 0 1 0 3.014-3.015 1 1 0 0 1-.474-1.68l1.683-1.682a2.414 2.414 0 0 1 3.414 0z",
]);
/// lucide's `scan-search`.
pub const SCAN_SEARCH: Icon = Icon(&[
    "M3 7V5a2 2 0 0 1 2-2h2",
    "M17 3h2a2 2 0 0 1 2 2v2",
    "M21 17v2a2 2 0 0 1-2 2h-2",
    "M7 21H5a2 2 0 0 1-2-2v-2",
    "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
    "m16 16-1.9-1.9",
]);
/// lucide's `rocket`.
pub const ROCKET: Icon = Icon(&[
    "M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5",
    "M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09",
    "M9 12a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.4 22.4 0 0 1-4 2z",
    "M9 12H4s.55-3.03 2-4c1.62-1.08 5 .05 5 .05",
]);
/// lucide's `package-open`.
pub const PACKAGE_OPEN: Icon = Icon(&[
    "M12 22v-9",
    "M15.17 2.21a1.67 1.67 0 0 1 1.63 0L21 4.57a1.93 1.93 0 0 1 0 3.36L8.82 14.79a1.655 1.655 0 0 1-1.64 0L3 12.43a1.93 1.93 0 0 1 0-3.36z",
    "M20 13v3.87a2.06 2.06 0 0 1-1.11 1.83l-6 3.08a1.93 1.93 0 0 1-1.78 0l-6-3.08A2.06 2.06 0 0 1 4 16.87V13",
    "M21 12.43a1.93 1.93 0 0 0 0-3.36L8.83 2.2a1.64 1.64 0 0 0-1.63 0L3 4.57a1.93 1.93 0 0 0 0 3.36l12.18 6.86a1.636 1.636 0 0 0 1.63 0z",
]);
/// lucide's `square-terminal`.
pub const SQUARE_TERMINAL: Icon = Icon(&[
    "m7 11 2-2-2-2",
    "M11 13h4",
    "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-14a2 2 0 0 1-2-2v-14a2 2 0 0 1 2-2z",
]);
/// lucide's `monitor-smartphone`.
pub const MONITOR_SMARTPHONE: Icon = Icon(&[
    "M18 8V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h8",
    "M10 19v-3.96 3.15",
    "M7 19h5",
    "M18 12h2a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2h-2a2 2 0 0 1-2-2v-6a2 2 0 0 1 2-2z",
]);
/// lucide's `cast`.
pub const CAST: Icon = Icon(&[
    "M2 8V6a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-6",
    "M2 12a9 9 0 0 1 8 8",
    "M2 16a5 5 0 0 1 4 4",
    "M2 20L2.01 20",
]);
/// lucide's `user-round`.
pub const USER_ROUND: Icon = Icon(&[
    "M12 3a5 5 0 1 0 0 10a5 5 0 1 0 0-10z",
    "M20 21a8 8 0 0 0-16 0",
]);
/// lucide's `users`.
pub const USERS: Icon = Icon(&[
    "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2",
    "M16 3.128a4 4 0 0 1 0 7.744",
    "M22 21v-2a4 4 0 0 0-3-3.87",
    "M9 3a4 4 0 1 0 0 8a4 4 0 1 0 0-8z",
]);
/// lucide's `database-backup`.
pub const DATABASE_BACKUP: Icon = Icon(&[
    "M12 2a9 3 0 1 0 0 6a9 3 0 1 0 0-6z",
    "M3 12a9 3 0 0 0 5 2.69",
    "M21 9.3V5",
    "M3 5v14a9 3 0 0 0 6.47 2.88",
    "M12 12v4h4",
    "M13 20a5 5 0 0 0 9-3 4.5 4.5 0 0 0-4.5-4.5c-1.33 0-2.54.54-3.41 1.41L12 16",
]);
/// lucide's `activity`.
pub const ACTIVITY: Icon = Icon(&[
    "M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2",
]);
/// lucide's `info`.
pub const INFO: Icon = Icon(&[
    "M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z",
    "M12 16v-4",
    "M12 8h.01",
]);
/// lucide's `refresh-cw`.
pub const REFRESH_CW: Icon = Icon(&[
    "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8",
    "M21 3v5h-5",
    "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16",
    "M8 16H3v5",
]);

/// lucide's `plus`.
pub const PLUS: Icon = Icon(&["M5 12h14", "M12 5v14"]);
/// lucide's `file-search`.
pub const FILE_SEARCH: Icon = Icon(&[
    "M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z",
    "M14 2v5a1 1 0 0 0 1 1h5",
    "M11.5 12a2.5 2.5 0 1 0 0 5a2.5 2.5 0 1 0 0-5z",
    "M13.3 16.3 15 18",
]);

/// lucide's `arrow-up`.
pub const ARROW_UP: Icon = Icon(&["m5 12 7-7 7 7", "M12 19V5"]);
/// lucide's `file`.
pub const FILE: Icon = Icon(&[
    "M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z",
    "M14 2v5a1 1 0 0 0 1 1h5",
]);
/// lucide's `folder`.
pub const FOLDER: Icon = Icon(&[
    "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z",
]);

// The game menu's (#99).
/// lucide's `heart-off`.
pub const HEART_OFF: Icon = Icon(&[
    "M10.5 4.893a5.5 5.5 0 0 1 1.091.931.56.56 0 0 0 .818 0A5.49 5.49 0 0 1 22 9.5c0 1.872-1.002 3.356-2.187 4.655",
    "m16.967 16.967-3.459 3.346a2 2 0 0 1-3 .019L5 15c-1.5-1.5-3-3.2-3-5.5a5.5 5.5 0 0 1 2.747-4.761",
    "m2 2 20 20",
]);
/// lucide's `pencil`.
pub const PENCIL: Icon = Icon(&[
    "M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z",
    "m15 5 4 4",
]);
/// lucide's `folder-input`.
pub const FOLDER_INPUT: Icon = Icon(&[
    "M2 9V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-1",
    "M2 13h10",
    "m9 16 3-3-3-3",
]);
/// lucide's `trash`.
pub const TRASH: Icon = Icon(&[
    "M10 11v6",
    "M14 11v6",
    "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6",
    "M3 6h18",
    "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
]);
/// lucide's `eye-off`.
pub const EYE_OFF: Icon = Icon(&[
    "M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49",
    "M14.084 14.158a3 3 0 0 1-4.242-4.242",
    "M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143",
    "m2 2 20 20",
]);
/// lucide's `file-plus-corner`.
pub const FILE_PLUS_CORNER: Icon = Icon(&[
    "M11.35 22H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.706.706l3.588 3.588A2.4 2.4 0 0 1 20 8v5.35",
    "M14 2v5a1 1 0 0 0 1 1h5",
    "M14 19h6",
    "M17 16v6",
]);
/// lucide's `history` (`rotate-ccw-clock`).
pub const HISTORY: Icon = Icon(&[
    "M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8",
    "M3 3v5h5",
    "M12 7v5l4 2",
]);
/// lucide's `cloud-off`.
pub const CLOUD_OFF: Icon = Icon(&[
    "M10.94 5.274A7 7 0 0 1 15.71 10h1.79a4.5 4.5 0 0 1 4.222 6.057",
    "M18.796 18.81A4.5 4.5 0 0 1 17.5 19H9A7 7 0 0 1 5.79 5.78",
    "m2 2 20 20",
]);
/// lucide's `list-checks`.
pub const LIST_CHECKS: Icon = Icon(&[
    "M13 5h8",
    "M13 12h8",
    "M13 19h8",
    "m3 17 2 2 4-4",
    "m3 7 2 2 4-4",
]);

/// Draws `icon` with `class`, `stroke_width` (lucide's default is 2) and
/// `data-part` when given.
#[component]
pub fn Svg(
    icon: Icon,
    #[props(default)] class: String,
    #[props(default = 2.0)] stroke_width: f32,
    #[props(default)] part: Option<&'static str>,
) -> Element {
    rsx! {
        svg {
            "data-part": part,
            "aria-hidden": "true",
            class: "{class}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "{stroke_width}",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            for d in icon.0 {
                path { d: *d }
            }
        }
    }
}
