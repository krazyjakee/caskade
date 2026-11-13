//! Settings sections and rows.

use super::*;

#[component]
fn Settings() -> Element {
    rsx! {
        InputRoot {
            SettingsSection {
                id: "startup",
                title: "Startup",
                description: "What happens when Caskade opens.",
                aside: rsx! { StatusDot { tone: DotTone::Ok, "Detected" } },
                SettingRow { label: "Launch on system startup", description: "Start Caskade when you log in.",
                    Switch { id: "launch", checked: true }
                }
                SettingRow { label: "Sort by",
                    Select {
                        id: "sort",
                        options: vec![SelectOption::new("title", "Title")],
                        value: Some("title".to_owned()),
                        on_change: move |_| {},
                    }
                }
                SettingRow { label: "Volume",
                    Slider { id: "volume", value: 40.0, on_change: move |_| {} }
                }
                SettingRow { label: "Nickname",
                    TextField { id: "nickname", value: "Ada", on_input: move |_| {} }
                }
            }
        }
    }
}

fn settings() -> Element {
    rsx! { Settings {} }
}

#[test]
fn a_setting_row_names_and_describes_its_controls() {
    let _one = one_window();
    let window = Window::open(settings);
    let heard = window.heard("launch");
    assert!(
        heard.starts_with("Switch \"Launch on system startup\""),
        "{heard}"
    );
    assert!(
        window.attr("launch", "aria-describedby").is_some(),
        "the row's description"
    );
    assert!(
        window.heard("sort").contains("\"Sort by Title\""),
        "{}",
        window.heard("sort")
    );
    assert!(
        window.heard("nickname").contains("\"Nickname\""),
        "{}",
        window.heard("nickname")
    );
    let heard = window.heard("startup");
    assert!(heard.contains("\"Startup\""), "{heard}");
    assert!(heard.contains("Detected"), "{heard}");
}

#[component]
fn TableRow() -> Element {
    rsx! {
        InputRoot {
            GameRow {
                id: "row",
                game: CardGame {
                    id: "1".into(),
                    title: "Hollow Pines".into(),
                    ..CardGame::default()
                },
                table: true,
            }
        }
    }
}

fn table_row() -> Element {
    rsx! { TableRow {} }
}

#[test]
fn a_game_row_in_a_tables_columns_is_one_cell_per_column() {
    let _one = one_window();
    let window = Window::open(table_row);
    let doc = window.doc.inner.borrow();
    let row = doc.get_node(window.id("row")).unwrap();
    let cells: Vec<String> = row
        .children
        .iter()
        .filter_map(|&child| doc.get_node(child).filter(|node| node.is_element()))
        .map(|node| {
            node.attr(blitz_dom::LocalName::from("class"))
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(
        cells.len(),
        6,
        "title, status, last played, playtime, size, actions"
    );
    // Each cell after the title is as wide as its column in TABLE_COLUMNS
    // (`7.5rem` is Tailwind's `w-30`), so rows line up with the header.
    let widths: Vec<String> = crate::ui::TABLE_COLUMNS
        .split(' ')
        .skip(1)
        .map(|rem| {
            let rem: f32 = rem.trim_end_matches("rem").parse().unwrap();
            format!("w-{}", rem * 4.0)
        })
        .collect();
    for (cell, width) in cells[1..].iter().zip(&widths) {
        assert!(
            cell.split(' ').any(|class| class == width),
            "{width} in {cell:?}"
        );
    }
    assert!(cells[0].contains("flex-1"), "{:?}", cells[0]);
}
